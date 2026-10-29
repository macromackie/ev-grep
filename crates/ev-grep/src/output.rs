use std::{
    io::{self, Write},
    process::ExitCode,
};

use crate::inputs::StateInput;
use clap::ValueEnum;
use ev_grep_core::{
    Assessment, Candidate, ErrorKind, LineRange, MAX_FILE_BYTES, Outcome, ScanEvent, SkipReason,
    Uncertainty, label,
};
use serde::Serialize;
use serde_json::{Value, json};

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
    pub(crate) const fn code(self) -> u8 {
        match self {
            Self::Match => 0,
            Self::NoMatch => 1,
            Self::Error => 2,
            Self::Uncertain => 3,
        }
    }
}

impl From<ExitStatus> for ExitCode {
    fn from(status: ExitStatus) -> Self {
        Self::from(status.code())
    }
}

/// Standard output, which receives only the search's results.
#[expect(
    clippy::disallowed_methods,
    reason = "the output module opens standard output, as cli-output-channels requires"
)]
pub(crate) fn stdout() -> io::StdoutLock<'static> {
    io::stdout().lock()
}

/// Standard error, which receives errors, skip notices, the summary, and a trace written to `-`.
#[expect(
    clippy::disallowed_methods,
    reason = "the output module opens standard error, as cli-output-channels requires"
)]
pub(crate) fn stderr() -> io::Stderr {
    io::stderr()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Format {
    Text,
    Json,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Sort {
    None,
    Score,
}

#[derive(Default, Serialize)]
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
    pub(crate) fn exit_status(&self) -> ExitStatus {
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

pub(crate) struct Output<W> {
    writer: W,
    format: Format,
    sort: Sort,
    min_confidence: f64,
    pending: Vec<(Candidate, Assessment)>,
    summary: Summary,
}

impl<W: Write> Output<W> {
    pub(crate) fn new(
        writer: W,
        format: Format,
        dry_run: bool,
        strict: bool,
        sort: Sort,
        min_confidence: f64,
    ) -> Self {
        Self {
            writer,
            format,
            sort,
            min_confidence,
            pending: Vec::new(),
            summary: Summary {
                dry_run,
                strict,
                ..Summary::default()
            },
        }
    }

    pub(crate) fn begin(&mut self, data: Value) -> io::Result<()> {
        self.event("begin", data)
    }

    fn event(&mut self, kind: &str, data: impl Serialize) -> io::Result<()> {
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

    pub(crate) fn error(
        &mut self,
        path: Option<&str>,
        lines: Option<LineRange>,
        kind: ErrorKind,
        message: &str,
    ) -> io::Result<()> {
        self.summary.errors += 1;
        if self.format == Format::Json {
            self.event(
                "error",
                located(
                    json!({ "file": path, "kind": kind, "message": message }),
                    lines,
                ),
            )?;
        } else {
            writeln!(
                stderr().lock(),
                "ev-grep: {}{message}",
                path.map(|p| format!("{}: ", label(p, lines)))
                    .unwrap_or_default()
            )?;
        }
        Ok(())
    }

    /// Report a candidate selected by a dry run.
    pub(crate) fn selected(&mut self, candidate: &Candidate) -> io::Result<()> {
        self.summary.selected += 1;
        let bytes = candidate.bytes();
        match self.format {
            Format::Json => {
                let mut data = serde_json::to_value(candidate)?;
                data["bytes"] = json!(bytes);
                self.event("selected", data)?;
            }
            Format::Text => writeln!(self.writer, "{}\t{bytes} bytes", candidate.label())?,
        }
        Ok(())
    }

    pub(crate) fn select_state(&mut self, input: &StateInput) -> io::Result<()> {
        self.summary.selected += 1;
        if !self.summary.dry_run {
            return Ok(());
        }
        match self.format {
            Format::Json => self.event("selected_state", input)?,
            Format::Text => writeln!(self.writer, "<state>\t{} bytes", input.bytes)?,
        }
        Ok(())
    }

    fn count_assessment(&mut self, assessment: &Assessment) {
        self.summary.evaluated += 1;
        match assessment.route(self.min_confidence).outcome {
            Outcome::Match => self.summary.matches += 1,
            Outcome::NoMatch => self.summary.no_match += 1,
            Outcome::Uncertain => self.summary.uncertain += 1,
        }
    }

    pub(crate) fn state_result(
        &mut self,
        input: &StateInput,
        assessment: &Assessment,
    ) -> io::Result<()> {
        self.count_assessment(assessment);
        let routing = assessment.route(self.min_confidence);
        match self.format {
            Format::Json => {
                let mut data = serde_json::to_value(input)?;
                data["assessment"] = raw_assessment(assessment);
                data["routing"] = serde_json::to_value(routing)?;
                self.event("assessment", data)?;
            }
            Format::Text => {
                let answer = match routing.outcome {
                    Outcome::Match => format!("match (confidence {})", assessment.confidence),
                    Outcome::NoMatch => format!("no_match (confidence {})", assessment.confidence),
                    Outcome::Uncertain => format!(
                        "uncertain ({})",
                        uncertainty(assessment, self.min_confidence)
                    ),
                };
                writeln!(self.writer, "<state>\t{answer}")?;
                self.writer.flush()?;
            }
        }
        Ok(())
    }

    /// Report the outcome of one selected input: a result, a skipped file, or an input error.
    pub(crate) fn scan_event(&mut self, event: ScanEvent) -> io::Result<()> {
        self.summary.selected += 1;
        match event {
            ScanEvent::Result {
                candidate,
                assessment,
            } => {
                self.count_assessment(&assessment);
                let routing = assessment.route(self.min_confidence);
                if self.sort == Sort::Score {
                    if self.format == Format::Json || routing.outcome != Outcome::NoMatch {
                        self.pending.push((candidate, *assessment));
                    }
                } else {
                    self.result(&candidate, &assessment)?;
                }
            }
            ScanEvent::Skipped {
                path,
                lines,
                reason,
            } => {
                self.summary.skipped += 1;
                if let SkipReason::TooLarge { bytes } = reason {
                    self.summary.too_large += 1;
                    if self.format == Format::Text {
                        writeln!(
                            stderr().lock(),
                            "ev-grep: {}: skipped; file is {bytes} bytes; limit is {MAX_FILE_BYTES} bytes",
                            label(&path, lines)
                        )?;
                    }
                }
                let mut data = serde_json::to_value(reason)?;
                data["file"] = json!(path);
                self.event("skipped", located(data, lines))?;
            }
            ScanEvent::Error(error) => {
                self.error(Some(&error.path), error.lines, error.kind, &error.message)?
            }
        }
        Ok(())
    }

    fn result(&mut self, candidate: &Candidate, assessment: &Assessment) -> io::Result<()> {
        let routing = assessment.route(self.min_confidence);
        match self.format {
            Format::Json => {
                let mut data = serde_json::to_value(candidate)?;
                data["assessment"] = raw_assessment(assessment);
                data["routing"] = serde_json::to_value(routing)?;
                self.event("result", data)?;
            }
            Format::Text if routing.outcome != Outcome::NoMatch => {
                if routing.outcome == Outcome::Match {
                    writeln!(
                        self.writer,
                        "{}\t{:.0}% match",
                        candidate.label(),
                        match_probability(assessment) * 100.0
                    )?;
                } else {
                    let explanation = uncertainty(assessment, self.min_confidence);
                    writeln!(
                        self.writer,
                        "{}\tuncertain ({explanation})",
                        candidate.label()
                    )?;
                }
                self.writer.flush()?;
            }
            Format::Text => {}
        }
        Ok(())
    }

    pub(crate) fn finish(&mut self) -> io::Result<ExitStatus> {
        self.pending.sort_by(|(a, a_score), (b, b_score)| {
            match_probability(b_score)
                .total_cmp(&match_probability(a_score))
                .then_with(|| location(a).cmp(&location(b)))
        });
        for (candidate, assessment) in std::mem::take(&mut self.pending) {
            self.result(&candidate, &assessment)?;
        }
        let status = self.summary.exit_status();
        if self.format == Format::Json {
            self.event("summary", serde_json::to_value(&self.summary)?)?;
        } else {
            writeln!(
                stderr().lock(),
                "{} selected · {} evaluated · {} match · {} no match · {} uncertain · {} skipped · {} errors{}",
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
}

fn uncertainty(assessment: &Assessment, minimum: f64) -> String {
    match assessment.route(minimum).reason {
        Some(Uncertainty::ModelUncertain) => "model chose uncertain".to_owned(),
        Some(Uncertainty::LowConfidence) => format!(
            "model chose {}; confidence {} < {}",
            match assessment.choice {
                Outcome::Match => "match",
                Outcome::NoMatch => "no_match",
                Outcome::Uncertain => "uncertain",
            },
            assessment.confidence,
            minimum,
        ),
        None => unreachable!("uncertain routing requires a reason"),
    }
}

fn match_probability(assessment: &Assessment) -> f64 {
    assessment
        .probabilities
        .get(&Outcome::Match)
        .copied()
        .unwrap_or(0.0)
}

fn location(candidate: &Candidate) -> (Option<&str>, usize, usize, usize, usize) {
    (
        candidate.file.as_deref(),
        candidate.range.start.line,
        candidate.range.start.column,
        candidate.range.end.line,
        candidate.range.end.column,
    )
}

pub(crate) fn raw_assessment(assessment: &Assessment) -> Value {
    json!({
        "choice": assessment.choice,
        "confidence": assessment.confidence,
        "probabilities": assessment.probabilities,
        "model": assessment.model,
        "request": assessment.request,
        "input_tokens": assessment.request.as_ref().map_or(0, |r| r.input_tokens),
        "output_tokens": assessment.request.as_ref().map_or(0, |r| r.output_tokens),
    })
}

/// Errors and skips retain the requested lines when no validated candidate region is available.
fn located(mut data: Value, lines: Option<LineRange>) -> Value {
    if let Some(lines) = lines {
        data["start_line"] = json!(lines.start);
        data["end_line"] = json!(lines.end);
    }
    data
}

#[cfg(test)]
mod tests;
