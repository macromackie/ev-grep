use alloc::collections::BTreeMap;

use ev_grep_core::{Assessment, DecisionBatch, Outcome, RequestInfo};
use serde::Deserialize;

use crate::{
    RequestError,
    error::{ErrorBody, is_identifier},
};

const BUDGET_SOURCES: &[&str] = &[
    "openrouter_credits",
    "openrouter_guardrail_budget",
    "openrouter_in_flight_budget",
    "openrouter_key_limit",
    "openrouter_workspace_budget",
];
const BUDGET_TIMESTAMP_PUNCTUATION: &[u8] = b"-T:.Z";

#[derive(Deserialize)]
struct Response {
    answers: BTreeMap<String, Choice>,
    #[serde(default)]
    id: Option<String>,
    model: String,
    usage: Usage,
}

#[derive(Deserialize)]
struct Choice {
    choice: Outcome,
    confidence: f64,
    #[serde(rename = "type")]
    kind: String,
    probabilities: BTreeMap<Outcome, f64>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    cost: Option<f64>,
    input_tokens: u64,
    output_tokens: u64,
}

pub(crate) fn parse_assessment(
    bytes: &[u8],
    expected_model: &str,
    ids: &[&str],
) -> Result<DecisionBatch, RequestError> {
    let Ok(response) = serde_json::from_slice::<Response>(bytes) else {
        return Err(RequestError::new_invalid("invalid Jev response"));
    };
    // Another attempt would reach the same model, so a mismatch is not an invalid answer to retry.
    if response.model != expected_model {
        return Err(RequestError::new_model_mismatch(
            expected_model,
            &response.model,
        ));
    }
    if response.answers.len() != ids.len()
        || !ids.iter().all(|id| response.answers.contains_key(*id))
    {
        return Err(RequestError::new_invalid(
            "Jev returned an unexpected set of answers",
        ));
    }
    if response
        .usage
        .cost
        .is_some_and(|cost| !cost.is_finite() || cost < 0.0_f64)
    {
        return Err(RequestError::new_invalid("invalid request cost"));
    }
    let request = RequestInfo {
        input_tokens: response.usage.input_tokens,
        output_tokens: response.usage.output_tokens,
        cost_usd: response.usage.cost,
        provider_id: response.id.filter(|id| is_identifier(id)),
        ..RequestInfo::default()
    };
    let answers = response
        .answers
        .into_iter()
        .map(|(id, answer)| Ok((id, validate(answer, &response.model)?)))
        .collect::<Result<_, RequestError>>()?;
    Ok(DecisionBatch { answers, request })
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: ErrorDetail,
}

#[derive(Deserialize)]
struct ErrorDetail {
    code: Option<serde_json::Value>,
    metadata: Option<ErrorMetadata>,
}

#[derive(Deserialize)]
struct ErrorMetadata {
    limit_source: Option<String>,
    reasons: Option<Vec<String>>,
    resets_at: Option<String>,
}

/// The safe fields of a provider error body. A body that is not a JSON error object, such as a
/// gateway's HTML page, has none.
pub(crate) fn parse_error_body(bytes: &[u8]) -> ErrorBody {
    let Ok(ErrorResponse { error }) = serde_json::from_slice(bytes) else {
        return ErrorBody::default();
    };
    let code = match error.code {
        Some(serde_json::Value::Number(number)) => Some(number.to_string().into_boxed_str()),
        Some(serde_json::Value::String(text)) => Some(text.into_boxed_str()),
        _ => None,
    };
    let limit_source = error
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.limit_source.as_deref())
        .filter(|source| BUDGET_SOURCES.contains(source))
        .map(Box::from);
    let resets_at = error
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.resets_at.as_deref())
        .filter(|value| is_budget_reset(value))
        .map(Box::from);
    let moderation = error
        .metadata
        .and_then(|metadata| metadata.reasons)
        .filter(|reasons| !reasons.is_empty())
        .map(|reasons| {
            reasons
                .into_iter()
                .filter(|reason| is_identifier(reason))
                .collect()
        });
    ErrorBody {
        code: code.filter(|text| is_identifier(text)),
        limit_source,
        moderation,
        resets_at,
    }
}

#[expect(
    clippy::float_arithmetic,
    reason = "probabilities are compared within a rounding tolerance"
)]
fn validate(answer: Choice, model: &str) -> Result<Assessment, RequestError> {
    if answer.kind != "choice" {
        return Err(RequestError::new_invalid(
            "Jev returned a non-choice assessment",
        ));
    }
    if !(answer.confidence.is_finite() && (0.0_f64..=1.0_f64).contains(&answer.confidence)) {
        return Err(RequestError::new_invalid("invalid confidence"));
    }
    let labels = [Outcome::Match, Outcome::NoMatch, Outcome::Uncertain];
    if answer.probabilities.len() != labels.len()
        || !labels
            .iter()
            .all(|label| answer.probabilities.contains_key(label))
    {
        return Err(RequestError::new_invalid("invalid probability labels"));
    }
    if !answer
        .probabilities
        .values()
        .all(|probability| probability.is_finite() && (0.0_f64..=1.0_f64).contains(probability))
    {
        return Err(RequestError::new_invalid("invalid probability values"));
    }
    if (answer.probabilities.values().sum::<f64>() - 1.0_f64).abs() >= 0.01_f64 {
        return Err(RequestError::new_invalid("probabilities do not sum to one"));
    }
    let Some(&chosen) = answer.probabilities.get(&answer.choice) else {
        return Err(RequestError::new_invalid("invalid probability labels"));
    };
    if !answer
        .probabilities
        .values()
        .all(|probability| *probability <= chosen + 1.0e-6_f64)
    {
        return Err(RequestError::new_invalid(
            "choice is not the highest probability",
        ));
    }
    Ok(Assessment {
        choice: answer.choice,
        confidence: answer.confidence,
        probabilities: answer.probabilities,
        model: model.to_owned(),
        request: None,
    })
}

fn is_budget_reset(value: &str) -> bool {
    (20..=30).contains(&value.len())
        && value.ends_with('Z')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || BUDGET_TIMESTAMP_PUNCTUATION.contains(&byte))
}
