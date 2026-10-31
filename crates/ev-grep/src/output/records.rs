use std::io::{self, Write};

use ev_grep_core::{
    Assessment,
    Candidate,
    ErrorKind,
    LineRange,
    MAX_FILE_BYTES,
    Outcome,
    ScanEvent,
    SkipReason,
    label,
};
use serde_json::json;

use super::{
    Extended,
    Format,
    Output,
    Sort,
    encoding::{attach_lines, describe_uncertainty, match_probability},
    open_stderr,
    serialize_raw_assessment,
};

impl<Writer> Output<Writer>
where
    Writer: Write,
{
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

    #[expect(
        clippy::float_arithmetic,
        reason = "the text output prints the match probability as a percentage"
    )]
    pub(super) fn write_result(
        &mut self,
        candidate: &Candidate,
        assessment: &Assessment,
    ) -> io::Result<()> {
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
}
