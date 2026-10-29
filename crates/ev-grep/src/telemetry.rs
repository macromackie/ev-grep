//! The trace that `--trace` writes: one JSON record per line, holding ev-grep's spans and events.
//!
//! Its first record names the trace schema, the binary's version and revision, and the environment.
//! Records hold statuses, kinds, sizes, counts, paths, identifiers, and durations, never the query,
//! source text, a credential, or a provider message.

use alloc::sync::Arc;
use std::{env, fs::File, path::Path};

use clap::ValueEnum;
use tracing::{Level, Span, field};
use tracing_subscriber::{
    filter::Targets,
    fmt::{self, format::FmtSpan, writer::BoxMakeWriter},
    prelude::*,
};

use crate::{error::CliError, output};

/// The version of the trace's record shapes. Raise it when a record's fields change meaning or a
/// field is removed.
pub(crate) const TRACE_SCHEMA_VERSION: u32 = 2;

/// The events ev-grep records. Each event names its member in an `event` field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LogEvent {
    /// The trace began; this is its first record.
    TraceStarted,
}

impl LogEvent {
    const fn name(self) -> &'static str {
        match self {
            Self::TraceStarted => "trace.started",
        }
    }
}

/// Where a trace was recorded, which its first record names.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum Environment {
    Ci,
    Local,
    Release,
}

impl Environment {
    /// `ci` when the `CI` variable that CI services set is present, otherwise `local`.
    pub(crate) fn detect() -> Self {
        if env::var_os("CI").is_some_and(|value| !value.is_empty()) {
            Self::Ci
        } else {
            Self::Local
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Ci => "ci",
            Self::Local => "local",
            Self::Release => "release",
        }
    }
}

/// Writes the trace to `path`, or to stderr for `-`, and records its first event.
///
/// Only the ev-grep crates' records at INFO are kept: the HTTP client's own records name endpoints
/// and connections. Each span records its duration when it closes. A failed write does not change
/// the search.
pub(crate) fn install(path: &Path, environment: Environment) -> Result<(), CliError> {
    let writer = if path.as_os_str() == "-" {
        BoxMakeWriter::new(output::open_stderr)
    } else {
        let file = File::create(path).map_err(|source| {
            CliError::Trace {
                path: path.to_path_buf(),
                source,
            }
        })?;
        BoxMakeWriter::new(Arc::new(file))
    };
    tracing_subscriber::registry()
        // The prefix also selects ev_grep_core and ev_grep_jev.
        .with(Targets::new().with_target("ev_grep", Level::INFO))
        .with(
            fmt::layer()
                .json()
                .with_writer(writer)
                .with_span_events(FmtSpan::CLOSE),
        )
        .init();
    tracing::info!(
        event = LogEvent::TraceStarted.name(),
        trace_schema_version = TRACE_SCHEMA_VERSION,
        version = env!("CARGO_PKG_VERSION"),
        revision = env!("EV_GREP_REVISION"),
        environment = environment.name(),
    );
    Ok(())
}

/// The whole run, including state assessments. Its `exit_code` is recorded when the run ends.
pub(crate) fn search() -> Span {
    tracing::info_span!("ev_grep.search", exit_code = field::Empty)
}
