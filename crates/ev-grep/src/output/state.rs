use std::io::{self, Write};

use ev_grep_core::{Assessment, Outcome};
use serde_json::json;

use super::{Extended, Format, Output, encoding::describe_uncertainty, serialize_raw_assessment};
use crate::state::StateInput;

impl<Writer> Output<Writer>
where
    Writer: Write,
{
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
