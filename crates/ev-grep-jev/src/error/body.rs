use super::kind::FailureKind;
use crate::Provider;

/// The fields of a provider error body that are safe to print. Flagged input and messages are never
/// kept.
#[derive(Default)]
pub(crate) struct ErrorBody {
    /// The provider's error code, such as `OpenRouter`'s numeric `error.code`, kept only when it
    /// is a short identifier.
    pub code: Option<Box<str>>,
    pub limit_source: Option<Box<str>>,
    /// Present only when the body names moderation reasons. Reasons that are not short identifiers
    /// are dropped.
    pub moderation: Option<Vec<String>>,
    pub resets_at: Option<Box<str>>,
}

impl ErrorBody {
    pub(super) fn render_message(
        &self,
        status: u16,
        provider: Provider,
        kind: FailureKind,
    ) -> String {
        let mut message = if kind == FailureKind::CredentialRejected {
            format!(
                "Jev returned HTTP {status}: credential rejected or key limit reached for \
                 provider {provider}; check {}; no further requests were started",
                provider.read_key_variable()
            )
        } else {
            format!("Jev returned HTTP {status}; no assessment was recorded")
        };
        if let Some(remedy) = kind.read_payment_remedy() {
            message.extend(["; ", remedy]);
        }
        if let Some(reset) = &self.resets_at {
            message.extend(["; budget resets at ", reset]);
        }
        if let Some(reasons) = &self.moderation {
            message.push_str("; flagged by moderation");
            if !reasons.is_empty() {
                message.extend([": ", reasons.join(", ").as_str()]);
            }
        }
        message
    }
}
