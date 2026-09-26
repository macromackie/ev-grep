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
    min_confidence: f64,
}

impl Jev {
    pub fn new(provider: Provider, model: &str, key: &str) -> Result<Self> {
        Self::with_endpoint(provider, model, key, provider.endpoint())
    }

    pub fn with_endpoint(
        provider: Provider,
        model: &str,
        key: &str,
        endpoint: &str,
    ) -> Result<Self> {
        provider.model(Some(model))?;
        let url = reqwest::Url::parse(endpoint)
            .map_err(|_| anyhow::anyhow!("invalid provider endpoint"))?;
        ensure!(
            matches!(url.scheme(), "https" | "http")
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "provider endpoint must use HTTP or HTTPS, without credentials, query, or fragment"
        );
        Self::connect(endpoint, model, key)
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
            min_confidence: MIN_CONFIDENCE,
        })
    }

    pub fn with_min_confidence(mut self, minimum: f64) -> Result<Self> {
        ensure!(
            minimum.is_finite() && (0.0..=1.0).contains(&minimum),
            "minimum confidence must be between 0 and 1"
        );
        self.min_confidence = minimum;
        Ok(self)
    }

    async fn send(&self, request: &serde_json::Value) -> Result<Assessment> {
        let body = serde_json::to_vec(request)?;
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

impl Evaluator for Jev {
    async fn assess(&self, query: &str, source: &Source) -> Result<Assessment> {
        validate_query(query)?;
        let request = protocol::request(&self.model, &self.prompt, query, source);
        Ok(self
            .send(&request)
            .await?
            .with_min_confidence(self.min_confidence))
    }

    async fn assess_context(&self, query: &str, state: &serde_json::Value) -> Result<Assessment> {
        validate_query(query)?;
        self.send(&protocol::context_request(&self.model, query, state))
            .await
    }
}

fn validate_query(query: &str) -> Result<()> {
    ensure!(
        !query.trim().is_empty() && query.len() <= MAX_QUERY_BYTES,
        "query must contain 1–8192 bytes"
    );
    Ok(())
}

#[cfg(test)]
mod tests;
