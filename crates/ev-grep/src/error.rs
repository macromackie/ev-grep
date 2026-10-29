use std::{io, path::PathBuf, string::FromUtf8Error};

use ev_grep_core::{ErrorKind, Failure};
use ev_grep_jev::Provider;

/// Why a run, or one of its candidate records, failed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Core(#[from] ev_grep_core::Error),
    #[error(transparent)]
    Jev(#[from] ev_grep_jev::Error),
    #[error("cannot write output")]
    Output(#[from] io::Error),
    #[error("stdin cannot contain both query and source")]
    StdinTwice,
    #[error("provide a query or --query-file")]
    MissingQuery,
    #[error("stdin, candidate, and state inputs cannot be combined with paths")]
    PathsWithInput,
    #[error("cannot read query file {}", .path.display())]
    QueryFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// It keeps no `VarError`, whose message would print a value that is not valid Unicode.
    #[error("set {variable} for provider {provider}")]
    MissingKey {
        variable: &'static str,
        provider: Provider,
    },
    #[error("cannot read state {}", .path.display())]
    State {
        path: PathBuf,
        #[source]
        source: ev_grep_core::Error,
    },
    #[error("state input is binary")]
    BinaryState,
    #[error("expected one JSON state value")]
    StateJson(#[source] ev_grep_core::Error),
    #[error("cannot encode the state")]
    EncodeState(#[source] serde_json::Error),
    #[error("cannot read stdin")]
    Stdin(#[source] io::Error),
    #[error("stdin exceeds 65536 bytes; input was not truncated")]
    StdinTooLarge,
    #[error("stdin is not valid UTF-8")]
    StdinNotUtf8(#[source] FromUtf8Error),
    #[error("cannot read candidates {}", .path.display())]
    CandidatesFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot read candidates")]
    ReadCandidates(#[source] io::Error),
    #[error("candidate JSON line exceeds 524288 bytes; remaining input was not read")]
    CandidateLineTooLarge,
    #[error("expected candidate JSON with file, range, and text")]
    CandidateFields,
    #[error("cannot read supporting source {argument}")]
    Supporting {
        argument: String,
        #[source]
        source: ev_grep_core::Error,
    },
    #[error("supporting source {argument} is binary")]
    BinarySupporting { argument: String },
    #[error("supporting sources exceed 65536 bytes; input was not truncated")]
    SupportingTooLarge,
    #[error("selection exceeds --max-candidates {limit}; no requests were sent")]
    TooManyCandidates { limit: usize },
    #[error("cannot write trace {}", .path.display())]
    Trace {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

impl Failure for Error {
    fn kind(&self) -> ErrorKind {
        match self {
            Self::Core(error)
            | Self::State { source: error, .. }
            | Self::StateJson(error)
            | Self::Supporting { source: error, .. } => error.kind(),
            Self::Jev(error) => error.kind(),
            Self::StdinTwice
            | Self::MissingQuery
            | Self::PathsWithInput
            | Self::MissingKey { .. } => ErrorKind::Configuration,
            Self::Output(_)
            | Self::Trace { .. }
            | Self::QueryFile { .. }
            | Self::Stdin(_)
            | Self::CandidatesFile { .. }
            | Self::ReadCandidates(_) => ErrorKind::Io,
            Self::BinaryState
            | Self::EncodeState(_)
            | Self::StdinNotUtf8(_)
            | Self::CandidateFields
            | Self::BinarySupporting { .. } => ErrorKind::InvalidInput,
            Self::StdinTooLarge
            | Self::CandidateLineTooLarge
            | Self::SupportingTooLarge
            | Self::TooManyCandidates { .. } => ErrorKind::TooLarge,
        }
    }
}
