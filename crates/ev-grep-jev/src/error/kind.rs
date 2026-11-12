use core::fmt;

use crate::Provider;

const PAYMENT_STATUSES: &[u16] = &[402, 403];

/// Why a Jev request produced no assessment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureKind {
    /// The provider has temporarily committed its budget to other requests.
    BudgetBusy,
    /// No connection was made, such as a refused connection or a failed name lookup.
    Connect,
    /// The provider answered HTTP 401 or an ordinary 403 whose body names no moderation reason: it
    /// rejected the credential. Every request shares the credential, so
    /// the transport starts no request after it.
    CredentialRejected,
    /// The provider cannot cover this request with its available credits.
    InsufficientCredits,
    /// The response failed protocol validation.
    InvalidResponse,
    /// The child key has spent its provider-enforced limit.
    KeyBudgetExceeded,
    /// The response names a different model than the one requested.
    ModelMismatch,
    /// The provider answered HTTP 403 with a body that names moderation reasons.
    Moderation,
    /// The provider refused payment without naming the limiting budget.
    PaymentRequired,
    /// The provider answered HTTP 429, or the rate limit it set for every request outlasts the
    /// request's 15-second budget.
    RateLimited,
    /// The provider answered any other HTTP error, such as 400 or 404.
    Rejected,
    /// The response exceeds 65536 bytes.
    ResponseTooLarge,
    /// A provider workspace or guardrail spend budget blocks the key.
    SpendingBudgetExceeded,
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
            Self::InsufficientCredits => "insufficient_credits",
            Self::KeyBudgetExceeded => "key_budget_exceeded",
            Self::PaymentRequired => "payment_required",
            Self::BudgetBusy => "budget_busy",
            Self::SpendingBudgetExceeded => "spending_budget_exceeded",
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
    pub(super) fn from_provider_status(
        status: u16,
        moderated: bool,
        provider: Provider,
        limit_source: Option<&str>,
    ) -> Self {
        let initial = Self::from_status_code(status, moderated);
        if provider != Provider::OpenRouter || moderated || !PAYMENT_STATUSES.contains(&status) {
            return initial;
        }
        match limit_source {
            Some("openrouter_key_limit") => Self::KeyBudgetExceeded,
            Some("openrouter_credits") => Self::InsufficientCredits,
            Some("openrouter_in_flight_budget") => Self::BudgetBusy,
            Some("openrouter_guardrail_budget" | "openrouter_workspace_budget") => {
                Self::SpendingBudgetExceeded
            },
            _ if status == 402 => Self::PaymentRequired,
            _ => initial,
        }
    }

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
            | Self::BudgetBusy
            | Self::Connect
            | Self::Transport
            | Self::RateLimited
            | Self::Unavailable
            | Self::InvalidResponse => true,
            Self::Tls
            | Self::Moderation
            | Self::CredentialRejected
            | Self::InsufficientCredits
            | Self::KeyBudgetExceeded
            | Self::PaymentRequired
            | Self::SpendingBudgetExceeded
            | Self::Rejected
            | Self::ModelMismatch
            | Self::ResponseTooLarge => false,
        }
    }

    pub(super) const fn read_payment_remedy(self) -> Option<&'static str> {
        match self {
            Self::KeyBudgetExceeded => {
                Some(
                    "the key has exhausted its budget; check its allowance and wait for a reset \
                     or ask its owner to raise the limit",
                )
            },
            Self::InsufficientCredits => {
                Some(
                    "available OpenRouter credits cannot cover this request; reduce its size or \
                     fund the account",
                )
            },
            Self::SpendingBudgetExceeded => {
                Some(
                    "an OpenRouter spending budget is exhausted; wait for reset or ask its \
                     administrator",
                )
            },
            Self::BudgetBusy => {
                Some(
                    "OpenRouter funds are committed to other requests; wait for the provider \
                     retry delay",
                )
            },
            Self::PaymentRequired => {
                Some("OpenRouter refused payment without identifying the exhausted budget")
            },
            Self::Connect
            | Self::CredentialRejected
            | Self::InvalidResponse
            | Self::ModelMismatch
            | Self::Moderation
            | Self::RateLimited
            | Self::Rejected
            | Self::ResponseTooLarge
            | Self::Timeout
            | Self::Tls
            | Self::Transport
            | Self::Unavailable => None,
        }
    }
}
