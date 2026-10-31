use crate::{JevError, RequestError};

pub(in crate::client::tests) fn request_failure(error: &JevError) -> Option<&RequestError> {
    if let JevError::Request(failure) = error {
        Some(failure)
    } else {
        None
    }
}
