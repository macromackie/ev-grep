use crate::{Assessment, Candidate, FileError, LineRange, SkipReason};

/// One per-input outcome that a scan emits.
#[derive(Debug)]
pub enum ScanEvent {
    Error(FileError),
    Result {
        assessment: Box<Assessment>,
        candidate: Candidate,
    },
    Skipped {
        lines: Option<LineRange>,
        path: String,
        reason: SkipReason,
    },
}
