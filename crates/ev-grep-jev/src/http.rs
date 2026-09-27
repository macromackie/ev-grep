use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use reqwest::{Client, Response};
use tokio::time::{Instant, sleep, timeout};

pub(super) async fn post(client: &Client, endpoint: &str, body: Vec<u8>) -> Result<Vec<u8>> {
    let budget = Duration::from_secs(60);
    let deadline = Instant::now() + budget;
    timeout(budget, async {
        for attempt in 1..=3 {
            let response = client
                .post(endpoint)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.clone())
                .send()
                .await
                .map_err(|error| error.without_url())
                .context("Jev request failed")?;
            let status = response.status();
            if status.is_success() {
                return read(response).await;
            }
            let error = diagnostic(&response, attempt);
            let retryable = matches!(
                status.as_u16(),
                408 | 429 | 500 | 502 | 503 | 504 | 520 | 522 | 524
            );
            if !retryable || attempt == 3 {
                bail!(error);
            }
            let delay = match response.headers().get(reqwest::header::RETRY_AFTER) {
                Some(value) => match value.to_str().ok().and_then(|s| s.parse::<u64>().ok()) {
                    Some(seconds) => Duration::from_secs(seconds),
                    None => bail!(error),
                },
                None => Duration::from_secs(attempt),
            };
            if delay >= deadline.saturating_duration_since(Instant::now()) {
                bail!(error);
            }
            drop(response);
            sleep(delay).await;
        }
        bail!("Jev attempts exhausted")
    })
    .await
    .context("Jev assessment exceeded its 60-second budget; no assessment was recorded")?
}

fn diagnostic(response: &Response, attempt: u64) -> String {
    let mut message = format!(
        "Jev returned HTTP {} after {attempt} attempt(s); no assessment was recorded",
        response.status().as_u16()
    );
    for name in ["x-request-id", "cf-ray"] {
        if let Some(value) = response.headers().get(name).and_then(|v| v.to_str().ok())
            && !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
        {
            message.push_str(&format!("; {name}={value}"));
        }
    }
    message
}

async fn read(mut response: Response) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| error.without_url())?
    {
        ensure!(
            bytes.len() + chunk.len() <= 64 * 1024,
            "Jev response exceeds 65536 bytes"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
