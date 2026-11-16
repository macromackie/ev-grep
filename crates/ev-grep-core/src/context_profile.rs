use serde::{Deserialize, Serialize};

/// Explicit byte-admission policy for labeled context. These memory bounds do not estimate tokens.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextProfile {
    /// Opt into the provider's larger context window, with its documented accuracy trade-off.
    Long,
    /// Preserve the original context and request admission limits.
    #[default]
    Standard,
}

impl ContextProfile {
    /// The largest encoded evidence a context consumer admits before adding question metadata.
    #[inline]
    #[must_use]
    pub const fn read_max_evidence_bytes(self) -> usize {
        match self {
            Self::Long => 1024 * 1024,
            Self::Standard => 64 * 1024,
        }
    }

    /// The largest complete encoded request, including the model, questions, and state.
    #[inline]
    #[must_use]
    pub const fn read_max_request_bytes(self) -> usize {
        match self {
            Self::Long => 2 * 1024 * 1024,
            Self::Standard => 96 * 1024,
        }
    }

    /// The largest encoded state a provider adapter admits.
    #[inline]
    #[must_use]
    pub const fn read_max_state_bytes(self) -> usize {
        match self {
            Self::Long => 1024 * 1024,
            Self::Standard => 96 * 1024,
        }
    }
}
