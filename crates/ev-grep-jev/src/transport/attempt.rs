use reqwest::{Client, Response, header::CONTENT_TYPE};
use tokio::{sync::SemaphorePermit, time::Instant};

use super::result::AttemptResult;
use crate::{Provider, RequestError, error::ErrorBody, protocol, telemetry};

/// The largest response body Jev reads, in bytes.
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

pub(super) async fn send<'permit, Validated, Validator>(
    client: &Client,
    endpoint: &str,
    provider: Provider,
    request: &[u8],
    permit: SemaphorePermit<'permit>,
    validate: &Validator,
) -> AttemptResult<'permit, Validated>
where
    Validated: Send,
    Validator: Fn(&[u8]) -> Result<Validated, RequestError> + Sync,
{
    let started = Instant::now();
    let result = async {
        let mut response = client
            .post(endpoint)
            .header(CONTENT_TYPE, "application/json")
            .body(request.to_vec())
            .send()
            .await
            .map_err(RequestError::from_transport)?;
        if !response.status().is_success() {
            // Only a 401, 402, or 403 body is read, and only for its error code and moderation
            // reasons.
            let body = if (401..=403).contains(&response.status().as_u16()) {
                protocol::parse_error_body(&read(&mut response).await?)
            } else {
                ErrorBody::default()
            };
            return Err(RequestError::from_status_response(
                &response, body, provider,
            ));
        }
        validate(&read(&mut response).await?)
    }
    .await;
    telemetry::record_attempt_finished(&result, started.elapsed());
    (permit, result)
}

async fn read(response: &mut Response) -> Result<Vec<u8>, RequestError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(RequestError::from_transport)?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(RequestError::new_response_too_large());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
