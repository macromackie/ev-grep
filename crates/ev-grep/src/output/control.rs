use core::mem;
use std::io::{self, Write};

use ev_grep_core::{Assessment, Outcome};
use serde::Serialize;
use serde_json::{Value, json};

use super::{
    ExitStatus,
    Format,
    Output,
    SCHEMA_VERSION,
    Sort,
    Summary,
    encoding::{match_probability, read_location},
    open_stderr,
};

impl<Writer> Output<Writer>
where
    Writer: Write,
{
    pub(crate) fn begin(&mut self, data: Value) -> io::Result<()> {
        self.write_event("begin", data)
    }

    pub(super) fn count_assessment(&mut self, assessment: &Assessment) {
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

    pub(super) fn write_event(&mut self, kind: &str, data: impl Serialize) -> io::Result<()> {
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
}
