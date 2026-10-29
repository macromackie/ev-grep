use crate::{RequestError, error::is_identifier};
use ev_grep_core::{Assessment, DecisionBatch, Outcome, RequestInfo};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Response {
    model: String,
    answers: BTreeMap<String, Choice>,
    usage: Usage,
    #[serde(default)]
    id: Option<String>,
}

#[derive(Deserialize)]
struct Choice {
    #[serde(rename = "type")]
    kind: String,
    choice: Outcome,
    confidence: f64,
    probabilities: BTreeMap<Outcome, f64>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    cost: Option<f64>,
    input_tokens: u64,
    output_tokens: u64,
}

pub(crate) fn assessment(
    bytes: &[u8],
    expected_model: &str,
    ids: &[&str],
) -> Result<DecisionBatch, RequestError> {
    let response: Response = serde_json::from_slice(bytes)
        .map_err(|_| RequestError::invalid_response("invalid Jev response"))?;
    // Another attempt would reach the same model, so a mismatch is not an invalid answer to retry.
    if response.model != expected_model {
        return Err(RequestError::model_mismatch(
            expected_model,
            &response.model,
        ));
    }
    if response.answers.len() != ids.len()
        || !ids.iter().all(|id| response.answers.contains_key(*id))
    {
        return Err(RequestError::invalid_response(
            "Jev returned an unexpected set of answers",
        ));
    }
    if response
        .usage
        .cost
        .is_some_and(|cost| !cost.is_finite() || cost < 0.0)
    {
        return Err(RequestError::invalid_response("invalid request cost"));
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
    reasons: Option<Vec<String>>,
}

/// The fields of a provider error body that are safe to print. Flagged input and messages are never kept.
#[derive(Default)]
pub(crate) struct ErrorBody {
    /// The provider's error code, such as OpenRouter's numeric `error.code`, kept only when it is a short identifier.
    pub code: Option<Box<str>>,
    /// Present only when the body names moderation reasons. Reasons that are not short identifiers are dropped.
    pub moderation: Option<Vec<String>>,
}

/// The safe fields of a provider error body. A body that is not a JSON error object, such as a gateway's HTML page,
/// has none.
pub(crate) fn error_body(bytes: &[u8]) -> ErrorBody {
    let Ok(ErrorResponse { error }) = serde_json::from_slice(bytes) else {
        return ErrorBody::default();
    };
    let code = match error.code {
        Some(serde_json::Value::Number(number)) => Some(number.to_string().into_boxed_str()),
        Some(serde_json::Value::String(text)) => Some(text.into_boxed_str()),
        _ => None,
    };
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
        code: code.filter(|code| is_identifier(code)),
        moderation,
    }
}

fn validate(answer: Choice, model: &str) -> Result<Assessment, RequestError> {
    if answer.kind != "choice" {
        return Err(RequestError::invalid_response(
            "Jev returned a non-choice assessment",
        ));
    }
    if !(answer.confidence.is_finite() && (0.0..=1.0).contains(&answer.confidence)) {
        return Err(RequestError::invalid_response("invalid confidence"));
    }
    let labels = [Outcome::Match, Outcome::NoMatch, Outcome::Uncertain];
    if answer.probabilities.len() != labels.len()
        || !labels
            .iter()
            .all(|label| answer.probabilities.contains_key(label))
    {
        return Err(RequestError::invalid_response("invalid probability labels"));
    }
    if !answer
        .probabilities
        .values()
        .all(|p| p.is_finite() && (0.0..=1.0).contains(p))
    {
        return Err(RequestError::invalid_response("invalid probability values"));
    }
    if (answer.probabilities.values().sum::<f64>() - 1.0).abs() >= 0.01 {
        return Err(RequestError::invalid_response(
            "probabilities do not sum to one",
        ));
    }
    // The label check above admits only maps that hold every outcome, so the choice has a probability.
    let chosen = answer.probabilities[&answer.choice];
    if !answer
        .probabilities
        .values()
        .all(|p| *p <= chosen + 0.000001)
    {
        return Err(RequestError::invalid_response(
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
