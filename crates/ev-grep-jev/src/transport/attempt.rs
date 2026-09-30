use super::error::Failure;
use reqwest::Client;
use tokio::sync::SemaphorePermit;

pub(super) async fn send<T>(
    client: &Client,
    endpoint: &str,
    body: &[u8],
    _permit: SemaphorePermit<'_>,
    validate: &impl Fn(&[u8]) -> anyhow::Result<T>,
) -> Result<T, Failure> {
    let mut response = client
        .post(endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body.to_vec())
        .send()
        .await
        .map_err(Failure::transport)?;
    if !response.status().is_success() {
        return Err(Failure::status(&response));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(Failure::transport)? {
        if bytes.len() + chunk.len() > 65536 {
            return Err(Failure::permanent("Jev response exceeds 65536 bytes"));
        }
        bytes.extend_from_slice(&chunk);
    }
    validate(&bytes).map_err(|error| Failure::invalid(error.to_string()))
}
