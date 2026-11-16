use ev_grep_core::Question;
use serde_json::Value;

use crate::{JevError, protocol};

/// Count the complete context request's JSON bytes using the same builder as the sending path.
/// This does not send a request or apply byte, question-count, or provider token limits.
#[inline]
pub fn count_context_request_bytes(
    model: &str,
    questions: &[Question<'_>],
    state: &Value,
) -> Result<usize, JevError> {
    serde_json::to_vec(&protocol::build_context_request(model, questions, state))
        .map(|bytes| bytes.len())
        .map_err(JevError::Encode)
}
