use core::{error::Error, time::Duration};
use std::{io, time::SystemTime};

use reqwest::{Response, header::RETRY_AFTER};

use super::kind::FailureKind;
use crate::Provider;

/// The fields of a provider error body that are safe to print. Flagged input and messages are never
/// kept.
#[derive(Default)]
pub(crate) struct ErrorBody {
    /// The provider's error code, such as `OpenRouter`'s numeric `error.code`, kept only when it
    /// is a short identifier.
    pub code: Option<Box<str>>,
    /// Present only when the body names moderation reasons. Reasons that are not short identifiers
    /// are dropped.
    pub moderation: Option<Vec<String>>,
}

/// A Jev request that produced no assessment, with only the provider details that are safe to
/// print.
///
/// Identifiers are boxed strings, which keeps every `Result` that carries this error under
/// Clippy's `result_large_err` size.
#[derive(Clone, Debug, thiserror::Error)]
#[error(
    // The adapter already constructs a complete diagnostic, including its quoted values.
    // ast-grep-ignore: diagnostic-values-rust
    "{message}"
)]
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
        // `reqwest` hands over its causes only as opaque errors, and `io::Error::source` skips the
        // error it wraps, so the walk descends into each wrapped error to name the innermost cause
        // and to find a TLS failure.
        let mut cause: &(dyn Error + 'static) = &error;
        let mut has_tls = false;
        loop {
            // `reqwest` exposes TLS failures only as opaque causes.
            // ast-grep-ignore: error-downcasts-rust
            let is_tls = cause.is::<rustls::Error>();
            has_tls |= is_tls;
            // `io::Error::source` skips its wrapped error, so its own accessor must reach it.
            // ast-grep-ignore: error-downcasts-rust
            let wrapped_io = cause.downcast_ref::<io::Error>();
            let step = match wrapped_io.and_then(io::Error::get_ref) {
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
        } else if has_tls {
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
