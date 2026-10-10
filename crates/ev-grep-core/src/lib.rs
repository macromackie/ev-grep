//! File selection and semantic assessments, independent of providers and terminal output.

mod candidate;
mod decision;
mod discovery;
mod json;
mod region;
mod scan;
mod source;
mod target;

pub use candidate::{Candidate, Input, Loaded, SupportingSource};
pub use decision::{
    Assessment, DecisionBatch, MIN_CONFIDENCE, Outcome, Question, RequestInfo, Routing, Uncertainty,
};
pub use discovery::{Discovery, discover};
pub use json::parse_json;
pub use region::{Position, Region};
pub use scan::{Evaluator, ScanEvent, scan_inputs};
pub use source::{
    Focus, MAX_FILE_BYTES, MAX_QUERY_BYTES, Source, SourceRead, SourceTooLarge, read_source,
    read_target, read_text,
};
pub use target::{LineRange, Target, label};

use std::fmt::Display;

#[derive(Debug)]
pub struct FileError {
    pub path: String,
    pub lines: Option<LineRange>,
    pub message: String,
}

impl FileError {
    pub fn new(path: &str, lines: Option<LineRange>, message: impl Display) -> Self {
        Self {
            path: path.to_owned(),
            lines,
            message: message.to_string(),
        }
    }
}
