use std::io::{self, Write};

use anyhow::Result;
use clap::ValueEnum;
use ev_grep_core::{Assessment, Candidate, LineRange, Outcome, ScanEvent, label};
use serde::Serialize;
use serde_json::{Value, json};

use crate::sarif::Sarif;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Format {
    Text,
    Json,
    Sarif,
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
}

impl Summary {
    pub(crate) fn exit_code(&self) -> u8 {
        if self.errors > 0 {
            2
        } else if self.uncertain > 0 {
            3
        } else if self.matches > 0 || (self.dry_run && self.selected > self.skipped) {
            0
        } else {
            1
        }
    }
}

pub(crate) struct Output<W> {
    writer: W,
    format: Format,
    sarif: Sarif,
    sort: Sort,
    pending: Vec<(Candidate, Assessment)>,
    pub summary: Summary,
}

impl<W: Write> Output<W> {
    pub(crate) fn new(writer: W, format: Format, dry_run: bool, sort: Sort) -> Self {
        Self {
            writer,
            format,
            sarif: Sarif::default(),
            sort,
            pending: Vec::new(),
            summary: Summary {
                dry_run,
                ..Summary::default()
            },
        }
    }

    /// Records the run configuration. JSON Lines streams it; SARIF keeps it, with the query, for its rule.
    pub(crate) fn begin(&mut self, query: &str, data: Value) -> Result<()> {
        if self.format == Format::Sarif {
            self.sarif.begin(query, data);
            return Ok(());
        }
        self.event("begin", data)
    }

    fn event(&mut self, kind: &str, data: impl Serialize) -> Result<()> {
        if self.format == Format::Json {
            serde_json::to_writer(
                &mut self.writer,
                &json!({"schema_version": 2, "type": kind, "data": data}),
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
        message: &str,
    ) -> Result<()> {
        self.summary.errors += 1;
        if self.format == Format::Json {
            self.event(
                "error",
                located(json!({ "file": path, "message": message }), lines),
            )?;
        } else {
            if self.format == Format::Sarif {
                self.sarif.notification("error", path, lines, message);
            }
            writeln!(
                io::stderr().lock(),
                "ev-grep: {}{message}",
                path.map(|p| format!("{}: ", label(p, lines)))
                    .unwrap_or_default()
            )?;
        }
        Ok(())
    }

    pub(crate) fn selected(&mut self, candidate: &Candidate) -> Result<()> {
        let bytes = candidate.bytes();
        match self.format {
            Format::Json => {
                let mut data = serde_json::to_value(candidate)?;
                data["bytes"] = json!(bytes);
                self.event("selected", data)?;
            }
            Format::Sarif => {
                if let Some(file) = &candidate.file {
                    self.sarif.artifact(file, bytes);
                }
            }
            Format::Text => writeln!(self.writer, "{}\t{bytes} bytes", candidate.label())?,
        }
        Ok(())
    }

    pub(crate) fn scan_event(&mut self, event: ScanEvent) -> Result<()> {
        match event {
            ScanEvent::Result {
                candidate,
                assessment,
            } => {
                self.summary.evaluated += 1;
                match assessment.outcome {
                    Outcome::Match => self.summary.matches += 1,
                    Outcome::NoMatch => self.summary.no_match += 1,
                    Outcome::Uncertain => self.summary.uncertain += 1,
                }
                if self.sort == Sort::Score {
                    if self.format == Format::Json || assessment.outcome != Outcome::NoMatch {
                        self.pending.push((candidate, assessment));
                    }
                } else {
                    self.result(&candidate, &assessment)?;
                }
            }
            ScanEvent::Skipped { path, lines } => {
                self.summary.skipped += 1;
                if self.format == Format::Sarif {
                    let message = "skipped binary file";
                    self.sarif.notification("note", Some(&path), lines, message);
                }
                let data = json!({"file": path, "reason": "binary"});
                self.event("skipped", located(data, lines))?;
            }
            ScanEvent::Error(error) => {
                self.error(Some(&error.path), error.lines, &error.message)?
            }
        }
        Ok(())
    }

    fn result(&mut self, candidate: &Candidate, assessment: &Assessment) -> Result<()> {
        match self.format {
            Format::Json => {
                let mut data = serde_json::to_value(candidate)?;
                data["assessment"] = raw_assessment(assessment);
                self.event("result", data)?;
            }
            Format::Sarif => self.sarif.result(candidate, assessment),
            Format::Text if assessment.outcome != Outcome::NoMatch => {
                if assessment.outcome == Outcome::Match {
                    writeln!(
                        self.writer,
                        "{}\t{:.0}% match",
                        candidate.label(),
                        match_probability(assessment) * 100.0
                    )?;
                } else {
                    writeln!(self.writer, "{}\tuncertain", candidate.label())?;
                }
                self.writer.flush()?;
            }
            Format::Text => {}
        }
        Ok(())
    }

    pub(crate) fn finish(&mut self) -> Result<u8> {
        self.pending.sort_by(|(a, a_score), (b, b_score)| {
            match_probability(b_score)
                .total_cmp(&match_probability(a_score))
                .then_with(|| location(a).cmp(&location(b)))
        });
        for (candidate, assessment) in std::mem::take(&mut self.pending) {
            self.result(&candidate, &assessment)?;
        }
        let code = self.summary.exit_code();
        if self.format == Format::Json {
            self.event("summary", serde_json::to_value(&self.summary)?)?;
        } else {
            writeln!(
                io::stderr().lock(),
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
        if self.format == Format::Sarif {
            let cwd = std::env::current_dir().ok();
            let summary = serde_json::to_value(&self.summary)?;
            let document = self.sarif.document(summary, cwd.as_deref());
            serde_json::to_writer_pretty(&mut self.writer, &document)?;
            writeln!(self.writer)?;
            self.writer.flush()?;
        }
        Ok(code)
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
        "input_tokens": assessment.input_tokens,
        "output_tokens": assessment.output_tokens,
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
