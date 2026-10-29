mod attempt;

use core::time::Duration;
use std::sync::{Mutex, OnceLock, PoisonError};

use futures_util::{StreamExt as _, stream::FuturesUnordered};
use reqwest::Client;
use tokio::{
    sync::{Semaphore, SemaphorePermit},
    time::{Instant, sleep_until, timeout_at},
};
use tracing::{Instrument as _, Span};

use crate::{
    FailureKind,
    JevError,
    Provider,
    RequestError,
    telemetry::{self, Wait},
};

/// Every attempt and wait of one logical request shares this budget.
const BUDGET: Duration = Duration::from_secs(15);
/// The most attempts, hedges included, that one logical request makes.
const MAX_ATTEMPTS: usize = 3;

/// A completed attempt retains its slot until the shared failure state is updated.
type AttemptResult<'permit, Validated> =
    (SemaphorePermit<'permit>, Result<Validated, RequestError>);

#[derive(Debug)]
pub(crate) struct Transport {
    cooldown: Mutex<Instant>,
    /// The first credential rejection. Every request shares the credential, so no attempt starts
    /// after it.
    rejection: OnceLock<RequestError>,
    slots: Semaphore,
}

pub(crate) struct Completed<Validated> {
    pub attempts: usize,
    pub elapsed_ms: u128,
    pub hedges: usize,
    pub value: Validated,
}

/// The attempts one logical request has made so far.
#[derive(Default)]
struct Count {
    attempts: usize,
    hedges: usize,
}

impl Transport {
    /// Waits for the shared rate limit, the retry delay at `retry_at`, and a pool slot, then
    /// returns the slot for the next attempt.
    #[expect(
        clippy::unreachable,
        reason = "the transport never closes its request pool, so acquiring a slot cannot fail"
    )]
    async fn acquire(
        &self,
        deadline: Instant,
        retry_at: Instant,
    ) -> Result<SemaphorePermit<'_>, RequestError> {
        loop {
            // A shared rate limit that outlasts the budget leaves no time for an attempt, so queued
            // work fails at once instead of waiting out its budget.
            let ready_at = self.compute_ready_at();
            if ready_at >= deadline {
                return Err(RequestError::new_rate_limit_exceeds_budget(
                    ready_at.saturating_duration_since(Instant::now()),
                ));
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
            let Ok(permit) = self
                .slots
                .acquire()
                .instrument(telemetry::wait(Wait::Permit))
                .await
            else {
                unreachable!("the transport never closes its request pool");
            };
            // A different request may have received rate-limit pushback while we queued.
            if self.compute_ready_at() > Instant::now() {
                drop(permit);
                continue;
            }
            // A different request may have had the credential rejected while we queued or waited
            // to retry.
            if let Some(rejection) = self.rejection.get() {
                return Err(rejection.clone());
            }
            return Ok(permit);
        }
    }

    /// A slot for a hedge, when no rate limit or credential rejection holds requests back and the
    /// pool has one free.
    fn acquire_hedge_slot(&self) -> Option<SemaphorePermit<'_>> {
        if self.compute_ready_at() > Instant::now() || self.rejection.get().is_some() {
            return None;
        }
        self.slots.try_acquire().ok()
    }

    fn compute_ready_at(&self) -> Instant {
        *self.cooldown.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records a failed attempt, and returns when the next attempt may start, or the failure when
    /// the request ends with it. `idle` says no other attempt is in flight.
    fn fail(
        &self,
        failure: RequestError,
        attempts: usize,
        idle: bool,
        deadline: Instant,
    ) -> Result<Instant, RequestError> {
        let jitter = Duration::from_millis(fastrand::u64(0..250));
        let delay = failure.retry_after.unwrap_or_else(|| {
            Duration::from_millis(250)
                .saturating_mul(u32::try_from(attempts).unwrap_or(u32::MAX))
                .saturating_add(jitter)
        });
        let retry_at = add_delay(Instant::now(), delay);
        // Requests that share the limit honor even the rate limit that ends this request.
        if failure.kind == FailureKind::RateLimited {
            self.push_back(retry_at);
        }
        if failure.kind == FailureKind::CredentialRejected {
            let _: &RequestError = self.rejection.get_or_init(|| failure.clone());
        }
        // A retry that could start only at or after the deadline is never made, so the failure
        // returns now.
        if !failure.kind.is_retryable()
            || (idle && (attempts == MAX_ATTEMPTS || retry_at >= deadline))
        {
            return Err(failure);
        }
        Ok(retry_at)
    }

    pub(crate) fn new(jobs: usize) -> Result<Self, JevError> {
        if !(1..=256).contains(&jobs) {
            return Err(JevError::Jobs);
        }
        Ok(Self {
            cooldown: Mutex::new(Instant::now()),
            rejection: OnceLock::new(),
            slots: Semaphore::new(jobs),
        })
    }

    pub(crate) async fn post<Validated, Validator>(
        &self,
        client: &Client,
        endpoint: &str,
        provider: Provider,
        body: &[u8],
        validate: Validator,
    ) -> Result<Completed<Validated>, RequestError>
    where
        Validated: Send,
        Validator: Fn(&[u8]) -> Result<Validated, RequestError> + Sync,
    {
        let started = Instant::now();
        let deadline = add_delay(started, BUDGET);
        let post = Post {
            body,
            client,
            endpoint,
            provider,
            validate,
        };
        let mut count = Count::default();
        let result = timeout_at(deadline, self.run(&post, deadline, &mut count))
            .await
            .unwrap_or_else(|_elapsed| Err(RequestError::new_budget_exhausted()));
        telemetry::request_finished(&result, count.attempts, count.hedges, started.elapsed());
        result.map(|value| {
            Completed {
                attempts: count.attempts,
                elapsed_ms: started.elapsed().as_millis(),
                hedges: count.hedges,
                value,
            }
        })
    }

    fn push_back(&self, until: Instant) {
        let mut cooldown = self.cooldown.lock().unwrap_or_else(PoisonError::into_inner);
        *cooldown = (*cooldown).max(until);
    }

    /// Makes attempts until one answers, the request fails, or its caller's deadline cancels it.
    #[expect(
        clippy::integer_division_remainder_used,
        reason = "tokio::select! picks its first branch with a remainder"
    )]
    async fn run<Validated, Validator>(
        &self,
        post: &Post<'_, Validator>,
        deadline: Instant,
        count: &mut Count,
    ) -> Result<Validated, RequestError>
    where
        Validated: Send,
        Validator: Fn(&[u8]) -> Result<Validated, RequestError> + Sync,
    {
        let mut active = FuturesUnordered::new();
        let mut hedge_at = add_delay(Instant::now(), Duration::from_secs(1));
        let mut retry_at = Instant::now();
        let mut allow_hedge = true;
        loop {
            if active.is_empty() {
                let permit = self.acquire(deadline, retry_at).await?;
                count.attempts = count.attempts.saturating_add(1);
                // Only the first attempt can be hedged: a failure turns hedging off.
                hedge_at = add_delay(Instant::now(), Duration::from_secs(1));
                active.push(post.send(permit, count.attempts, false));
            }
            let hedge_open = allow_hedge
                && count.hedges == 0
                && count.attempts < MAX_ATTEMPTS
                && active.len() == 1;
            // The loop starts an attempt whenever none is active, so `next` always yields a
            // result.
            tokio::select! {
                Some((_permit, result)) = active.next() => match result {
                    Ok(value) => return Ok(value),
                    Err(failure) => {
                        let idle = active.is_empty();
                        retry_at = self.fail(failure, count.attempts, idle, deadline)?;
                        allow_hedge = false;
                    },
                },
                () = sleep_until(hedge_at),
                    if hedge_open =>
                {
                    let hedge = self
                        .acquire_hedge_slot()
                        .map(|permit| post.send(permit, count.attempts.saturating_add(1), true));
                    let hedged = usize::from(hedge.is_some());
                    count.attempts = count.attempts.saturating_add(hedged);
                    count.hedges = count.hedges.saturating_add(hedged);
                    active.extend(hedge);
                    hedge_at = add_delay(Instant::now(), Duration::from_millis(100));
                }
            }
        }
    }
}

/// One request's body and destination, which each of its attempts sends.
struct Post<'post, Validator> {
    body: &'post [u8],
    client: &'post Client,
    endpoint: &'post str,
    provider: Provider,
    validate: Validator,
}

impl<Validated, Validator> Post<'_, Validator>
where
    Validated: Send,
    Validator: Fn(&[u8]) -> Result<Validated, RequestError> + Sync,
{
    /// Attempt `attempt` of the request, counted from 1, holding `permit` until its result is
    /// processed so a credential rejection or rate limit is recorded before another attempt starts.
    fn send<'permit>(
        &'permit self,
        permit: SemaphorePermit<'permit>,
        attempt: usize,
        hedge: bool,
    ) -> impl Future<Output = AttemptResult<'permit, Validated>> + Send + 'permit
    where
        Validated: 'permit,
    {
        attempt::send(
            self.client,
            self.endpoint,
            self.provider,
            self.body,
            permit,
            &self.validate,
        )
        .instrument(telemetry::create_attempt_span(attempt, hedge))
    }
}

/// `delay` after `at`. Every delay is at most the request budget, which no clock comes near to
/// overflowing.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "delays are at most the 15-second budget past the current time"
)]
fn add_delay(at: Instant, delay: Duration) -> Instant {
    at + delay
}

#[cfg(test)]
mod tests;
