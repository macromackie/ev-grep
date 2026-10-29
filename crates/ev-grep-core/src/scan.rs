use futures_util::{FutureExt as _, Stream, StreamExt as _};
use serde::Serialize;
use tracing::Instrument as _;

use crate::{
    Assessment,
    Candidate,
    Failure,
    FileError,
    Input,
    InputError,
    LineRange,
    Loaded,
    SupportingSource,
    telemetry,
};

/// A model backend that answers questions about candidates and labeled context.
pub trait Evaluator: Sync {
    /// Why an assessment produced no answer. The core's validation errors convert into it, and its
    /// kind classifies the failed file in a scan.
    type Error: Failure + From<InputError> + Send + Sync + 'static;

    /// Assess whether `candidate` matches `query`.
    fn assess(
        &self,
        query: &str,
        candidate: &Candidate,
    ) -> impl Future<Output = Result<Assessment, Self::Error>> + Send;

    /// Assess labeled context without assuming it is one source file.
    #[inline]
    fn assess_context(
        &self,
        query: &str,
        state: &serde_json::Value,
    ) -> impl Future<Output = Result<Assessment, Self::Error>> + Send {
        async move {
            Ok(self
                .assess_questions(&[crate::Question { id: "match", query }], state)
                .await?
                .into_single()?)
        }
    }

    /// Answer several named questions about labeled `state` in one request.
    fn assess_questions(
        &self,
        questions: &[crate::Question<'_>],
        state: &serde_json::Value,
    ) -> impl Future<Output = Result<crate::DecisionBatch, Self::Error>> + Send;
}

/// Why a selected file was not assessed. JSON skip records name it as `reason`, with the variant's
/// fields beside it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum SkipReason {
    /// The file contains a NUL byte.
    Binary,
    /// A directory search found a symbolic link, which it does not follow.
    Symlink,
    /// The file exceeds `MAX_FILE_BYTES`, so no request could carry it whole.
    TooLarge { bytes: u64 },
}

/// One per-input outcome that a scan emits.
#[derive(Debug)]
pub enum ScanEvent {
    Error(FileError),
    Result {
        assessment: Box<Assessment>,
        candidate: Candidate,
    },
    Skipped {
        lines: Option<LineRange>,
        path: String,
        reason: SkipReason,
    },
}

/// Assesses each input in a bounded pool that all input modes share.
///
/// The scan stops at the first error that `emit` returns, and at the first evaluator error whose
/// kind is fatal, which it returns instead of reporting a failed file. Stopping drops the
/// assessments still in flight. Each input runs in an `ev_grep.file` tracing span.
#[inline]
pub async fn scan_inputs<Inputs, Assessor, Emit, EmitError>(
    inputs: Inputs,
    query: &str,
    supporting: &[SupportingSource],
    evaluator: &Assessor,
    jobs: usize,
    mut emit: Emit,
) -> Result<(), EmitError>
where
    Inputs: Stream<Item = Input>,
    Assessor: Evaluator,
    Emit: FnMut(ScanEvent) -> Result<(), EmitError>,
    EmitError: From<InputError> + From<Assessor::Error>,
{
    if !(1..=256).contains(&jobs) {
        return Err(InputError::Jobs.into());
    }
    let pending = inputs
        .map(|input| {
            let span = telemetry::create_file_span(&input);
            async move {
                let candidate = match input.read(supporting) {
                    Ok(Loaded::Candidate(candidate)) => candidate,
                    Ok(Loaded::Skipped { target, reason }) => {
                        return Ok(ScanEvent::Skipped {
                            path: target.path.display().to_string(),
                            lines: target.lines,
                            reason,
                        });
                    },
                    Err(error) => return Ok(ScanEvent::Error(error)),
                };
                telemetry::record_loaded(&candidate);
                match evaluator.assess(query, &candidate).await {
                    Ok(assessment) => {
                        Ok(ScanEvent::Result {
                            candidate,
                            assessment: Box::new(assessment),
                        })
                    },
                    Err(error) if error.classify().is_fatal() => Err(error),
                    Err(error) => {
                        Ok(ScanEvent::Error(FileError::new(
                            candidate.file.as_deref().unwrap_or("<stdin>"),
                            Some(candidate.range.to_lines()),
                            &error,
                        )))
                    },
                }
            }
            .inspect(telemetry::record_file_finished)
            .instrument(span)
        })
        .buffer_unordered(jobs);
    futures_util::pin_mut!(pending);
    while let Some(event) = pending.next().await {
        emit(event?)?;
    }
    Ok(())
}
