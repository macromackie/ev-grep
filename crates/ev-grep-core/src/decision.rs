use alloc::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::InputError;

/// One question in a batched request, named by `id`.
#[derive(Debug)]
pub struct Question<'text> {
    pub id: &'text str,
    pub query: &'text str,
}

/// Usage and transport facts about the request that produced an answer.
#[derive(Clone, Debug, Default, Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the field order is the order of the record's JSON keys"
)]
pub struct RequestInfo {
    pub prompt_id: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: Option<f64>,
    pub provider_id: Option<String>,
    pub attempts: usize,
    pub hedges: usize,
    pub elapsed_ms: u128,
}

/// The answers to one batched request, keyed by question id, and the request that produced them.
#[derive(Debug)]
pub struct DecisionBatch {
    pub answers: BTreeMap<String, Assessment>,
    pub request: RequestInfo,
}

impl DecisionBatch {
    /// The only answer in a batch of one question; an error when the batch holds any other count.
    #[inline]
    pub fn into_single(self) -> Result<Assessment, InputError> {
        let count = self.answers.len();
        let mut answers = self.answers.into_values();
        match (answers.next(), answers.next()) {
            (Some(mut answer), None) => {
                answer.request = Some(self.request);
                Ok(answer)
            },
            _ => Err(InputError::AnswerCount { count }),
        }
    }
}

/// A conservative routing default, not a claim of calibrated accuracy.
pub const MIN_CONFIDENCE: f64 = 0.8;

/// The answer a model chose for a question.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Match,
    NoMatch,
    Uncertain,
}

/// Why routing reports an answer as uncertain.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Uncertainty {
    LowConfidence,
    ModelUncertain,
}

/// The outcome a caller acts on after applying a confidence threshold, and why it became uncertain.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Routing {
    pub outcome: Outcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Uncertainty>,
}

/// Validated model evidence, unchanged by a caller's confidence policy.
#[derive(Clone, Debug, Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the field order is the order of the record's JSON keys"
)]
pub struct Assessment {
    pub choice: Outcome,
    pub confidence: f64,
    pub probabilities: BTreeMap<Outcome, f64>,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<RequestInfo>,
}

impl Assessment {
    /// Route the answer: uncertain when the model chose it or its confidence is below `minimum`.
    #[inline]
    #[must_use]
    pub fn route(&self, minimum: f64) -> Routing {
        let reason = if self.choice == Outcome::Uncertain {
            Some(Uncertainty::ModelUncertain)
        } else if self.confidence < minimum {
            Some(Uncertainty::LowConfidence)
        } else {
            None
        };
        Routing {
            outcome: if reason.is_some() {
                Outcome::Uncertain
            } else {
                self.choice
            },
            reason,
        }
    }
}
