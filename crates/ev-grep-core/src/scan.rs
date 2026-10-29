use std::future::Future;

use futures_util::{FutureExt, Stream, StreamExt};
use serde::Serialize;
use tracing::Instrument;

use crate::{
    Assessment, Candidate, Error, Failure, FileError, Input, LineRange, Loaded, SupportingSource,
    telemetry,
};

pub trait Evaluator: Sync {
    /// Why an assessment produced no answer. The core's validation errors convert into it, and its kind classifies
    /// the failed file in a scan.
    type Error: Failure + From<Error> + Send + Sync + 'static;

    fn assess(
        &self,
        query: &str,
        candidate: &Candidate,
    ) -> impl Future<Output = Result<Assessment, Self::Error>> + Send;

    fn assess_questions(
        &self,
        questions: &[crate::Question<'_>],
        state: &serde_json::Value,
    ) -> impl Future<Output = Result<crate::DecisionBatch, Self::Error>> + Send;

    /// Assess labeled context without assuming it is one source file.
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
}

/// Why a selected file was not assessed. JSON skip records name it as `reason`, with the variant's fields beside it.
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

#[derive(Debug)]
pub enum ScanEvent {
    Result {
        candidate: Candidate,
        assessment: Box<Assessment>,
    },
    Skipped {
        path: String,
        lines: Option<LineRange>,
        reason: SkipReason,
    },
    Error(FileError),
}

/// All input modes share this bounded assessment pool. The scan stops at the first error that `emit` returns, and at
/// the first evaluator error whose kind is fatal, which it returns instead of reporting a failed file. Stopping drops
/// the assessments still in flight. Each input runs in an `ev_grep.file` tracing span.
pub async fn scan_inputs<E: Evaluator, EmitError: From<Error> + From<E::Error>>(
    inputs: impl Stream<Item = Input>,
    query: &str,
    supporting: &[SupportingSource],
    evaluator: &E,
    jobs: usize,
    mut emit: impl FnMut(ScanEvent) -> Result<(), EmitError>,
) -> Result<(), EmitError> {
    if !(1..=256).contains(&jobs) {
        return Err(Error::Jobs.into());
    }
    let pending = inputs
        .map(|input| {
            let span = telemetry::file(&input);
            async move {
                let candidate = match input.load(supporting) {
                    Ok(Loaded::Candidate(candidate)) => candidate,
                    Ok(Loaded::Skipped { target, reason }) => {
                        return Ok(ScanEvent::Skipped {
                            path: target.path.display().to_string(),
                            lines: target.lines,
                            reason,
                        });
                    }
                    Err(error) => return Ok(ScanEvent::Error(error)),
                };
                telemetry::loaded(&candidate);
                match evaluator.assess(query, &candidate).await {
                    Ok(assessment) => Ok(ScanEvent::Result {
                        candidate,
                        assessment: Box::new(assessment),
                    }),
                    Err(error) if error.kind().fatal() => Err(error),
                    Err(error) => Ok(ScanEvent::Error(FileError::new(
                        candidate.file.as_deref().unwrap_or("<stdin>"),
                        Some(candidate.range.lines()),
                        &error,
                    ))),
                }
            }
            .inspect(telemetry::file_finished)
            .instrument(span)
        })
        .buffer_unordered(jobs);
    futures_util::pin_mut!(pending);
    while let Some(event) = pending.next().await {
        emit(event?)?;
    }
    Ok(())
}
