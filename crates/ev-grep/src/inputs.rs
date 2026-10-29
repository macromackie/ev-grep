use std::{
    io,
    path::{Path, PathBuf},
    pin::Pin,
};

use ev_grep_core::{
    Candidate, FileError, Input, MAX_FILE_BYTES, Region, SourceRead, SupportingSource, Target,
    discover, parse_json, read_source, read_target, read_text,
};
use futures_util::{Stream, stream};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};

use crate::{cli::Cli, error::Error};

const MAX_RECORD_BYTES: usize = 512 * 1024;
type Inputs = Pin<Box<dyn Stream<Item = Input>>>;

#[derive(Serialize)]
pub(crate) struct StateInput {
    pub state: Value,
    pub input_id: String,
    pub bytes: usize,
}

pub(crate) fn state(path: &Path) -> Result<StateInput, Error> {
    let text = if path.as_os_str() == "-" {
        read_text(io::stdin().lock(), MAX_FILE_BYTES)?
    } else {
        let SourceRead::Text(source) = read_source(path).map_err(|source| Error::State {
            path: path.to_path_buf(),
            source,
        })?
        else {
            return Err(Error::BinaryState);
        };
        source.text
    };
    let state = parse_json(text.as_bytes()).map_err(Error::StateJson)?;
    let input_id = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&state).map_err(Error::EncodeState)?)
    );
    Ok(StateInput {
        state,
        input_id,
        bytes: text.len(),
    })
}

/// Select the run's inputs, with the discovery errors and skipped symbolic links to report before them.
pub(crate) async fn select(
    cli: &Cli,
    arguments: &[String],
) -> Result<(Inputs, Vec<FileError>, Vec<String>), Error> {
    if cli.stdin {
        let mut bytes = Vec::new();
        tokio::io::stdin()
            .take((MAX_FILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .await
            .map_err(Error::Stdin)?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(Error::StdinTooLarge);
        }
        let text = String::from_utf8(bytes).map_err(Error::StdinNotUtf8)?;
        let candidate = Candidate {
            file: None,
            range: Region::from_offsets(&text, 0..text.len())?,
            text,
            context: None,
            supporting: Vec::new(),
            meta_variables: None,
        };
        candidate.validate()?;
        return Ok((
            Box::pin(stream::iter([Input::Candidate(candidate)])),
            vec![],
            vec![],
        ));
    }
    if let Some(path) = &cli.candidates {
        let reader: Box<dyn AsyncRead + Unpin> =
            if path.as_os_str() == "-" {
                Box::new(tokio::io::stdin())
            } else {
                Box::new(tokio::fs::File::open(path).await.map_err(|source| {
                    Error::CandidatesFile {
                        path: path.clone(),
                        source,
                    }
                })?)
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
            vec![],
        ));
    }
    let found = discover(arguments, &cli.glob)?;
    // The search writes its trace while it runs, so it never selects that file, as ripgrep never searches its output.
    let trace = cli
        .trace
        .as_deref()
        .filter(|path| path.as_os_str() != "-")
        .and_then(|path| path.canonicalize().ok());
    let targets = found
        .targets
        .into_iter()
        .filter(move |target| trace.is_none() || target.path.canonicalize().ok() != trace);
    Ok((
        Box::pin(stream::iter(targets.map(Input::File))),
        found.errors,
        found.symlinks,
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
                    Err(Error::CandidateLineTooLarge)
                }
                Ok(_) if bytes.iter().all(u8::is_ascii_whitespace) => continue,
                Ok(_) => parse_json(&bytes).map_err(Error::Core).and_then(|value| {
                    let candidate = serde_json::from_value::<Candidate>(value)
                        .map_err(|_| Error::CandidateFields)?;
                    candidate.validate()?;
                    Ok(candidate)
                }),
                Err(error) => {
                    self.done = true;
                    Err(Error::ReadCandidates(error))
                }
            };
            return Some(match parsed {
                Ok(candidate) => Input::Candidate(candidate),
                Err(error) => Input::Error(FileError::new(
                    &format!("{}:{}", self.path.display(), self.line),
                    None,
                    &error,
                )),
            });
        }
        None
    }
}

pub(crate) fn supporting(arguments: &[String]) -> Result<Vec<SupportingSource>, Error> {
    let mut sources = Vec::new();
    let mut bytes = 0usize;
    for argument in arguments {
        let target = Target::parse(argument)?;
        let SourceRead::Text(source) =
            read_target(&target).map_err(|source| Error::Supporting {
                argument: argument.clone(),
                source,
            })?
        else {
            return Err(Error::BinarySupporting {
                argument: argument.clone(),
            });
        };
        let source = SupportingSource::from_source(source)?;
        bytes = bytes.saturating_add(source.text.len());
        if bytes > MAX_FILE_BYTES {
            return Err(Error::SupportingTooLarge);
        }
        sources.push(source);
    }
    Ok(sources)
}
