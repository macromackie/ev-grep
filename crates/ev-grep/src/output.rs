use core::mem;
use std::{
    io::{self, Write},
    process::ExitCode,
};

use clap::ValueEnum;
use ev_grep_core::{
    Assessment,
    Candidate,
    ErrorKind,
    LineRange,
    MAX_FILE_BYTES,
    Outcome,
    ScanEvent,
    SkipReason,
    Uncertainty,
    label,
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::inputs::StateInput;

/// The version of the JSON Lines record shapes, which every record names.
const SCHEMA_VERSION: u32 = 2;

/// How a run ends, as the process exit status reports it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExitStatus {
    /// An input, provider, or output error happened, or `--strict` saw a file over the file limit.
    Error,
    /// A candidate matched, or a dry run selected a file it would assess.
    Match,
    /// No candidate matched and none was uncertain.
    NoMatch,
    /// No error happened and a candidate was uncertain.
    Uncertain,
}

impl ExitStatus {
    /// The process exit code, which the trace's search span also records.
    pub(crate) const fn to_code(self) -> u8 {
        match self {
            Self::Match => 0,
            Self::NoMatch => 1,
            Self::Error => 2,
            Self::Uncertain => 3,
        }
    }
}

impl From<ExitStatus> for ExitCode {
    #[inline]
    fn from(status: ExitStatus) -> Self {
        Self::from(status.to_code())
    }
}

/// Standard output, which receives only the search's results.
#[expect(
    clippy::disallowed_methods,
    reason = "the output module opens standard output, as cli-output-channels requires"
)]
pub(crate) fn lock_stdout() -> io::StdoutLock<'static> {
    io::stdout().lock()
}

/// Standard error, which receives errors, skip notices, the summary, and a trace written to `-`.
#[expect(
    clippy::disallowed_methods,
    reason = "the output module opens standard error, as cli-output-channels requires"
)]
pub(crate) fn open_stderr() -> io::Stderr {
    io::stderr()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Format {
    Json,
    Text,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Sort {
    None,
    Score,
}

#[derive(Default, Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the field order is the order of the summary's JSON keys"
)]
pub(crate) struct Summary {
    pub selected: usize,
    pub evaluated: usize,
    pub matches: usize,
    pub no_match: usize,
    pub uncertain: usize,
    pub skipped: usize,
    pub errors: usize,
    pub dry_run: bool,
    /// Files skipped for exceeding the file limit, also counted in `skipped`.
    #[serde(skip)]
    pub too_large: usize,
    /// Whether a file skipped for exceeding the file limit fails the run, as `--strict` asks.
    #[serde(skip)]
    pub strict: bool,
}

impl Summary {
    pub(crate) const fn exit_status(&self) -> ExitStatus {
        if self.errors > 0 || (self.strict && self.too_large > 0) {
            ExitStatus::Error
        } else if self.uncertain > 0 {
            ExitStatus::Uncertain
        } else if self.matches > 0 || (self.dry_run && self.selected > self.skipped) {
            ExitStatus::Match
        } else {
            ExitStatus::NoMatch
        }
    }
}

pub(crate) struct Output<Writer> {
    format: Format,
    min_confidence: f64,
    pending: Vec<(Candidate, Assessment)>,
    sort: Sort,
    summary: Summary,
    writer: Writer,
}

/// A record's data: the keys of `base` and the keys `added` to them.
#[derive(Serialize)]
struct Extended<Added, Base> {
    #[serde(flatten)]
    added: Added,
    #[serde(flatten)]
    base: Base,
}

impl<Writer> Output<Writer>
where
    Writer: Write,
{
    pub(crate) fn begin(&mut self, data: Value) -> io::Result<()> {
        self.write_event("begin", data)
    }

    fn count_assessment(&mut self, assessment: &Assessment) {
        self.summary.evaluated = self.summary.evaluated.saturating_add(1);
        match assessment.route(self.min_confidence).outcome {
            Outcome::Match => self.summary.matches = self.summary.matches.saturating_add(1),
            Outcome::NoMatch => self.summary.no_match = self.summary.no_match.saturating_add(1),
            Outcome::Uncertain => self.summary.uncertain = self.summary.uncertain.saturating_add(1),
        }
    }

    pub(crate) fn finish(&mut self) -> io::Result<ExitStatus> {
        self.pending
            .sort_by(|(left, left_assessment), (right, right_assessment)| {
                match_probability(right_assessment)
                    .total_cmp(&match_probability(left_assessment))
                    .then_with(|| read_location(left).cmp(&read_location(right)))
            });
        for (candidate, assessment) in mem::take(&mut self.pending) {
            self.write_result(&candidate, &assessment)?;
        }
        let status = self.summary.exit_status();
        if self.format == Format::Json {
            self.write_event("summary", serde_json::to_value(&self.summary)?)?;
        } else {
            writeln!(
                open_stderr().lock(),
                "{} selected · {} evaluated · {} match · {} no match · {} uncertain · {} skipped \
                 · {} errors{}",
                self.summary.selected,
                self.summary.evaluated,
                self.summary.matches,
                self.summary.no_match,
                self.summary.uncertain,
                self.summary.skipped,
                self.summary.errors,
                if self.summary.dry_run {
                    " (dry run)"
                } else {
                    ""
                }
            )?;
        }
        Ok(status)
    }

    pub(crate) fn new(
        writer: Writer,
        format: Format,
        dry_run: bool,
        strict: bool,
        sort: Sort,
        min_confidence: f64,
    ) -> Self {
        Self {
            format,
            min_confidence,
            pending: Vec::new(),
            sort,
            summary: Summary {
                dry_run,
                strict,
                ..Summary::default()
            },
            writer,
        }
    }

    /// Report the outcome of one selected input: a result, a skipped file, or an input error.
    pub(crate) fn scan_event(&mut self, event: ScanEvent) -> io::Result<()> {
        self.summary.selected = self.summary.selected.saturating_add(1);
        match event {
            ScanEvent::Result {
                candidate,
                assessment,
            } => {
                self.count_assessment(&assessment);
                let routing = assessment.route(self.min_confidence);
                match self.sort {
                    Sort::None => self.write_result(&candidate, &assessment)?,
                    Sort::Score
                        if self.format == Format::Json || routing.outcome != Outcome::NoMatch =>
                    {
                        self.pending.push((candidate, *assessment));
                    },
                    // Text output never prints a nonmatch, so a score sort drops it.
                    Sort::Score => {},
                }
            },
            ScanEvent::Skipped {
                path,
                lines,
                reason,
            } => {
                self.summary.skipped = self.summary.skipped.saturating_add(1);
                if let SkipReason::TooLarge { .. } = reason {
                    self.summary.too_large = self.summary.too_large.saturating_add(1);
                }
                if let SkipReason::TooLarge { bytes } = reason
                    && self.format == Format::Text
                {
                    writeln!(
                        open_stderr().lock(),
                        "ev-grep: {}: skipped; file is {bytes} bytes; limit is {MAX_FILE_BYTES} \
                         bytes",
                        label(&path, lines)
                    )?;
                }
                let data = Extended {
                    added: json!({ "file": path }),
                    base: reason,
                };
                self.write_event("skipped", attach_lines(data, lines))?;
            },
            ScanEvent::Error(error) => {
                self.write_error(Some(&error.path), error.lines, error.kind, &error.message)?;
            },
        }
        Ok(())
    }

    pub(crate) fn select_state(&mut self, input: &StateInput) -> io::Result<()> {
        self.summary.selected = self.summary.selected.saturating_add(1);
        if !self.summary.dry_run {
            return Ok(());
        }
        match self.format {
            Format::Json => self.write_event("selected_state", input)?,
            Format::Text => writeln!(self.writer, "<state>\t{} bytes", input.bytes)?,
        }
        Ok(())
    }

    pub(crate) fn write_error(
        &mut self,
        path: Option<&str>,
        lines: Option<LineRange>,
        kind: ErrorKind,
        message: &str,
    ) -> io::Result<()> {
        self.summary.errors = self.summary.errors.saturating_add(1);
        if self.format == Format::Json {
            self.write_event(
                "error",
                attach_lines(
                    json!({ "file": path, "kind": kind, "message": message }),
                    lines,
                ),
            )?;
        } else {
            writeln!(
                open_stderr().lock(),
                "ev-grep: {}{message}",
                path.map(|file| format!("{}: ", label(file, lines)))
                    .unwrap_or_default()
            )?;
        }
        Ok(())
    }

    fn write_event(&mut self, kind: &str, data: impl Serialize) -> io::Result<()> {
        if self.format == Format::Json {
            serde_json::to_writer(
                &mut self.writer,
                &json!({"schema_version": SCHEMA_VERSION, "type": kind, "data": data}),
            )?;
            writeln!(self.writer)?;
            self.writer.flush()?;
        }
        Ok(())
    }

    #[expect(
        clippy::float_arithmetic,
        reason = "the text output prints the match probability as a percentage"
    )]
    fn write_result(&mut self, candidate: &Candidate, assessment: &Assessment) -> io::Result<()> {
        let routing = assessment.route(self.min_confidence);
        match self.format {
            Format::Json => {
                self.write_event(
                    "result",
                    Extended {
                        added: json!({
                            "assessment": serialize_raw_assessment(assessment),
                            "routing": routing,
                        }),
                        base: candidate,
                    },
                )?;
            },
            Format::Text if routing.outcome != Outcome::NoMatch => {
                if routing.outcome == Outcome::Match {
                    writeln!(
                        self.writer,
                        "{}\t{:.0}% match",
                        candidate.label(),
                        match_probability(assessment) * 100.0_f64
                    )?;
                } else {
                    let explanation = describe_uncertainty(assessment, self.min_confidence);
                    writeln!(
                        self.writer,
                        "{}\tuncertain ({explanation})",
                        candidate.label()
                    )?;
                }
                self.writer.flush()?;
            },
            Format::Text => {},
        }
        Ok(())
    }

    /// Report a candidate selected by a dry run.
    pub(crate) fn write_selected(&mut self, candidate: &Candidate) -> io::Result<()> {
        self.summary.selected = self.summary.selected.saturating_add(1);
        let bytes = candidate.count_bytes();
        match self.format {
            Format::Json => {
                self.write_event(
                    "selected",
                    Extended {
                        added: json!({ "bytes": bytes }),
                        base: candidate,
                    },
                )?;
            },
            Format::Text => writeln!(self.writer, "{}\t{bytes} bytes", candidate.label())?,
        }
        Ok(())
    }

    pub(crate) fn write_state_result(
        &mut self,
        input: &StateInput,
        assessment: &Assessment,
    ) -> io::Result<()> {
        self.count_assessment(assessment);
        let routing = assessment.route(self.min_confidence);
        match self.format {
            Format::Json => {
                self.write_event(
                    "assessment",
                    Extended {
                        added: json!({
                            "assessment": serialize_raw_assessment(assessment),
                            "routing": routing,
                        }),
                        base: input,
                    },
                )?;
            },
            Format::Text => {
                let answer = match routing.outcome {
                    Outcome::Match => format!("match (confidence {})", assessment.confidence),
                    Outcome::NoMatch => format!("no_match (confidence {})", assessment.confidence),
                    Outcome::Uncertain => {
                        format!(
                            "uncertain ({})",
                            describe_uncertainty(assessment, self.min_confidence)
                        )
                    },
                };
                writeln!(self.writer, "<state>\t{answer}")?;
                self.writer.flush()?;
            },
        }
        Ok(())
    }
}

#[expect(
    clippy::unreachable,
    reason = "`Assessment::route` gives a reason exactly when the outcome is uncertain, and only \
              then is this called"
)]
fn describe_uncertainty(assessment: &Assessment, minimum: f64) -> String {
    match assessment.route(minimum).reason {
        Some(Uncertainty::ModelUncertain) => "model chose uncertain".to_owned(),
        Some(Uncertainty::LowConfidence) => {
            format!(
                "model chose {}; confidence {} < {minimum}",
                match assessment.choice {
                    Outcome::Match => "match",
                    Outcome::NoMatch => "no_match",
                    Outcome::Uncertain => "uncertain",
                },
                assessment.confidence,
            )
        },
        None => unreachable!("uncertain routing requires a reason"),
    }
}

fn match_probability(assessment: &Assessment) -> f64 {
    assessment
        .probabilities
        .get(&Outcome::Match)
        .copied()
        .unwrap_or(0.0_f64)
}

fn read_location(candidate: &Candidate) -> (Option<&str>, usize, usize, usize, usize) {
    (
        candidate.file.as_deref(),
        candidate.range.start.line,
        candidate.range.start.column,
        candidate.range.end.line,
        candidate.range.end.column,
    )
}

pub(crate) fn serialize_raw_assessment(assessment: &Assessment) -> Value {
    json!({
        "choice": assessment.choice,
        "confidence": assessment.confidence,
        "probabilities": assessment.probabilities,
        "model": assessment.model,
        "request": assessment.request,
        "input_tokens": assessment.request.as_ref().map_or(0, |request| request.input_tokens),
        "output_tokens": assessment.request.as_ref().map_or(0, |request| request.output_tokens),
    })
}

/// Errors and skips retain the requested lines when no validated candidate region is available.
fn attach_lines<Data>(data: Data, lines: Option<LineRange>) -> Extended<Value, Data> {
    Extended {
        added: lines.map_or_else(
            || json!({}),
            |range| json!({ "start_line": range.start, "end_line": range.end }),
        ),
        base: data,
    }
}

#[cfg(test)]
mod tests;
