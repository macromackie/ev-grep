//! Jev's tracing spans and its closed set of events.
//!
//! They record statuses, kinds, sizes, counts, identifiers, and durations, never a query, prompt,
//! source, credential, header other than a request identifier, or provider message. Jev installs no
//! subscriber, so an application that installs one decides what is kept and where it goes.

use core::time::Duration;

use tracing::{Level, Span, field};

use crate::{FailureKind, Provider, RequestError};

struct FailureFields<'failure> {
    failure: Option<&'failure RequestError>,
}

impl FailureFields<'_> {
    fn read_kind(&self) -> Option<field::DisplayValue<FailureKind>> {
        self.failure.map(|error| field::display(error.kind))
    }

    fn read_retry_after_ms(&self) -> Option<u64> {
        self.failure
            .and_then(|error| error.retry_after)
            .map(to_millis)
    }

    fn read_status(&self) -> Option<u16> {
        let failure = self.failure?;
        failure.status
    }
}

/// The events Jev records. Each event names its member in an `event` field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LogEvent {
    /// An attempt received a valid answer or failed. A cancelled attempt, such as the slower of a
    /// hedged pair, records none; its `provider.attempt` span still closes.
    AttemptFinished,
    /// A logical request returned an answer or a failure.
    RequestFinished { attempts: usize, hedges: usize },
}

impl LogEvent {
    const fn name(self) -> &'static str {
        match self {
            Self::AttemptFinished => "provider.attempt_finished",
            Self::RequestFinished { .. } => "provider.request_finished",
        }
    }
}

/// What a request waits for before an attempt. Each wait is its own `provider.wait` span, apart
/// from the attempt it precedes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Wait {
    /// The delay before the next attempt after a failed one.
    Backoff,
    /// The end of the rate limit that the provider set for every request.
    Cooldown,
    /// A slot in the provider concurrency pool.
    Permit,
}

impl Wait {
    const fn name(self) -> &'static str {
        match self {
            Self::Backoff => "backoff",
            Self::Cooldown => "cooldown",
            Self::Permit => "permit",
        }
    }
}

/// One logical request; its waits and attempts are child spans. `body_bytes` is the encoded
/// request's size.
pub(crate) fn request(provider: Provider, model: &str, body_bytes: usize) -> Span {
    tracing::info_span!("provider.request", %provider, model, body_bytes)
}

/// One physical attempt of the current request, counted from 1. A hedge is a duplicate of an
/// attempt still in flight.
pub(crate) fn create_attempt_span(attempt: usize, hedge: bool) -> Span {
    tracing::info_span!("provider.attempt", attempt, hedge)
}

pub(crate) fn wait(reason: Wait) -> Span {
    tracing::info_span!("provider.wait", reason = reason.name())
}

/// Records how the current attempt ended. A failure carries its kind and the provider details its
/// error keeps.
pub(crate) fn record_attempt_finished<Success>(
    result: &Result<Success, RequestError>,
    elapsed: Duration,
) {
    record_finished(result, LogEvent::AttemptFinished, elapsed);
}

/// Records how the current request ended, after `attempts` attempts, `hedges` of them hedges.
pub(crate) fn request_finished<Success>(
    result: &Result<Success, RequestError>,
    attempts: usize,
    hedges: usize,
    elapsed: Duration,
) {
    record_finished(
        result,
        LogEvent::RequestFinished { attempts, hedges },
        elapsed,
    );
}

fn record_finished<Success>(
    result: &Result<Success, RequestError>,
    event: LogEvent,
    elapsed: Duration,
) {
    let failure = result.as_ref().err();
    let fields = FailureFields { failure };
    match event {
        LogEvent::AttemptFinished => {
            tracing::event!(
                Level::INFO,
                event = event.name(),
                elapsed_ms = to_millis(elapsed),
                kind = fields.read_kind(),
                status = fields.read_status(),
                retry_after_ms = fields.read_retry_after_ms(),
                request_id = failure.and_then(|error| error.request_id.as_deref()),
                cf_ray = failure.and_then(|error| error.cf_ray.as_deref()),
            );
        },
        LogEvent::RequestFinished { attempts, hedges } => {
            tracing::event!(
                Level::INFO,
                event = event.name(),
                attempts,
                hedges,
                elapsed_ms = to_millis(elapsed),
                kind = fields.read_kind(),
                status = fields.read_status(),
                retry_after_ms = fields.read_retry_after_ms(),
            );
        },
    }
}

/// Whole milliseconds, as a field that subscribers record as a number.
fn to_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
