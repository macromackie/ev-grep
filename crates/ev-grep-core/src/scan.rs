use std::{future::Future, path::PathBuf};

use anyhow::Result;
use futures_util::{StreamExt, stream};

use crate::{Assessment, FileError, Source, SourceRead, read_source};

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
        assessment: Assessment,
    },
    Skipped {
        path: String,
    },
    Error(FileError),
}

/// At most four files are read and evaluated concurrently; output is emitted as work completes.
pub async fn scan(
    paths: Vec<PathBuf>,
    query: &str,
    evaluator: &impl Evaluator,
    mut emit: impl FnMut(ScanEvent) -> Result<()>,
) -> Result<()> {
    let mut pending = stream::iter(paths)
        .map(|path| async move {
            let label = path.display().to_string();
            let assessment = match read_source(&path) {
                Ok(SourceRead::Binary) => return ScanEvent::Skipped { path: label },
                Ok(SourceRead::Text(source)) => evaluator.assess(query, &source).await,
                Err(error) => Err(error),
            };
            match assessment {
                Ok(assessment) => ScanEvent::Result {
                    path: label,
                    assessment,
                },
                Err(error) => ScanEvent::Error(FileError {
                    path: label,
                    message: format!("{error:#}"),
                }),
            }
        })
        .buffer_unordered(4);
    while let Some(event) = pending.next().await {
        emit(event)?;
    }
    Ok(())
}
