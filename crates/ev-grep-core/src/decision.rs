use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Error;

pub struct Question<'a> {
    pub id: &'a str,
    pub query: &'a str,
}

#[derive(Clone, Debug, Default, Serialize)]
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

#[derive(Debug)]
pub struct DecisionBatch {
    pub answers: BTreeMap<String, Assessment>,
    pub request: RequestInfo,
}

impl DecisionBatch {
    pub fn into_single(self) -> Result<Assessment, Error> {
        let count = self.answers.len();
        let mut answers = self.answers.into_values();
        match (answers.next(), answers.next()) {
            (Some(mut answer), None) => {
                answer.request = Some(self.request);
                Ok(answer)
            }
            _ => Err(Error::AnswerCount { count }),
        }
    }
}

/// A conservative routing default, not a claim of calibrated accuracy.
pub const MIN_CONFIDENCE: f64 = 0.8;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Match,
    NoMatch,
    Uncertain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Uncertainty {
    ModelUncertain,
    LowConfidence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Routing {
    pub outcome: Outcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Uncertainty>,
}

/// Validated model evidence, unchanged by a caller's confidence policy.
#[derive(Clone, Debug, Serialize)]
pub struct Assessment {
    pub choice: Outcome,
    pub confidence: f64,
    pub probabilities: BTreeMap<Outcome, f64>,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<RequestInfo>,
}

impl Assessment {
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
