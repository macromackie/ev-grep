use reqwest::Client;
use tokio::sync::SemaphorePermit;
use tracing::Instrument as _;

use super::{attempt, result::AttemptResult};
use crate::{Provider, RequestError, telemetry};

/// One request's body and destination, which each of its attempts sends.
pub(super) struct Post<'post, Validator> {
    pub body: &'post [u8],
    pub client: &'post Client,
    pub endpoint: &'post str,
    pub provider: Provider,
    pub validate: Validator,
}

impl<Validated, Validator> Post<'_, Validator>
where
    Validated: Send,
    Validator: Fn(&[u8]) -> Result<Validated, RequestError> + Sync,
{
    /// Attempt `attempt` of the request, counted from 1, holding `permit` until its result is
    /// processed so a credential rejection or rate limit is recorded before another attempt starts.
    pub(super) fn send<'permit>(
        &'permit self,
        permit: SemaphorePermit<'permit>,
        attempt: usize,
        hedge: bool,
    ) -> impl Future<Output = AttemptResult<'permit, Validated>> + Send + 'permit
    where
        Validated: 'permit,
    {
        attempt::send(
            self.client,
            self.endpoint,
            self.provider,
            self.body,
            permit,
            &self.validate,
        )
        .instrument(telemetry::create_attempt_span(attempt, hedge))
    }
}
