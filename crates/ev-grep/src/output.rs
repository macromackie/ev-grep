use std::io::{self, Write};

use anyhow::Result;
use ev_grep_core::{LineRange, Outcome, ScanEvent, Source, label};
use serde::Serialize;
use serde_json::{Value, json};

use crate::sarif::Sarif;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Format {
    Text,
    Json,
    Sarif,
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
    pub summary: Summary,
}

impl<W: Write> Output<W> {
    pub(crate) fn new(writer: W, format: Format, dry_run: bool) -> Self {
        Self {
            writer,
            format,
            sarif: Sarif::default(),
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
                &json!({"schema_version": 1, "type": kind, "data": data}),
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
                located(json!({ "path": path, "message": message }), lines),
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

    pub(crate) fn selected(&mut self, source: &Source) -> Result<()> {
        let bytes = source.text.len();
        match self.format {
            Format::Json => {
                let lines = source.focus.as_ref().map(|focus| focus.lines);
                self.event(
                    "selected",
                    located(json!({"path": source.path, "bytes": bytes}), lines),
                )?;
            }
            Format::Sarif => self.sarif.artifact(&source.path, bytes),
            Format::Text => writeln!(self.writer, "{}\t{bytes} bytes", source.label())?,
        }
        Ok(())
    }

    pub(crate) fn scan_event(&mut self, event: ScanEvent) -> Result<()> {
        match event {
            ScanEvent::Result {
                path,
                lines,
                assessment,
            } => {
                self.summary.evaluated += 1;
                let outcome = match assessment.outcome {
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
                match self.format {
                    Format::Json => {
                        let data = json!({"path": path, "assessment": assessment});
                        self.event("result", located(data, lines))?;
                    }
                    Format::Sarif => self.sarif.result(&path, lines, &assessment),
                    Format::Text if assessment.outcome != Outcome::NoMatch => {
                        writeln!(self.writer, "{}\t{outcome}", label(&path, lines))?;
                        self.writer.flush()?;
                    }
                    Format::Text => {}
                }
            }
            ScanEvent::Skipped { path, lines } => {
                self.summary.skipped += 1;
                if self.format == Format::Sarif {
                    let message = "skipped binary file";
                    self.sarif.notification("note", Some(&path), lines, message);
                }
                let data = json!({"path": path, "reason": "binary"});
                self.event("skipped", located(data, lines))?;
            }
            ScanEvent::Error(error) => {
                self.error(Some(&error.path), error.lines, &error.message)?
            }
        }
        Ok(())
    }

    pub(crate) fn finish(&mut self) -> Result<u8> {
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

/// Ranged records add `start_line` and `end_line`, counted from 1; whole-file records are unchanged.
fn located(mut data: Value, lines: Option<LineRange>) -> Value {
    if let Some(lines) = lines {
        data["start_line"] = json!(lines.start);
        data["end_line"] = json!(lines.end);
    }
    data
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ev_grep_core::{Assessment, LineRange, Outcome, ScanEvent, Uncertainty};
    use serde_json::{Value, json};

    use super::{Format, Output};

    fn assessment(outcome: Outcome, reason: Option<Uncertainty>, confidence: f64) -> Assessment {
        Assessment {
            outcome,
            reason,
            choice: if outcome == Outcome::Uncertain {
                Outcome::Match
            } else {
                outcome
            },
            confidence,
            probabilities: BTreeMap::from([(outcome, confidence)]),
            model: "fixture".into(),
            input_tokens: 1,
            output_tokens: 1,
        }
    }

    #[test]
    fn json_retains_uncertainty_and_errors_take_exit_precedence() -> anyhow::Result<()> {
        let mut output = Output::new(Vec::new(), Format::Json, false);
        output.summary.selected = 2;
        output.scan_event(ScanEvent::Result {
            path: "a.py".into(),
            lines: Some(LineRange { start: 3, end: 9 }),
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
        output.error(Some("b.py"), None, "request failed")?;
        assert_eq!(output.finish()?, 2);
        let records: Vec<Value> = std::str::from_utf8(&output.writer)?
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
        assert_eq!(records[0]["data"]["path"], "a.py");
        assert_eq!(records[0]["data"]["start_line"], 3);
        assert_eq!(records[0]["data"]["end_line"], 9);
        assert!(records[1]["data"].get("start_line").is_none());
        assert_eq!(records[0]["data"]["assessment"]["choice"], "no_match");
        assert_eq!(records[0]["data"]["assessment"]["outcome"], "uncertain");
        assert_eq!(records[2]["type"], "summary");
        assert_eq!(records[2]["data"]["errors"], 1);
        Ok(())
    }

    #[test]
    fn sarif_reports_matches_and_uncertain_results_with_locations() -> anyhow::Result<()> {
        let mut output = Output::new(Vec::new(), Format::Sarif, false);
        let query = "Catches a database failure\nand returns an empty result.\n";
        output.begin(query, json!({"provider": "openrouter", "dry_run": false}))?;
        output.summary.selected = 5;
        for (path, lines, assessment) in [
            (
                "./src/users.py",
                Some(LineRange { start: 20, end: 56 }),
                assessment(Outcome::Match, None, 0.93),
            ),
            (
                "src/odd:12 x.py",
                None,
                assessment(Outcome::Uncertain, Some(Uncertainty::LowConfidence), 0.62),
            ),
            (
                "src/cart.py",
                None,
                assessment(Outcome::NoMatch, None, 0.97),
            ),
        ] {
            output.scan_event(ScanEvent::Result {
                path: path.into(),
                lines,
                assessment,
            })?;
        }
        output.scan_event(ScanEvent::Skipped {
            path: "/abs/image.py".into(),
            lines: None,
        })?;
        output.error(Some("src/big.py"), None, "file exceeds 65536 bytes")?;
        assert_eq!(output.finish()?, 2);

        let log: Value = serde_json::from_slice(&output.writer)?;
        assert_eq!(log["version"], "2.1.0");
        let run = &log["runs"][0];
        let rule = &run["tool"]["driver"]["rules"][0];
        // Code scanning tracks alerts by rule ID, so the ID for a query must never change.
        assert_eq!(rule["id"], "query/9884e336a13bc935");
        assert_eq!(
            rule["shortDescription"]["text"],
            "Catches a database failure"
        );
        let results = run["results"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        assert_eq!(results.len(), 2, "nonmatches are left out");
        assert!(results.iter().all(|result| result["ruleId"] == rule["id"]));
        let location = |index: usize| &results[index]["locations"][0]["physicalLocation"];
        assert_eq!(results[0]["level"], "warning");
        assert_eq!(
            location(0)["artifactLocation"],
            json!({"uri": "src/users.py", "uriBaseId": "%SRCROOT%"})
        );
        assert_eq!(
            location(0)["region"],
            json!({"startLine": 20, "endLine": 56})
        );
        assert_eq!(results[1]["level"], "note");
        assert_eq!(
            location(1)["artifactLocation"]["uri"],
            "src/odd%3A12%20x.py"
        );
        assert_eq!(location(1)["region"], json!({"startLine": 1}));
        assert_eq!(
            results[1]["message"]["text"],
            "Catches a database failure (ev-grep uncertain: low confidence 0.62)"
        );
        assert_eq!(results[1]["properties"]["assessment"]["choice"], "match");

        let invocation = &run["invocations"][0];
        assert_eq!(invocation["executionSuccessful"], false);
        let notifications = invocation["toolExecutionNotifications"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        assert_eq!(notifications.len(), 2);
        assert_eq!(notifications[0]["level"], "note");
        assert_eq!(
            notifications[0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "file:///abs/image.py"
        );
        assert_eq!(notifications[1]["level"], "error");
        assert_eq!(run["properties"]["evGrep"]["summary"]["errors"], 1);
        Ok(())
    }
}
