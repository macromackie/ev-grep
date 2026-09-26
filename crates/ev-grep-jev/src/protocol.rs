use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use ev_grep_core::{Assessment, Outcome, Source, Uncertainty};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

// This is a conservative routing policy, not a claim of calibrated accuracy.
pub const MIN_CONFIDENCE: f64 = 0.8;

/// Prompt experiments edit this file and rebuild, so candidates run through the shipped request path.
const PROMPT: &str = include_str!("prompt.json");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Prompt {
    pub(crate) task: String,
    /// Added to the instructions only when a line range narrows the search.
    pub(crate) focus: String,
    pub(crate) criteria: Criteria,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Criteria {
    #[serde(rename = "match")]
    pub(crate) matches: String,
    pub(crate) no_match: String,
    pub(crate) uncertain: String,
}

pub(crate) fn prompt() -> Result<Prompt> {
    parse_prompt(PROMPT)
}

pub(crate) fn parse_prompt(text: &str) -> Result<Prompt> {
    let prompt: Prompt = serde_json::from_str(text).context("invalid prompt.json")?;
    let criteria = &prompt.criteria;
    ensure!(
        [
            &prompt.task,
            &prompt.focus,
            &criteria.matches,
            &criteria.no_match,
            &criteria.uncertain
        ]
        .iter()
        .all(|text| !text.trim().is_empty()),
        "prompt.json fields must not be empty"
    );
    Ok(prompt)
}

/// A ranged search sends the whole file plus the focus lines as text, because models count lines poorly.
pub(crate) fn request(model: &str, prompt: &Prompt, query: &str, source: &Source) -> Value {
    let mut state = json!({ "path": source.path, "source": source.text });
    let mut instructions = json!({ "task": prompt.task, "query": query });
    if let Some(focus) = &source.focus {
        state["focus"] = json!({
            "start_line": focus.lines.start,
            "end_line": focus.lines.end,
            "text": focus.text
        });
        instructions["focus"] = json!(prompt.focus);
    }
    json!({
        "model": model,
        "state": state,
        "questions": {
            "match": {
                "type": "choice",
                "instructions": instructions,
                "criteria": prompt.criteria
            }
        }
    })
}

#[derive(Deserialize)]
struct Response {
    model: String,
    answers: BTreeMap<String, Choice>,
    usage: Usage,
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
    input_tokens: u64,
    output_tokens: u64,
}

pub(crate) fn assessment(bytes: &[u8], expected_model: &str) -> Result<Assessment> {
    // Provider response bodies can contain source text. Do not echo deserialization details.
    let response: Response =
        serde_json::from_slice(bytes).map_err(|_| anyhow::anyhow!("invalid Jev response"))?;
    ensure!(
        response.model == expected_model,
        "Jev returned a different model than requested"
    );
    ensure!(
        response.answers.len() == 1,
        "Jev returned an unexpected set of answers"
    );
    let answer = response
        .answers
        .get("match")
        .context("Jev omitted the assessment")?;
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
    let reason = if answer.choice == Outcome::Uncertain {
        Some(Uncertainty::InsufficientContext)
    } else if answer.confidence < MIN_CONFIDENCE {
        Some(Uncertainty::LowConfidence)
    } else {
        None
    };
    Ok(Assessment {
        outcome: if reason.is_some() {
            Outcome::Uncertain
        } else {
            answer.choice
        },
        reason,
        choice: answer.choice,
        confidence: answer.confidence,
        probabilities: answer.probabilities.clone(),
        model: response.model,
        input_tokens: response.usage.input_tokens,
        output_tokens: response.usage.output_tokens,
    })
}
