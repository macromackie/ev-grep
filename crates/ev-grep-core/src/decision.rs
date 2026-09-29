use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::Assessment;

pub struct Question<'a> {
    pub id: &'a str,
    pub query: &'a str,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct RequestInfo {
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
    pub fn into_single(mut self) -> Result<Assessment> {
        anyhow::ensure!(self.answers.len() == 1, "expected one assessment");
        let (_, mut answer) = self.answers.pop_first().context("missing assessment")?;
        answer.request = Some(self.request);
        Ok(answer)
    }
}
