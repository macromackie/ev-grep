//! Jev's typed decision protocol over OpenRouter or direct TypeSafe HTTP.

mod config;
mod error;
mod protocol;
mod telemetry;
mod transport;

pub use config::Provider;
pub use error::{Error, FailureKind, RequestError};
pub use protocol::{context_prompt_id, search_prompt_id};

use std::time::Duration;

use ev_grep_core::{Assessment, Candidate, DecisionBatch, Evaluator, MAX_QUERY_BYTES, Question};
use reqwest::{
    Client,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use tracing::Instrument;

pub struct Jev {
    client: Client,
    transport: transport::Transport,
    provider: Provider,
    endpoint: String,
    model: String,
    prompt: protocol::Prompt,
}

impl Jev {
    pub fn new(provider: Provider, model: &str, key: &str) -> Result<Self, Error> {
        Self::with_endpoint(provider, model, key, provider.endpoint())
    }

    /// Send requests to `endpoint` for a model that `Provider::supported_models` lists.
    pub fn with_endpoint(
        provider: Provider,
        model: &str,
        key: &str,
        endpoint: &str,
    ) -> Result<Self, Error> {
        provider.model(Some(model))?;
        Self::with_unvalidated_model(provider, model, key, endpoint)
    }

    /// Connect to any model the provider serves, including one that `Provider::supported_models` does not list.
    /// No labeled eval has checked such a model's answers, so callers record that beside its results.
    pub fn with_unvalidated_model(
        provider: Provider,
        model: &str,
        key: &str,
        endpoint: &str,
    ) -> Result<Self, Error> {
        let url = reqwest::Url::parse(endpoint).map_err(|_| Error::InvalidEndpoint)?;
        if !(matches!(url.scheme(), "https" | "http")
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none())
        {
            return Err(Error::UnsupportedEndpoint);
        }
        Self::connect(provider, endpoint, model, key)
    }

    fn connect(provider: Provider, endpoint: &str, model: &str, key: &str) -> Result<Self, Error> {
        if key.trim().is_empty() {
            return Err(Error::EmptyKey);
        }
        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {key}")).map_err(Error::InvalidKey)?;
        authorization.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, authorization);
        let client = Client::builder()
            .default_headers(headers)
            .user_agent(concat!("ev-grep/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(Error::Client)?;
        Ok(Self {
            client,
            transport: transport::Transport::new(4)?,
            provider,
            endpoint: endpoint.into(),
            model: model.into(),
            prompt: protocol::prompt()?,
        })
    }

    pub fn with_jobs(mut self, jobs: usize) -> Result<Self, Error> {
        self.transport = transport::Transport::new(jobs)?;
        Ok(self)
    }

    /// Send a request whose questions are named `ids`.
    async fn send(
        &self,
        request: &serde_json::Value,
        ids: &[&str],
        prompt_id: &str,
    ) -> Result<DecisionBatch, Error> {
        let body = serde_json::to_vec(request).map_err(Error::Encode)?;
        if body.len() > 96 * 1024 {
            return Err(Error::RequestTooLarge);
        }
        let completed = self
            .transport
            .post(
                &self.client,
                &self.endpoint,
                self.provider,
                &body,
                |bytes| protocol::assessment(bytes, &self.model, ids),
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
}

impl Evaluator for Jev {
    type Error = Error;

    async fn assess(&self, query: &str, candidate: &Candidate) -> Result<Assessment, Error> {
        validate_query(query)?;
        candidate.validate()?;
        let request = protocol::request(&self.model, &self.prompt, query, candidate);
        Ok(self
            .send(&request, &["match"], &search_prompt_id())
            .await?
            .into_single()?)
    }

    async fn assess_questions(
        &self,
        questions: &[Question<'_>],
        state: &serde_json::Value,
    ) -> Result<DecisionBatch, Error> {
        if questions.is_empty() || questions.len() > 32 {
            return Err(Error::QuestionCount);
        }
        let mut ids = std::collections::BTreeSet::new();
        for question in questions {
            validate_query(question.query)?;
            if question.id.is_empty() || question.id.len() > 64 || !ids.insert(question.id) {
                return Err(Error::QuestionId);
            }
        }
        let state_bytes = serde_json::to_vec(state).map_err(Error::Encode)?;
        if state_bytes.len() > 96 * 1024 {
            return Err(Error::StateTooLarge);
        }
        ev_grep_core::parse_json(&state_bytes)?;
        let ids: Vec<&str> = ids.into_iter().collect();
        self.send(
            &protocol::context_request(&self.model, questions, state),
            &ids,
            &protocol::context_prompt_id(),
        )
        .await
    }
}

fn validate_query(query: &str) -> Result<(), Error> {
    if query.trim().is_empty() || query.len() > MAX_QUERY_BYTES {
        return Err(Error::InvalidQuery);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
