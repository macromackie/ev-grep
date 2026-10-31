use super::request_failure;
use crate::{FailureKind, JevError};

pub(in crate::client::tests) fn read_kind(error: &JevError) -> Option<FailureKind> {
    request_failure(error).map(|failure| failure.kind)
}
