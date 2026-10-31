use ev_grep_core::{Assessment, Candidate, LineRange, Outcome, Uncertainty};
use serde_json::{Value, json};

use super::Extended;

#[expect(
    clippy::unreachable,
    reason = "`Assessment::route` gives a reason exactly when the outcome is uncertain, and only \
              then is this called"
)]
pub(super) fn describe_uncertainty(assessment: &Assessment, minimum: f64) -> String {
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

pub(super) fn match_probability(assessment: &Assessment) -> f64 {
    assessment
        .probabilities
        .get(&Outcome::Match)
        .copied()
        .unwrap_or(0.0_f64)
}

pub(super) fn read_location(candidate: &Candidate) -> (Option<&str>, usize, usize, usize, usize) {
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
pub(super) fn attach_lines<Data>(data: Data, lines: Option<LineRange>) -> Extended<Value, Data> {
    Extended {
        added: lines.map_or_else(
            || json!({}),
            |range| json!({ "start_line": range.start, "end_line": range.end }),
        ),
        base: data,
    }
}
