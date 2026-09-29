//! Jev's typed decision protocol over OpenRouter or direct TypeSafe HTTP.

mod config;
mod protocol;
mod transport;

pub use config::Provider;
pub use protocol::MIN_CONFIDENCE;

use std::time::Duration;

use anyhow::{Context, Result, ensure};
use ev_grep_core::{Assessment, DecisionBatch, Evaluator, MAX_QUERY_BYTES, Question, Source};
use reqwest::{
    Client,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};

pub struct Jev {
    client: Client,
    transport: transport::Transport,
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
            .timeout(Duration::from_secs(5))
            .build()?;
        Ok(Self {
            client,
            transport: transport::Transport::new(4)?,
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

    pub fn with_jobs(mut self, jobs: usize) -> Result<Self> {
        self.transport = transport::Transport::new(jobs)?;
        Ok(self)
    }

    async fn send(&self, request: &serde_json::Value) -> Result<DecisionBatch> {
        let body = serde_json::to_vec(request)?;
        ensure!(
            body.len() <= 96 * 1024,
            "encoded request exceeds 98304 bytes; context was not truncated"
        );
        let ids: Vec<_> = request["questions"]
            .as_object()
            .context("missing questions")?
            .keys()
            .map(String::as_str)
            .collect();
        let completed = self
            .transport
            .post(&self.client, &self.endpoint, &body, |bytes| {
                protocol::assessment(bytes, &self.model, &ids)
            })
            .await?;
        let mut batch = completed.value;
        batch.request.attempts = completed.attempts;
        batch.request.hedges = completed.hedges;
        batch.request.elapsed_ms = completed.elapsed_ms;
        Ok(batch)
    }
}

impl Evaluator for Jev {
    async fn assess(&self, query: &str, source: &Source) -> Result<Assessment> {
        validate_query(query)?;
        let request = protocol::request(&self.model, &self.prompt, query, source);
        Ok(self
            .send(&request)
            .await?
            .into_single()?
            .with_min_confidence(self.min_confidence))
    }

    async fn assess_context(&self, query: &str, state: &serde_json::Value) -> Result<Assessment> {
        self.assess_questions(&[Question { id: "match", query }], state)
            .await?
            .into_single()
    }

    async fn assess_questions(
        &self,
        questions: &[Question<'_>],
        state: &serde_json::Value,
    ) -> Result<DecisionBatch> {
        ensure!(
            !questions.is_empty() && questions.len() <= 32,
            "expected 1–32 questions"
        );
        let mut ids = std::collections::BTreeSet::new();
        for question in questions {
            validate_query(question.query)?;
            ensure!(
                !question.id.is_empty() && question.id.len() <= 64 && ids.insert(question.id),
                "question IDs must be unique and contain 1–64 bytes"
            );
        }
        self.send(&protocol::context_request(&self.model, questions, state))
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
