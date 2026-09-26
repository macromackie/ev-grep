//! Jev's typed decision protocol over OpenRouter or direct TypeSafe HTTP.

mod config;
mod protocol;

pub use config::Provider;
pub use protocol::MIN_CONFIDENCE;

use std::time::Duration;

use anyhow::{Context, Result, ensure};
use ev_grep_core::{Assessment, Evaluator, MAX_QUERY_BYTES, Source};
use reqwest::{
    Client,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};

pub struct Jev {
    client: Client,
    endpoint: String,
    model: String,
    prompt: protocol::Prompt,
}

impl Jev {
    pub fn new(provider: Provider, model: &str, key: &str) -> Result<Self> {
        provider.model(Some(model))?;
        Self::connect(provider.endpoint(), model, key)
    }

    fn connect(endpoint: &str, model: &str, key: &str) -> Result<Self> {
        ensure!(!key.trim().is_empty(), "API key is empty");
        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {key}")).context("invalid API key header")?;
        authorization.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, authorization);
        let client = Client::builder()
            .default_headers(headers)
            .user_agent(concat!("ev-grep/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            client,
            endpoint: endpoint.into(),
            model: model.into(),
            prompt: protocol::prompt()?,
        })
    }
}

impl Evaluator for Jev {
    async fn assess(&self, query: &str, source: &Source) -> Result<Assessment> {
        ensure!(
            !query.trim().is_empty() && query.len() <= MAX_QUERY_BYTES,
            "query must contain 1–8192 bytes"
        );
        let body =
            serde_json::to_vec(&protocol::request(&self.model, &self.prompt, query, source))?;
        ensure!(
            body.len() <= 96 * 1024,
            "encoded request exceeds 98304 bytes; context was not truncated"
        );
        let mut response = self
            .client
            .post(&self.endpoint)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|error| error.without_url())
            .context("Jev request failed")?;
        ensure!(
            response.status().is_success(),
            "Jev returned HTTP {}; no assessment was recorded",
            response.status().as_u16()
        );
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
        protocol::assessment(&bytes, &self.model)
    }
}

#[cfg(test)]
mod tests;
