use crate::{Provider, RequestError, protocol, telemetry};
use reqwest::{Client, Response};
use tokio::{sync::SemaphorePermit, time::Instant};

pub(super) async fn send<T>(
    client: &Client,
    endpoint: &str,
    provider: Provider,
    body: &[u8],
    _permit: SemaphorePermit<'_>,
    validate: &impl Fn(&[u8]) -> Result<T, RequestError>,
) -> Result<T, RequestError> {
    let started = Instant::now();
    let result = async {
        let mut response = client
            .post(endpoint)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_vec())
            .send()
            .await
            .map_err(RequestError::transport)?;
        if !response.status().is_success() {
            // Only a 401, 402, or 403 body is read, and only for its error code and moderation reasons.
            let body = if (401..=403).contains(&response.status().as_u16()) {
                protocol::error_body(&read(&mut response).await?)
            } else {
                protocol::ErrorBody::default()
            };
            return Err(RequestError::status(&response, body, provider));
        }
        validate(&read(&mut response).await?)
    }
    .await;
    telemetry::attempt_finished(&result, started.elapsed());
    result
}

async fn read(response: &mut Response) -> Result<Vec<u8>, RequestError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(RequestError::transport)? {
        if bytes.len() + chunk.len() > 65536 {
            return Err(RequestError::response_too_large());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
