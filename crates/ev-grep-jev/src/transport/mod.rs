mod attempt;
mod error;

use anyhow::{Context, Result, bail, ensure};
use futures_util::{StreamExt, stream::FuturesUnordered};
use reqwest::Client;
use std::{sync::Mutex, time::Duration};
use tokio::{
    sync::Semaphore,
    time::{Instant, sleep_until, timeout},
};

pub(crate) struct Transport {
    slots: Semaphore,
    cooldown: Mutex<Instant>,
}

pub(crate) struct Completed<T> {
    pub value: T,
    pub attempts: usize,
    pub hedges: usize,
    pub elapsed_ms: u128,
}

impl Transport {
    pub(crate) fn new(jobs: usize) -> Result<Self> {
        ensure!((1..=256).contains(&jobs), "jobs must be between 1 and 256");
        Ok(Self {
            slots: Semaphore::new(jobs),
            cooldown: Mutex::new(Instant::now()),
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
        body: &[u8],
        validate: impl Fn(&[u8]) -> Result<T>,
    ) -> Result<Completed<T>> {
        let started = Instant::now();
        timeout(Duration::from_secs(15), async {
            let mut active = FuturesUnordered::new();
            let mut attempts = 0;
            let mut hedges = 0;
            let mut hedge_at = started + Duration::from_secs(1);
            let mut retry_at = started;
            let mut allow_hedge = true;
            let mut last_error = String::new();
            loop {
                if active.is_empty() {
                    if attempts == 3 { bail!("{last_error}"); }
                    sleep_until(retry_at.max(self.ready_at())).await;
                    let permit = self.slots.acquire().await.context("request pool closed")?;
                    // A different request may have received rate-limit pushback while we queued.
                    if self.ready_at() > Instant::now() {
                        drop(permit);
                        continue;
                    }
                    attempts += 1;
                    if attempts == 1 { hedge_at = Instant::now() + Duration::from_secs(1); }
                    active.push(attempt::send(client, endpoint, body, permit, &validate));
                }
                tokio::select! {
                    result = active.next() => {
                        match result.context("missing active request")? {
                            Ok(value) => return Ok(Completed { value, attempts, hedges, elapsed_ms: started.elapsed().as_millis() }),
                            Err(failure) => {
                                if !failure.retryable { bail!("{}", failure.message); }
                                last_error = failure.message;
                                let delay = failure.delay.unwrap_or_else(|| Duration::from_millis(250 * attempts as u64 + fastrand::u64(0..250)));
                                retry_at = Instant::now() + delay;
                                allow_hedge = false;
                                if failure.rate_limited { self.push_back(retry_at); }
                            }
                        }
                    }
                    _ = sleep_until(hedge_at), if allow_hedge && hedges == 0 && attempts < 3 && active.len() == 1 => {
                        if self.ready_at() <= Instant::now()
                            && let Ok(permit) = self.slots.try_acquire() {
                            attempts += 1;
                            hedges += 1;
                            active.push(attempt::send(client, endpoint, body, permit, &validate));
                        }
                        hedge_at = Instant::now() + Duration::from_millis(100);
                    }
                }
            }
        }).await.context("Jev assessment exceeded its 15-second budget; no assessment was recorded")?
    }
}

#[cfg(test)]
mod tests;
