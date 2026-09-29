use anyhow::{Context, Result, ensure};
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
) -> Result<DecisionBatch> {
    let response: Response =
        serde_json::from_slice(bytes).map_err(|_| anyhow::anyhow!("invalid Jev response"))?;
    ensure!(
        response.model == expected_model,
        "Jev returned a different model than requested"
    );
    ensure!(
        response.answers.len() == ids.len()
            && ids.iter().all(|id| response.answers.contains_key(*id)),
        "Jev returned an unexpected set of answers"
    );
    if let Some(cost) = response.usage.cost {
        ensure!(cost.is_finite() && cost >= 0.0, "invalid request cost");
    }
    let request = RequestInfo {
        input_tokens: response.usage.input_tokens,
        output_tokens: response.usage.output_tokens,
        cost_usd: response.usage.cost,
        provider_id: response.id.filter(|id| {
            id.len() <= 128
                && id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        }),
        ..RequestInfo::default()
    };
    let answers = response
        .answers
        .into_iter()
        .map(|(id, answer)| Ok((id, validate(answer, &response.model)?)))
        .collect::<Result<_>>()?;
    Ok(DecisionBatch { answers, request })
}

fn validate(answer: Choice, model: &str) -> Result<Assessment> {
    ensure!(
        answer.kind == "choice",
        "Jev returned a non-choice assessment"
    );
    ensure!(
        answer.confidence.is_finite() && (0.0..=1.0).contains(&answer.confidence),
        "invalid confidence"
    );
    let labels = [Outcome::Match, Outcome::NoMatch, Outcome::Uncertain];
    ensure!(
        answer.probabilities.len() == labels.len()
            && labels
                .iter()
                .all(|label| answer.probabilities.contains_key(label)),
        "invalid probability labels"
    );
    ensure!(
        answer
            .probabilities
            .values()
            .all(|p| p.is_finite() && (0.0..=1.0).contains(p)),
        "invalid probability values"
    );
    ensure!(
        (answer.probabilities.values().sum::<f64>() - 1.0).abs() < 0.01,
        "probabilities do not sum to one"
    );
    let chosen = answer
        .probabilities
        .get(&answer.choice)
        .context("missing chosen probability")?;
    ensure!(
        answer
            .probabilities
            .values()
            .all(|p| *p <= *chosen + 0.000001),
        "choice is not the highest probability"
    );
    Ok(Assessment {
        outcome: answer.choice,
        reason: None,
        choice: answer.choice,
        confidence: answer.confidence,
        probabilities: answer.probabilities.clone(),
        model: model.to_owned(),
        request: None,
    }
    .with_min_confidence(0.0))
}
