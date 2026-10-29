//! File selection and semantic assessments, independent of providers and terminal output.

mod candidate;
mod decision;
mod discovery;
mod error;
mod json;
mod region;
mod scan;
mod source;
mod target;
mod telemetry;

pub use candidate::{Candidate, Input, Loaded, SupportingSource};
pub use decision::{
    Assessment, DecisionBatch, MIN_CONFIDENCE, Outcome, Question, RequestInfo, Routing, Uncertainty,
};
pub use discovery::{Discovery, discover};
pub use error::{Error, ErrorKind, Failure, FileError};
pub use json::parse_json;
pub use region::{Position, Region};
pub use scan::{Evaluator, ScanEvent, SkipReason, scan_inputs};
pub use source::{
    Focus, MAX_FILE_BYTES, MAX_QUERY_BYTES, Source, SourceRead, read_source, read_target, read_text,
};
pub use target::{LineRange, Target, label};
