//! The core's tracing span for each scanned input. It records the input's path, size, outcome, and
//! failure or skip reason, never the query or source text. The core installs no subscriber.

use tracing::{Span, field};

use crate::{Candidate, Failure, Input, ScanEvent, SkipReason};

/// One input of a scan, named by its path as output names it. The request that assesses it is a
/// child span.
pub(crate) fn create_file_span(input: &Input) -> Span {
    let span = tracing::info_span!(
        "ev_grep.file",
        path = field::Empty,
        bytes = field::Empty,
        outcome = field::Empty,
        reason = field::Empty,
        kind = field::Empty,
    );
    let _: &Span = match input {
        Input::File(target) => span.record("path", field::display(target.path.display())),
        Input::Candidate(candidate) => {
            span.record("path", candidate.file.as_deref().unwrap_or("<stdin>"))
        },
        Input::Error(error) => span.record("path", error.path.as_str()),
    };
    span
}

/// Records the size of the current input's candidate, with its supporting sources, before it is
/// assessed.
pub(crate) fn record_loaded(candidate: &Candidate) {
    let _: &Span = Span::current().record("bytes", candidate.count_bytes());
}

/// Records how the scan ended the current input. `outcome` names the JSON record the input
/// produced, and a failed input's `kind`, including a fatal one that stops the scan, is its error
/// kind.
pub(crate) fn record_file_finished<ScanError>(event: &Result<ScanEvent, ScanError>)
where
    ScanError: Failure,
{
    let span = Span::current();
    let _: &Span = match event {
        Ok(ScanEvent::Result { .. }) => span.record("outcome", "result"),
        Ok(ScanEvent::Skipped { reason, .. }) => {
            let name = match reason {
                SkipReason::Binary => "binary",
                SkipReason::Symlink => "symlink",
                SkipReason::TooLarge { bytes } => {
                    let _: &Span = span.record("bytes", bytes);
                    "too_large"
                },
            };
            span.record("outcome", "skipped").record("reason", name)
        },
        Ok(ScanEvent::Error(error)) => {
            span.record("outcome", "error")
                .record("kind", field::debug(error.kind))
        },
        Err(error) => {
            span.record("outcome", "error")
                .record("kind", field::debug(error.classify()))
        },
    };
}
