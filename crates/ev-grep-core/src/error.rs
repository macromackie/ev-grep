use alloc::string::FromUtf8Error;
use core::error::Error;
use std::io;

use serde::Serialize;

use crate::{LineRange, MAX_FILE_BYTES};

/// What kind of failure an error reports. JSON error records name it as `kind`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// The run's settings are invalid, such as conflicting arguments, an invalid glob, an
    /// unsupported model, or a missing API key.
    Configuration,
    /// No connection to the service was made.
    Connect,
    /// The service rejected the credential, or its key reached a limit, so no later request can
    /// succeed.
    CredentialRejected,
    /// An input is malformed, such as invalid UTF-8, an empty query, a line range outside its file,
    /// invalid JSON, or a candidate range that does not match its text.
    InvalidInput,
    /// The service's response failed validation.
    InvalidResponse,
    /// Reading an input or writing output failed, such as for a missing file or a closed pipe.
    Io,
    /// The response names a different model than the one requested.
    ModelMismatch,
    /// The service's moderation flagged the request.
    Moderation,
    /// The service limited the request rate.
    RateLimited,
    /// The service rejected the request.
    Rejected,
    /// The response exceeds the size the evaluator accepts.
    ResponseTooLarge,
    /// A request to the evaluator's service timed out.
    Timeout,
    /// The TLS handshake with the service failed.
    Tls,
    /// An input exceeds a byte limit, or a selection exceeds a candidate limit. Nothing was
    /// truncated.
    TooLarge,
    /// The connection failed while a request was sent or its response was read.
    Transport,
    /// The service or its gateway was unavailable.
    Unavailable,
    /// An input is not a regular file, such as a symbolic link, a special file, or a directory
    /// given a line range.
    UnsupportedFile,
}

impl ErrorKind {
    /// Whether no later request with the same settings can succeed, so a scan stops at the first
    /// such error.
    #[inline]
    #[must_use]
    pub fn is_fatal(self) -> bool {
        self == Self::CredentialRejected
    }
}

/// An error that names its kind, so callers and reports can classify it without reading its
/// message.
pub trait Failure: Error {
    /// The kind that classifies this error in reports.
    fn classify(&self) -> ErrorKind;

    /// The error's message followed by each underlying cause, separated by `: `.
    #[inline]
    fn format_message(&self) -> String {
        let mut message = self.to_string();
        let mut cause = self.source();
        while let Some(error) = cause {
            message.push_str(": ");
            message.push_str(&error.to_string());
            cause = error.source();
        }
        message
    }
}

/// Why an input could not be selected, read, or validated, or why an evaluator's answers were
/// incomplete.
#[derive(Debug, thiserror::Error)]
pub enum InputError {
    #[error("expected one assessment, got {count:?}")]
    AnswerCount { count: usize },
    #[error("byte offsets disagree with context")]
    ByteOffsetContext,
    #[error("byte offsets do not match text length")]
    ByteOffsetLength,
    #[error("candidate contains NUL bytes")]
    CandidateNul,
    #[error("candidate source exceeds 65536 bytes; input was not truncated")]
    CandidateTooLarge,
    #[error("context slice does not match candidate text")]
    ContextMismatch,
    #[error("cannot read the current directory")]
    CurrentDirectory(#[source] io::Error),
    #[error("query must not be empty")]
    EmptyText,
    #[error("cannot encode candidate metadata")]
    EncodeMetadata(#[source] serde_json::Error),
    #[error("invalid glob {glob:?}")]
    Glob {
        glob: String,
        #[source]
        source: ignore::Error,
    },
    #[error("cannot build the glob filter")]
    Globs(#[source] ignore::Error),
    #[error("cannot inspect file")]
    Inspect(#[source] io::Error),
    #[error("invalid candidate file")]
    InvalidCandidateFile,
    #[error("invalid focus start")]
    InvalidFocus,
    #[error("invalid line range {range:?}; use N or N-M, counting lines from 1")]
    InvalidLineRange { range: String },
    #[error("invalid source range")]
    InvalidSourceRange,
    #[error("invalid start offset")]
    InvalidStartOffset,
    #[error("invalid supporting file")]
    InvalidSupportingFile,
    #[error("jobs must be between 1 and 256")]
    Jobs,
    #[error(transparent)]
    Json(serde_json::Error),
    #[error("JSON nesting exceeds 128 levels")]
    JsonDepth,
    #[error("line range {:?} is outside the file, which has {total:?} lines", .lines.to_string())]
    LinesOutsideFile { lines: LineRange, total: usize },
    #[error("candidate metadata exceeds 65536 bytes; input was not truncated")]
    MetadataTooLarge,
    #[error("expected a regular file; symbolic links are not followed")]
    NotRegularFile,
    #[error("expected a regular file or directory; symbolic links are not followed")]
    NotRegularFileOrDirectory,
    #[error("cannot open file")]
    Open(#[source] io::Error),
    #[error("file path is not valid UTF-8")]
    PathNotUtf8,
    #[error("position is outside context")]
    PositionOutsideContext,
    #[error("position overflow")]
    PositionOverflow,
    #[error("position is too large")]
    PositionTooLarge,
    #[error("range does not match text extent")]
    RangeExtent,
    #[error("line ranges need a regular file; directories and symbolic links are not supported")]
    RangeNeedsRegularFile,
    #[error("cannot read file")]
    Read(#[source] io::Error),
    #[error("cannot read input")]
    ReadText(#[source] io::Error),
    #[error("cannot resolve file")]
    Resolve(#[source] io::Error),
    #[error("source is not valid UTF-8")]
    SourceNotUtf8(#[source] FromUtf8Error),
    #[error("file is {bytes:?} bytes; limit is {MAX_FILE_BYTES:?} bytes; source was not truncated")]
    SourceTooLarge { bytes: u64 },
    #[error("supporting source contains NUL bytes")]
    SupportingNul,
    #[error("query is not valid UTF-8")]
    TextNotUtf8(#[source] FromUtf8Error),
    #[error("query exceeds {limit:?} bytes; query was not truncated")]
    TextTooLarge { limit: usize },
    #[error(transparent)]
    Walk(ignore::Error),
}

impl Failure for InputError {
    #[inline]
    fn classify(&self) -> ErrorKind {
        match self {
            Self::Glob { .. } | Self::Globs(_) | Self::Jobs => ErrorKind::Configuration,
            Self::CurrentDirectory(_)
            | Self::Walk(_)
            | Self::Inspect(_)
            | Self::Open(_)
            | Self::Read(_)
            | Self::Resolve(_)
            | Self::ReadText(_) => ErrorKind::Io,
            Self::NotRegularFile
            | Self::NotRegularFileOrDirectory
            | Self::RangeNeedsRegularFile => ErrorKind::UnsupportedFile,
            Self::SourceTooLarge { .. }
            | Self::TextTooLarge { .. }
            | Self::CandidateTooLarge
            | Self::MetadataTooLarge => ErrorKind::TooLarge,
            Self::SourceNotUtf8(_)
            | Self::PathNotUtf8
            | Self::InvalidLineRange { .. }
            | Self::LinesOutsideFile { .. }
            | Self::TextNotUtf8(_)
            | Self::EmptyText
            | Self::InvalidCandidateFile
            | Self::CandidateNul
            | Self::EncodeMetadata(_)
            | Self::InvalidSupportingFile
            | Self::SupportingNul
            | Self::InvalidFocus
            | Self::PositionOverflow
            | Self::PositionOutsideContext
            | Self::InvalidStartOffset
            | Self::InvalidSourceRange
            | Self::RangeExtent
            | Self::PositionTooLarge
            | Self::ByteOffsetLength
            | Self::ContextMismatch
            | Self::ByteOffsetContext
            | Self::Json(_)
            | Self::JsonDepth => ErrorKind::InvalidInput,
            Self::AnswerCount { .. } => ErrorKind::InvalidResponse,
        }
    }
}

/// A selected file or input that produced no assessment.
#[derive(Debug)]
pub struct FileError {
    pub kind: ErrorKind,
    pub lines: Option<LineRange>,
    /// The error's message and its causes.
    pub message: String,
    pub path: String,
}

impl FileError {
    /// Record `error` against `path` and its optional line range, with its kind and full message.
    #[inline]
    pub fn new<Cause>(path: &str, lines: Option<LineRange>, error: &Cause) -> Self
    where
        Cause: Failure,
    {
        Self {
            kind: error.classify(),
            lines,
            message: error.format_message(),
            path: path.to_owned(),
        }
    }
}
