use core::{error::Error, fmt, time::Duration};
use std::{io, time::SystemTime};

use ev_grep_core::{ErrorKind, Failure};
use reqwest::{
    Response,
    header::{InvalidHeaderValue, RETRY_AFTER},
};

use crate::{Provider, protocol::ErrorBody};

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

/// Why a Jev request produced no assessment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureKind {
    /// No connection was made, such as a refused connection or a failed name lookup.
    Connect,
    /// The provider answered HTTP 401, 402, or a 403 whose body names no moderation reason: it
    /// rejected the credential, or the key reached a limit. Every request shares the credential, so
    /// the transport starts no request after it.
    CredentialRejected,
    /// The response failed protocol validation.
    InvalidResponse,
    /// The response names a different model than the one requested.
    ModelMismatch,
    /// The provider answered HTTP 403 with a body that names moderation reasons.
    Moderation,
    /// The provider answered HTTP 429, or the rate limit it set for every request outlasts the
    /// request's 15-second budget.
    RateLimited,
    /// The provider answered any other HTTP error, such as 400 or 404.
    Rejected,
    /// The response exceeds 65536 bytes.
    ResponseTooLarge,
    /// An attempt or the request's 15-second budget timed out, or the provider answered HTTP 408.
    Timeout,
    /// The TLS handshake failed, such as a certificate that no trusted root accepts.
    Tls,
    /// The connection failed while the request was sent or the response was read.
    Transport,
    /// The provider or its gateway answered HTTP 500, 502, 503, 504, 520, 522, or 524.
    Unavailable,
}

impl fmt::Display for FailureKind {
    #[inline]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Connect => "connect",
            Self::CredentialRejected => "credential_rejected",
            Self::InvalidResponse => "invalid_response",
            Self::ModelMismatch => "model_mismatch",
            Self::Moderation => "moderation",
            Self::RateLimited => "rate_limited",
            Self::Rejected => "rejected",
            Self::ResponseTooLarge => "response_too_large",
            Self::Timeout => "timeout",
            Self::Tls => "tls",
            Self::Transport => "transport",
            Self::Unavailable => "unavailable",
        })
    }
}

impl FailureKind {
    /// The kind of an HTTP error status, given whether its body names moderation reasons.
    const fn from_status_code(status: u16, moderated: bool) -> Self {
        match status {
            408 => Self::Timeout,
            429 => Self::RateLimited,
            500 | 502 | 503 | 504 | 520 | 522 | 524 => Self::Unavailable,
            403 if moderated => Self::Moderation,
            401..=403 => Self::CredentialRejected,
            _ => Self::Rejected,
        }
    }

    /// The one retry table: whether another attempt of the same request may obtain an answer.
    /// A response that fails validation carries no usable answer, so another attempt may return a
    /// valid one.
    #[inline]
    #[must_use]
    pub const fn is_retryable(self) -> bool {
        match self {
            Self::Timeout
            | Self::Connect
            | Self::Transport
            | Self::RateLimited
            | Self::Unavailable
            | Self::InvalidResponse => true,
            Self::Tls
            | Self::Moderation
            | Self::CredentialRejected
            | Self::Rejected
            | Self::ModelMismatch
            | Self::ResponseTooLarge => false,
        }
    }
}

/// A Jev request that produced no assessment, with only the provider details that are safe to
/// print.
///
/// Identifiers are boxed strings, which keeps every `Result` that carries this error under
/// Clippy's `result_large_err` size.
#[derive(Clone, Debug, thiserror::Error)]
pub struct RequestError {
    /// The gateway's `cf-ray` header.
    pub cf_ray: Option<Box<str>>,
    /// The provider's error code from a 401, 402, or 403 body, when it is a short identifier.
    pub code: Option<Box<str>>,
    pub kind: FailureKind,
    /// The printable summary, which names only the details above.
    pub message: String,
    /// Present when a 401, 402, or 403 body names moderation. It keeps only reason codes that are
    /// short identifiers, never flagged text or messages, so it can be empty.
    pub moderation: Option<Vec<String>>,
    /// The provider's `x-request-id` header.
    pub request_id: Option<Box<str>>,
    /// The provider's `Retry-After` delay, or the rate limit's remaining wait when it outlasts the
    /// request's budget.
    pub retry_after: Option<Duration>,
    /// The HTTP status of the provider's answer.
    pub status: Option<u16>,
}

/// The summary is already a whole diagnostic, quoted where it names a value.
impl fmt::Display for RequestError {
    #[inline]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl RequestError {
    pub(crate) fn from_status_response(
        response: &Response,
        body: ErrorBody,
        provider: Provider,
    ) -> Self {
        let ErrorBody { code, moderation } = body;
        let status = response.status().as_u16();
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .filter(|value| is_identifier(value))
                .map(Box::from)
        };
        let request_id = header("x-request-id");
        let cf_ray = header("cf-ray");
        let kind = FailureKind::from_status_code(status, moderation.is_some());
        let mut message = if kind == FailureKind::CredentialRejected {
            format!(
                "Jev returned HTTP {status}: credential rejected or key limit reached for \
                 provider {provider}; check {}; no further requests were started",
                provider.read_key_variable()
            )
        } else {
            format!("Jev returned HTTP {status}; no assessment was recorded")
        };
        if let Some(reasons) = &moderation {
            message.push_str("; flagged by moderation");
            if !reasons.is_empty() {
                message.extend([": ", reasons.join(", ").as_str()]);
            }
        }
        for (name, detail) in [
            ("code", &code),
            ("x-request-id", &request_id),
            ("cf-ray", &cf_ray),
        ] {
            if let Some(text) = detail {
                message.extend(["; ", name, "=", text]);
            }
        }
        Self {
            cf_ray,
            code,
            kind,
            message,
            moderation,
            request_id,
            retry_after: retry_after(response),
            status: Some(status),
        }
    }

    pub(crate) fn from_transport(failure: reqwest::Error) -> Self {
        let error = failure.without_url();
        // reqwest hands over its causes only as opaque errors, and `io::Error::source` skips the
        // error it wraps, so the walk descends into each wrapped error to name the innermost cause
        // and to find a TLS failure.
        let mut cause: &(dyn Error + 'static) = &error;
        let mut tls = false;
        loop {
            // reqwest exposes its TLS failure only as an opaque cause, so rustls's error type
            // identifies it. ast-grep-ignore: error-downcasts-rust
            tls |= cause.is::<rustls::Error>();
            // reqwest wraps causes in an `io::Error`, whose `source` skips the wrapped error, so
            // its type reaches it. ast-grep-ignore: error-downcasts-rust
            let step = match cause
                .downcast_ref::<io::Error>()
                .and_then(io::Error::get_ref)
            {
                Some(wrapped) => {
                    let erased: &(dyn Error + 'static) = wrapped;
                    Some(erased)
                },
                None => cause.source(),
            };
            match step {
                Some(inner) => cause = inner,
                None => break,
            }
        }
        let (kind, name) = if error.is_timeout() {
            (FailureKind::Timeout, "timeout")
        } else if tls {
            (FailureKind::Tls, "tls")
        } else if error.is_connect() {
            (FailureKind::Connect, "connect")
        } else {
            (FailureKind::Transport, "transport")
        };
        Self::new(kind, format!("Jev transport failed ({name}): {cause}"))
    }

    const fn new(kind: FailureKind, message: String) -> Self {
        Self {
            cf_ray: None,
            code: None,
            kind,
            message,
            moderation: None,
            request_id: None,
            retry_after: None,
            status: None,
        }
    }

    pub(crate) fn new_budget_exhausted() -> Self {
        Self::new(
            FailureKind::Timeout,
            "Jev assessment timed out after its 15-second budget; no assessment was recorded"
                .into(),
        )
    }

    pub(crate) fn new_invalid(message: impl Into<String>) -> Self {
        Self::new(FailureKind::InvalidResponse, message.into())
    }

    pub(crate) fn new_model_mismatch(requested: &str, returned: &str) -> Self {
        let message = if is_identifier(returned) {
            format!(
                "Jev returned model {returned}, not the requested {requested}; no assessment was \
                 recorded"
            )
        } else {
            format!(
                "Jev returned a model other than the requested {requested}; no assessment was \
                 recorded"
            )
        };
        Self::new(FailureKind::ModelMismatch, message)
    }

    /// The rate limit that the provider set for every request lasts `wait` more, past the request's
    /// budget.
    pub(crate) fn new_rate_limit_exceeds_budget(wait: Duration) -> Self {
        Self {
            retry_after: Some(wait),
            ..Self::new(
                FailureKind::RateLimited,
                format!(
                    "Jev is rate limited for {} more seconds, past the 15-second assessment \
                     budget; no assessment was recorded",
                    wait.as_millis().div_ceil(1000)
                ),
            )
        }
    }

    pub(crate) fn new_response_too_large() -> Self {
        Self::new(
            FailureKind::ResponseTooLarge,
            "Jev response exceeds 65536 bytes".into(),
        )
    }
}

/// The provider's `Retry-After` delay, in seconds or as an HTTP date.
fn retry_after(response: &Response) -> Option<Duration> {
    response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|header| header.to_str().ok())
        .and_then(|value| {
            value
                .parse::<u64>()
                .ok()
                .map(Duration::from_secs)
                .or_else(|| {
                    httpdate::parse_http_date(value)
                        .ok()
                        .map(|at| at.duration_since(SystemTime::now()).unwrap_or_default())
                })
        })
}

/// Whether provider text is a short identifier, safe to print, rather than free-form text such as a
/// message.
pub(crate) fn is_identifier(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_./:".contains(&byte))
}
