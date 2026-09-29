use std::future::Future;

use anyhow::{Result, ensure};
use futures_util::{Stream, StreamExt, stream};

use crate::{Assessment, Candidate, FileError, Input, LineRange, Loaded, Source, Target};

pub trait Evaluator: Sync {
    fn assess(
        &self,
        query: &str,
        source: &Source,
    ) -> impl Future<Output = Result<Assessment>> + Send;

    fn assess_questions(
        &self,
        questions: &[crate::Question<'_>],
        state: &serde_json::Value,
    ) -> impl Future<Output = Result<crate::DecisionBatch>> + Send;

    /// Assess labeled context without assuming it is one source file.
    fn assess_context(
        &self,
        query: &str,
        state: &serde_json::Value,
    ) -> impl Future<Output = Result<Assessment>> + Send {
        async move {
            let source = Source {
                path: "<context>".into(),
                text: serde_json::to_string(state)?,
                focus: None,
            };
            Ok(self.assess(query, &source).await?.with_min_confidence(0.0))
        }
    }
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
    },
    Error(FileError),
}

/// At most four targets are read and evaluated concurrently; output is emitted as work completes.
pub async fn scan(
    targets: Vec<Target>,
    query: &str,
    evaluator: &impl Evaluator,
    emit: impl FnMut(ScanEvent) -> Result<()>,
) -> Result<()> {
    scan_with_jobs(targets, query, evaluator, 4, emit).await
}

/// Scan with a bounded pool of 1 to 256 concurrent assessments.
pub async fn scan_with_jobs(
    targets: Vec<Target>,
    query: &str,
    evaluator: &impl Evaluator,
    jobs: usize,
    emit: impl FnMut(ScanEvent) -> Result<()>,
) -> Result<()> {
    scan_inputs(
        stream::iter(targets.into_iter().map(Input::File)),
        query,
        evaluator,
        jobs,
        emit,
    )
    .await
}

/// All input modes share this bounded assessment pool.
pub async fn scan_inputs(
    inputs: impl Stream<Item = Input>,
    query: &str,
    evaluator: &impl Evaluator,
    jobs: usize,
    mut emit: impl FnMut(ScanEvent) -> Result<()>,
) -> Result<()> {
    ensure!((1..=256).contains(&jobs), "jobs must be between 1 and 256");
    let pending = inputs
        .map(|input| async move {
            let candidate = match input.load() {
                Ok(Loaded::Candidate(candidate)) => candidate,
                Ok(Loaded::Binary(target)) => {
                    return ScanEvent::Skipped {
                        path: target.path.display().to_string(),
                        lines: target.lines,
                    };
                }
                Err(error) => return ScanEvent::Error(error),
            };
            match evaluator.assess(query, &candidate.source()).await {
                Ok(assessment) => ScanEvent::Result {
                    candidate,
                    assessment: Box::new(assessment),
                },
                Err(error) => ScanEvent::Error(FileError::new(
                    candidate.file.as_deref().unwrap_or("<stdin>"),
                    Some(candidate.range.lines()),
                    format!("{error:#}"),
                )),
            }
        })
        .buffer_unordered(jobs);
    futures_util::pin_mut!(pending);
    while let Some(event) = pending.next().await {
        emit(event)?;
    }
    Ok(())
}
