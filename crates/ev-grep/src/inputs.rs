use std::{path::PathBuf, pin::Pin};

use anyhow::{Context, Result, ensure};
use ev_grep_core::{Candidate, FileError, Input, MAX_FILE_BYTES, Region, discover};
use futures_util::{Stream, stream};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};

use crate::cli::Cli;

const MAX_RECORD_BYTES: usize = 512 * 1024;
type Inputs = Pin<Box<dyn Stream<Item = Input>>>;

pub(crate) async fn select(cli: &Cli, arguments: &[String]) -> Result<(Inputs, Vec<FileError>)> {
    if cli.stdin {
        let mut bytes = Vec::new();
        tokio::io::stdin()
            .take((MAX_FILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .await?;
        ensure!(
            bytes.len() <= MAX_FILE_BYTES,
            "stdin exceeds 65536 bytes; input was not truncated"
        );
        let text = String::from_utf8(bytes).context("stdin is not valid UTF-8")?;
        let candidate = Candidate {
            file: None,
            range: Region::from_offsets(&text, 0..text.len())?,
            text,
            context: None,
        };
        candidate.validate()?;
        return Ok((
            Box::pin(stream::iter([Input::Candidate(candidate)])),
            vec![],
        ));
    }
    if let Some(path) = &cli.candidates {
        let reader: Box<dyn AsyncRead + Unpin> = if path.as_os_str() == "-" {
            Box::new(tokio::io::stdin())
        } else {
            Box::new(
                tokio::fs::File::open(path)
                    .await
                    .with_context(|| format!("cannot read candidates {}", path.display()))?,
            )
        };
        let input = CandidateLines {
            reader: BufReader::new(reader),
            path: path.clone(),
            line: 0,
            done: false,
        };
        return Ok((
            Box::pin(stream::unfold(input, |mut input| async {
                input.next().await.map(|item| (item, input))
            })),
            vec![],
        ));
    }
    let found = discover(arguments, &cli.glob)?;
    Ok((
        Box::pin(stream::iter(found.targets.into_iter().map(Input::File))),
        found.errors,
    ))
}

struct CandidateLines<R> {
    reader: R,
    path: PathBuf,
    line: usize,
    done: bool,
}

impl<R: tokio::io::AsyncBufRead + Unpin> CandidateLines<R> {
    async fn next(&mut self) -> Option<Input> {
        while !self.done {
            let mut bytes = Vec::new();
            let read = (&mut self.reader)
                .take((MAX_RECORD_BYTES + 1) as u64)
                .read_until(b'\n', &mut bytes)
                .await;
            self.line += 1;
            let parsed = match read {
                Ok(0) => {
                    self.done = true;
                    return None;
                }
                Ok(_) if bytes.len() > MAX_RECORD_BYTES => {
                    self.done = true;
                    Err(anyhow::anyhow!(
                        "candidate JSON line exceeds 524288 bytes; remaining input was not read"
                    ))
                }
                Ok(_) if bytes.iter().all(u8::is_ascii_whitespace) => continue,
                Ok(_) => serde_json::from_slice::<Candidate>(&bytes)
                    .map_err(|_| {
                        anyhow::anyhow!("expected candidate JSON with file, range, and text")
                    })
                    .and_then(|candidate| {
                        candidate.validate()?;
                        Ok(candidate)
                    }),
                Err(error) => {
                    self.done = true;
                    Err(anyhow::Error::from(error).context("cannot read candidates"))
                }
            };
            return Some(match parsed {
                Ok(candidate) => Input::Candidate(candidate),
                Err(error) => Input::Error(FileError::new(
                    &format!("{}:{}", self.path.display(), self.line),
                    None,
                    error,
                )),
            });
        }
        None
    }
}
