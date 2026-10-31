//! File selection and semantic assessments, independent of providers and terminal output.

extern crate alloc;

mod candidate;
mod decision;
mod discovery;
mod error;
mod json;
mod limits;
mod line_range;
mod region;
mod scan;
mod scan_event;
mod source;
mod target;
mod telemetry;

pub use candidate::{Candidate, Input, Loaded, SkipReason, SupportingSource};
pub use decision::{
    Assessment,
    DecisionBatch,
    MIN_CONFIDENCE,
    Outcome,
    Question,
    RequestInfo,
    Routing,
    Uncertainty,
};
pub use discovery::{Discovery, discover};
pub use error::{ErrorKind, Failure, FileError, InputError};
pub use json::parse_json;
pub use limits::{MAX_FILE_BYTES, MAX_QUERY_BYTES};
pub use line_range::LineRange;
pub use region::{Position, Region};
pub use scan::{Evaluator, scan_inputs};
pub use scan_event::ScanEvent;
pub use source::{Focus, Source, SourceRead, read_source, read_target, read_text};
pub use target::{Target, label};
