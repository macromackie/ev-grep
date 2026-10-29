mod attempt;

use crate::{
    Error, FailureKind, Provider, RequestError,
    telemetry::{self, Wait},
};
use futures_util::{StreamExt, stream::FuturesUnordered};
use reqwest::Client;
use std::{
    sync::{Mutex, OnceLock},
    time::Duration,
};
use tokio::{
    sync::Semaphore,
    time::{Instant, sleep_until, timeout_at},
};
use tracing::{Instrument, Span};

/// Every attempt and wait of one logical request shares this budget.
const BUDGET: Duration = Duration::from_secs(15);

pub(crate) struct Transport {
    slots: Semaphore,
    cooldown: Mutex<Instant>,
    /// The first credential rejection. Every request shares the credential, so no attempt starts after it.
    rejection: OnceLock<RequestError>,
}

pub(crate) struct Completed<T> {
    pub value: T,
    pub attempts: usize,
    pub hedges: usize,
    pub elapsed_ms: u128,
}

impl Transport {
    pub(crate) fn new(jobs: usize) -> Result<Self, Error> {
        if !(1..=256).contains(&jobs) {
            return Err(Error::Jobs);
        }
        Ok(Self {
            slots: Semaphore::new(jobs),
            cooldown: Mutex::new(Instant::now()),
            rejection: OnceLock::new(),
        })
    }

    fn ready_at(&self) -> Instant {
        *self
            .cooldown
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn push_back(&self, until: Instant) {
        let mut cooldown = self
            .cooldown
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        *cooldown = (*cooldown).max(until);
    }

    pub(crate) async fn post<T>(
        &self,
        client: &Client,
        endpoint: &str,
        provider: Provider,
        body: &[u8],
        validate: impl Fn(&[u8]) -> Result<T, RequestError>,
    ) -> Result<Completed<T>, RequestError> {
        let started = Instant::now();
        let deadline = started + BUDGET;
        let mut attempts = 0;
        let mut hedges = 0;
        let result = timeout_at(deadline, async {
            let mut active = FuturesUnordered::new();
            let mut hedge_at = started + Duration::from_secs(1);
            let mut retry_at = started;
            let mut allow_hedge = true;
            loop {
                if active.is_empty() {
                    // A shared rate limit that outlasts the budget leaves no time for an attempt, so queued work fails
                    // at once instead of waiting out its budget.
                    let ready_at = self.ready_at();
                    if ready_at >= deadline {
                        return Err(RequestError::rate_limit_exceeds_budget(ready_at.saturating_duration_since(Instant::now())));
                    }
                    let resume_at = retry_at.max(ready_at);
                    let wait = if resume_at <= Instant::now() {
                        Span::none()
                    } else if ready_at > retry_at {
                        telemetry::wait(Wait::Cooldown)
                    } else {
                        telemetry::wait(Wait::Backoff)
                    };
                    sleep_until(resume_at).instrument(wait).await;
                    let Ok(permit) = self.slots.acquire().instrument(telemetry::wait(Wait::Permit)).await else {
                        unreachable!("the transport never closes its request pool");
                    };
                    // A different request may have received rate-limit pushback while we queued.
                    if self.ready_at() > Instant::now() {
                        drop(permit);
                        continue;
                    }
                    // A different request may have had the credential rejected while we queued or waited to retry.
                    if let Some(rejection) = self.rejection.get() { return Err(rejection.clone()); }
                    attempts += 1;
                    if attempts == 1 { hedge_at = Instant::now() + Duration::from_secs(1); }
                    active.push(attempt::send(client, endpoint, provider, body, permit, &validate).instrument(telemetry::attempt(attempts, false)));
                }
                // The loop starts an attempt whenever none is active, so `next` always yields a result.
                tokio::select! {
                    Some(result) = active.next() => {
                        match result {
                            Ok(value) => return Ok(Completed { value, attempts, hedges, elapsed_ms: started.elapsed().as_millis() }),
                            Err(failure) => {
                                let delay = failure.retry_after.unwrap_or_else(|| Duration::from_millis(250 * attempts as u64 + fastrand::u64(0..250)));
                                retry_at = Instant::now() + delay;
                                // Requests that share the limit honor even the rate limit that ends this request.
                                if failure.kind == FailureKind::RateLimited { self.push_back(retry_at); }
                                if failure.kind == FailureKind::CredentialRejected { self.rejection.get_or_init(|| failure.clone()); }
                                // A retry that could start only at or after the deadline is never made, so the failure returns now.
                                if !failure.kind.retryable() || (active.is_empty() && (attempts == 3 || retry_at >= deadline)) { return Err(failure); }
                                allow_hedge = false;
                            }
                        }
                    }
                    _ = sleep_until(hedge_at), if allow_hedge && hedges == 0 && attempts < 3 && active.len() == 1 => {
                        if self.ready_at() <= Instant::now()
                            && self.rejection.get().is_none()
                            && let Ok(permit) = self.slots.try_acquire() {
                            attempts += 1;
                            hedges += 1;
                            active.push(attempt::send(client, endpoint, provider, body, permit, &validate).instrument(telemetry::attempt(attempts, true)));
                        }
                        hedge_at = Instant::now() + Duration::from_millis(100);
                    }
                }
            }
        }).await.unwrap_or_else(|_| Err(RequestError::budget_exhausted()));
        telemetry::request_finished(&result, attempts, hedges, started.elapsed());
        result
    }
}

#[cfg(test)]
mod tests;
