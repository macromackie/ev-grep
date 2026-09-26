use std::future::Future;

use anyhow::Result;
use futures_util::{StreamExt, stream};

use crate::{Assessment, FileError, LineRange, Source, SourceRead, Target, read_target};

pub trait Evaluator: Sync {
    fn assess(
        &self,
        query: &str,
        source: &Source,
    ) -> impl Future<Output = Result<Assessment>> + Send;
}

#[derive(Debug)]
pub enum ScanEvent {
    Result {
        path: String,
        lines: Option<LineRange>,
        assessment: Assessment,
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
    mut emit: impl FnMut(ScanEvent) -> Result<()>,
) -> Result<()> {
    let mut pending = stream::iter(targets)
        .map(|target| async move {
            let path = target.path.display().to_string();
            let lines = target.lines;
            let assessment = match read_target(&target) {
                Ok(SourceRead::Binary) => return ScanEvent::Skipped { path, lines },
                Ok(SourceRead::Text(source)) => evaluator.assess(query, &source).await,
                Err(error) => Err(error),
            };
            match assessment {
                Ok(assessment) => ScanEvent::Result {
                    path,
                    lines,
                    assessment,
                },
                Err(error) => ScanEvent::Error(FileError::new(&path, lines, format!("{error:#}"))),
            }
        })
        .buffer_unordered(4);
    while let Some(event) = pending.next().await {
        emit(event)?;
    }
    Ok(())
}
