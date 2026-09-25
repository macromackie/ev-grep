//! File selection and semantic assessments, independent of providers and terminal output.

mod discovery;
mod scan;
mod source;

pub use discovery::{Discovery, discover};
pub use scan::{Evaluator, ScanEvent, scan};
pub use source::{MAX_FILE_BYTES, MAX_QUERY_BYTES, Source, SourceRead, read_source, read_text};

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

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
    InsufficientContext,
    LowConfidence,
}

#[derive(Clone, Debug, Serialize)]
pub struct Assessment {
    pub outcome: Outcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Uncertainty>,
    pub choice: Outcome,
    pub confidence: f64,
    pub probabilities: BTreeMap<Outcome, f64>,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Serialize)]
pub struct FileError {
    pub path: String,
    pub message: String,
}
