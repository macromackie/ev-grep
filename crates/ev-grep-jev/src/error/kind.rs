use core::fmt;

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
    pub(super) const fn from_status_code(status: u16, is_moderated: bool) -> Self {
        match status {
            408 => Self::Timeout,
            429 => Self::RateLimited,
            500 | 502 | 503 | 504 | 520 | 522 | 524 => Self::Unavailable,
            403 if is_moderated => Self::Moderation,
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
