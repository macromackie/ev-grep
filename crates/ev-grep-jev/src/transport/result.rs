use tokio::sync::SemaphorePermit;

use crate::RequestError;

/// A completed attempt retains its slot until the shared failure state is updated.
pub(super) type AttemptResult<'permit, Validated> =
    (SemaphorePermit<'permit>, Result<Validated, RequestError>);
