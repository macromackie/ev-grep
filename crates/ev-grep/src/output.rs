use std::io::{self, Write};

use anyhow::Result;
use ev_grep_core::{Outcome, ScanEvent};
use serde::Serialize;
use serde_json::json;

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
    json: bool,
    pub summary: Summary,
}

impl<W: Write> Output<W> {
    pub(crate) fn new(writer: W, json: bool, dry_run: bool) -> Self {
        Self {
            writer,
            json,
            summary: Summary {
                dry_run,
                ..Summary::default()
            },
        }
    }

    pub(crate) fn event(&mut self, kind: &str, data: impl Serialize) -> Result<()> {
        if self.json {
            serde_json::to_writer(
                &mut self.writer,
                &json!({"schema_version": 1, "type": kind, "data": data}),
            )?;
            writeln!(self.writer)?;
            self.writer.flush()?;
        }
        Ok(())
    }

    pub(crate) fn error(&mut self, path: Option<&str>, message: &str) -> Result<()> {
        self.summary.errors += 1;
        if self.json {
            self.event("error", json!({ "path": path, "message": message }))?;
        } else {
            writeln!(
                io::stderr().lock(),
                "ev-grep: {}{message}",
                path.map(|p| format!("{p}: ")).unwrap_or_default()
            )?;
        }
        Ok(())
    }

    pub(crate) fn selected(&mut self, path: &str, bytes: usize) -> Result<()> {
        if self.json {
            self.event("selected", json!({"path": path, "bytes": bytes}))?;
        } else {
            writeln!(self.writer, "{path}\t{bytes} bytes")?;
        }
        Ok(())
    }

    pub(crate) fn scan_event(&mut self, event: ScanEvent) -> Result<()> {
        match event {
            ScanEvent::Result { path, assessment } => {
                self.summary.evaluated += 1;
                let label = match assessment.outcome {
                    Outcome::Match => {
                        self.summary.matches += 1;
                        "match"
                    }
                    Outcome::NoMatch => {
                        self.summary.no_match += 1;
                        "no_match"
                    }
                    Outcome::Uncertain => {
                        self.summary.uncertain += 1;
                        "uncertain"
                    }
                };
                if self.json {
                    self.event("result", json!({"path": path, "assessment": assessment}))?;
                } else if assessment.outcome != Outcome::NoMatch {
                    writeln!(self.writer, "{path}\t{label}")?;
                    self.writer.flush()?;
                }
            }
            ScanEvent::Skipped { path } => {
                self.summary.skipped += 1;
                self.event("skipped", json!({"path": path, "reason": "binary"}))?;
            }
            ScanEvent::Error(error) => self.error(Some(&error.path), &error.message)?,
        }
        Ok(())
    }

    pub(crate) fn finish(&mut self) -> Result<u8> {
        let code = self.summary.exit_code();
        if self.json {
            self.event("summary", serde_json::to_value(&self.summary)?)?;
        } else {
            writeln!(
                io::stderr().lock(),
                "{} files selected · {} evaluated · {} match · {} no match · {} uncertain · {} skipped · {} errors{}",
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
        Ok(code)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ev_grep_core::{Assessment, Outcome, ScanEvent, Uncertainty};
    use serde_json::Value;

    use super::Output;

    #[test]
    fn json_retains_uncertainty_and_errors_take_exit_precedence() -> anyhow::Result<()> {
        let mut output = Output::new(Vec::new(), true, false);
        output.summary.selected = 2;
        output.scan_event(ScanEvent::Result {
            path: "a.py".into(),
            assessment: Assessment {
                outcome: Outcome::Uncertain,
                reason: Some(Uncertainty::LowConfidence),
                choice: Outcome::NoMatch,
                confidence: 0.5,
                probabilities: BTreeMap::from([
                    (Outcome::NoMatch, 0.7),
                    (Outcome::Match, 0.2),
                    (Outcome::Uncertain, 0.1),
                ]),
                model: "fixture".into(),
                input_tokens: 1,
                output_tokens: 1,
            },
        })?;
        assert_eq!(output.summary.exit_code(), 3);
        output.error(Some("b.py"), "request failed")?;
        assert_eq!(output.finish()?, 2);
        let records: Vec<Value> = std::str::from_utf8(&output.writer)?
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
        assert_eq!(records[0]["data"]["assessment"]["choice"], "no_match");
        assert_eq!(records[0]["data"]["assessment"]["outcome"], "uncertain");
        assert_eq!(records[2]["type"], "summary");
        assert_eq!(records[2]["data"]["errors"], 1);
        Ok(())
    }
}
