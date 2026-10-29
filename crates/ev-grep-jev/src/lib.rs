//! Jev's typed decision protocol over `OpenRouter` or direct `TypeSafe` HTTP.

extern crate alloc;

mod config;
mod error;
mod protocol;
mod telemetry;
mod transport;

use alloc::collections::BTreeSet;
use core::time::Duration;

pub use config::Provider;
pub use error::{FailureKind, JevError, RequestError};
use ev_grep_core::{Assessment, Candidate, DecisionBatch, Evaluator, MAX_QUERY_BYTES, Question};
pub use protocol::{build_context_prompt_id, search_prompt_id};
use reqwest::{
    Client,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
    redirect::Policy,
};
use tracing::Instrument as _;

/// The URL schemes an endpoint may use.
const ENDPOINT_SCHEMES: &[&str] = &["https", "http"];

/// A client that sends search and context questions to a provider's decision model.
#[derive(Debug)]
pub struct Jev {
    client: Client,
    endpoint: String,
    model: String,
    prompt: protocol::Prompt,
    provider: Provider,
    transport: transport::Transport,
}

impl Jev {
    fn connect(
        provider: Provider,
        endpoint: &str,
        model: &str,
        key: &str,
    ) -> Result<Self, JevError> {
        if key.trim().is_empty() {
            return Err(JevError::EmptyKey);
        }
        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {key}")).map_err(JevError::InvalidKey)?;
        authorization.set_sensitive(true);
        let mut headers = HeaderMap::new();
        drop(headers.insert(AUTHORIZATION, authorization));
        let client = Client::builder()
            .default_headers(headers)
            .user_agent(concat!("ev-grep/", env!("CARGO_PKG_VERSION")))
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(JevError::Client)?;
        Ok(Self {
            client,
            endpoint: endpoint.into(),
            model: model.into(),
            prompt: protocol::prompt()?,
            provider,
            transport: transport::Transport::new(4)?,
        })
    }

    /// Connect to the provider's own endpoint for a model that `Provider::list_supported_models`
    /// lists.
    #[inline]
    pub fn new(provider: Provider, model: &str, key: &str) -> Result<Self, JevError> {
        Self::with_endpoint(provider, model, key, provider.read_endpoint())
    }

    /// Send a request whose questions are named `ids`.
    async fn send(
        &self,
        request: &serde_json::Value,
        ids: &[&str],
        prompt_id: &str,
    ) -> Result<DecisionBatch, JevError> {
        let body = serde_json::to_vec(request).map_err(JevError::Encode)?;
        if body.len() > MAX_REQUEST_BYTES {
            return Err(JevError::RequestTooLarge);
        }
        let completed = self
            .transport
            .post(
                &self.client,
                &self.endpoint,
                self.provider,
                &body,
                |bytes| protocol::parse_assessment(bytes, &self.model, ids),
            )
            .instrument(telemetry::request(self.provider, &self.model, body.len()))
            .await?;
        let mut batch = completed.value;
        batch.request.prompt_id = Some(prompt_id.to_owned());
        batch.request.attempts = completed.attempts;
        batch.request.hedges = completed.hedges;
        batch.request.elapsed_ms = completed.elapsed_ms;
        Ok(batch)
    }

    /// Send requests to `endpoint` for a model that `Provider::list_supported_models` lists.
    #[inline]
    pub fn with_endpoint(
        provider: Provider,
        model: &str,
        key: &str,
        endpoint: &str,
    ) -> Result<Self, JevError> {
        drop(provider.select_model(Some(model))?);
        Self::with_unvalidated_model(provider, model, key, endpoint)
    }

    /// Allow at most `jobs` concurrent requests, from 1 to 256.
    #[inline]
    pub fn with_jobs(mut self, jobs: usize) -> Result<Self, JevError> {
        self.transport = transport::Transport::new(jobs)?;
        Ok(self)
    }

    /// Connect to any model the provider serves, including one that
    /// `Provider::list_supported_models` does not list. No labeled eval has checked such a model's
    /// answers, so callers record that beside its results.
    #[inline]
    pub fn with_unvalidated_model(
        provider: Provider,
        model: &str,
        key: &str,
        endpoint: &str,
    ) -> Result<Self, JevError> {
        let url = url::Url::parse(endpoint).map_err(JevError::InvalidEndpoint)?;
        if !(ENDPOINT_SCHEMES.contains(&url.scheme())
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none())
        {
            return Err(JevError::UnsupportedEndpoint);
        }
        Self::connect(provider, endpoint, model, key)
    }
}

impl Evaluator for Jev {
    type Error = JevError;

    #[inline]
    async fn assess(&self, query: &str, candidate: &Candidate) -> Result<Assessment, JevError> {
        validate_query(query)?;
        candidate.validate()?;
        let request = protocol::request(&self.model, &self.prompt, query, candidate);
        Ok(self
            .send(&request, &["match"], &search_prompt_id())
            .await?
            .into_single()?)
    }

    #[inline]
    async fn assess_questions(
        &self,
        questions: &[Question<'_>],
        state: &serde_json::Value,
    ) -> Result<DecisionBatch, JevError> {
        if questions.is_empty() || questions.len() > 32 {
            return Err(JevError::QuestionCount);
        }
        let mut unique = BTreeSet::new();
        for question in questions {
            validate_query(question.query)?;
            if question.id.is_empty() || question.id.len() > 64 || !unique.insert(question.id) {
                return Err(JevError::QuestionId);
            }
        }
        let state_bytes = serde_json::to_vec(state).map_err(JevError::Encode)?;
        if state_bytes.len() > MAX_REQUEST_BYTES {
            return Err(JevError::StateTooLarge);
        }
        drop(ev_grep_core::parse_json(&state_bytes)?);
        let ids: Vec<&str> = unique.into_iter().collect();
        self.send(
            &protocol::build_context_request(&self.model, questions, state),
            &ids,
            &build_context_prompt_id(),
        )
        .await
    }
}

/// The largest encoded request or state Jev sends, in bytes.
const MAX_REQUEST_BYTES: usize = 96 * 1024;

fn validate_query(query: &str) -> Result<(), JevError> {
    if query.trim().is_empty() || query.len() > MAX_QUERY_BYTES {
        return Err(JevError::InvalidQuery);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
