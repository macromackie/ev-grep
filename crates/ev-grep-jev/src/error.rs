mod kind;
mod request;

use ev_grep_core::{ErrorKind, Failure};
pub use kind::FailureKind;
pub use request::RequestError;
pub(crate) use request::{ErrorBody, is_identifier};
use reqwest::header::InvalidHeaderValue;

use crate::Provider;

/// Why Jev could not be configured or could not assess an input.
#[derive(Debug, thiserror::Error)]
pub enum JevError {
    #[error("cannot build the HTTP client")]
    Client(#[source] reqwest::Error),
    /// The candidate, state, or answer set failed the core's validation.
    #[error(transparent)]
    Core(#[from] ev_grep_core::InputError),
    #[error("API key is empty")]
    EmptyKey,
    #[error("prompt.json fields must not be empty")]
    EmptyPrompt,
    #[error("cannot encode the request")]
    Encode(#[source] serde_json::Error),
    #[error("invalid provider endpoint")]
    InvalidEndpoint(#[source] url::ParseError),
    #[error("invalid API key header")]
    InvalidKey(#[source] InvalidHeaderValue),
    #[error("query must contain 1 to 8192 bytes")]
    InvalidQuery,
    #[error("jobs must be between 1 and 256")]
    Jobs,
    #[error("invalid prompt.json")]
    Prompt(#[source] serde_json::Error),
    #[error("expected 1 to 32 questions")]
    QuestionCount,
    #[error("question IDs must be unique and contain 1 to 64 bytes")]
    QuestionId,
    /// A request was sent and produced no assessment.
    #[error(transparent)]
    Request(#[from] RequestError),
    #[error("encoded request exceeds 98304 bytes; context was not truncated")]
    RequestTooLarge,
    #[error("encoded state exceeds 98304 bytes; context was not truncated")]
    StateTooLarge,
    #[error("provider must be openrouter or typesafe")]
    UnknownProvider,
    #[error("provider endpoint must use HTTP or HTTPS, without credentials, query, or fragment")]
    UnsupportedEndpoint,
    #[error(
        "unsupported model {model:?} for {:?}; supported models: {:#}",
        .provider.to_string(),
        .provider.list_supported_models().join(", ")
    )]
    UnsupportedModel { model: String, provider: Provider },
}

impl Failure for JevError {
    #[inline]
    fn classify(&self) -> ErrorKind {
        match self {
            Self::Request(error) => {
                match error.kind {
                    FailureKind::Timeout => ErrorKind::Timeout,
                    FailureKind::Connect => ErrorKind::Connect,
                    FailureKind::Tls => ErrorKind::Tls,
                    FailureKind::Transport => ErrorKind::Transport,
                    FailureKind::RateLimited => ErrorKind::RateLimited,
                    FailureKind::Unavailable => ErrorKind::Unavailable,
                    FailureKind::Moderation => ErrorKind::Moderation,
                    FailureKind::CredentialRejected => ErrorKind::CredentialRejected,
                    FailureKind::Rejected => ErrorKind::Rejected,
                    FailureKind::InvalidResponse => ErrorKind::InvalidResponse,
                    FailureKind::ModelMismatch => ErrorKind::ModelMismatch,
                    FailureKind::ResponseTooLarge => ErrorKind::ResponseTooLarge,
                }
            },
            Self::Core(error) => error.classify(),
            Self::UnsupportedModel { .. }
            | Self::UnknownProvider
            | Self::InvalidEndpoint(_)
            | Self::UnsupportedEndpoint
            | Self::EmptyKey
            | Self::InvalidKey(_)
            | Self::Client(_)
            | Self::Jobs
            | Self::Prompt(_)
            | Self::EmptyPrompt => ErrorKind::Configuration,
            Self::InvalidQuery | Self::QuestionCount | Self::QuestionId | Self::Encode(_) => {
                ErrorKind::InvalidInput
            },
            Self::RequestTooLarge | Self::StateTooLarge => ErrorKind::TooLarge,
        }
    }
}
