use core::pin::Pin;
use std::{
    io,
    path::{Path, PathBuf},
};

use ev_grep_core::{
    Candidate,
    FileError,
    Input,
    MAX_FILE_BYTES,
    Region,
    SourceRead,
    SupportingSource,
    Target,
    discover,
    parse_json,
    read_source,
    read_target,
    read_text,
};
use futures_util::{Stream, StreamExt as _, stream};
use serde::Serialize;
use serde_json::{Value, from_value};
use sha2::{Digest as _, Sha256};
use tokio::{
    fs::File,
    io::{AsyncBufRead, AsyncBufReadExt as _, AsyncRead, AsyncReadExt as _, BufReader, stdin},
};

use crate::{cli::Cli, error::CliError};

const MAX_RECORD_BYTES: usize = 512 * 1024;
pub(crate) type Inputs = Pin<Box<dyn Stream<Item = Input>>>;

#[derive(Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the field order is the order of the record's JSON keys"
)]
pub(crate) struct StateInput {
    pub state: Value,
    pub input_id: String,
    pub bytes: usize,
}

pub(crate) fn read_state(path: &Path) -> Result<StateInput, CliError> {
    let text = if path.as_os_str() == "-" {
        read_text(io::stdin().lock(), MAX_FILE_BYTES)?
    } else {
        let SourceRead::Text(source) = read_source(path).map_err(|source| {
            CliError::State {
                path: path.to_path_buf(),
                source,
            }
        })?
        else {
            return Err(CliError::BinaryState);
        };
        source.text
    };
    let state = parse_json(text.as_bytes()).map_err(CliError::StateJson)?;
    let input_id = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&state).map_err(CliError::EncodeState)?)
    );
    Ok(StateInput {
        state,
        input_id,
        bytes: text.len(),
    })
}

/// Select the run's inputs, with the discovery errors and skipped symbolic links to report before
/// them.
pub(crate) async fn select(
    cli: &Cli,
    arguments: &[String],
) -> Result<(Inputs, Vec<FileError>, Vec<String>), CliError> {
    if cli.stdin {
        let candidate = read_stdin_candidate().await?;
        return Ok((
            Box::pin(stream::iter([Input::Candidate(candidate)])),
            vec![],
            vec![],
        ));
    }
    if let Some(path) = &cli.candidates {
        let reader: Box<dyn AsyncRead + Unpin> = if path.as_os_str() == "-" {
            Box::new(stdin())
        } else {
            Box::new(File::open(path).await.map_err(|source| {
                CliError::CandidatesFile {
                    path: path.clone(),
                    source,
                }
            })?)
        };
        let input = CandidateLines {
            done: false,
            line: 0,
            path: path.clone(),
            reader: BufReader::new(reader),
        };
        return Ok((
            Box::pin(stream::unfold(input, async |mut lines| {
                lines.read_next().await.map(|item| (item, lines))
            })),
            vec![],
            vec![],
        ));
    }
    let found = discover(arguments, &cli.glob)?;
    // The search writes its trace while it runs, so it never selects that file, as ripgrep never
    // searches its output.
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

/// Fail before any request when the selection holds more than `limit` inputs.
#[expect(
    clippy::future_not_send,
    reason = "the runtime is current_thread, so the inputs need not be Send"
)]
pub(crate) async fn limit(mut inputs: Inputs, limit: Option<usize>) -> Result<Inputs, CliError> {
    let Some(maximum) = limit else {
        return Ok(inputs);
    };
    let selected: Vec<Input> = inputs
        .by_ref()
        .take(maximum.saturating_add(1))
        .collect()
        .await;
    if selected.len() > maximum {
        return Err(CliError::TooManyCandidates { limit: maximum });
    }
    Ok(Box::pin(stream::iter(selected)))
}

/// Read stdin as one text candidate.
async fn read_stdin_candidate() -> Result<Candidate, CliError> {
    let mut bytes = Vec::new();
    let _: usize = stdin()
        .take(
            u64::try_from(MAX_FILE_BYTES)
                .unwrap_or(u64::MAX)
                .saturating_add(1),
        )
        .read_to_end(&mut bytes)
        .await
        .map_err(CliError::Stdin)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(CliError::StdinTooLarge);
    }
    let text = String::from_utf8(bytes).map_err(CliError::StdinNotUtf8)?;
    let candidate = Candidate {
        file: None,
        range: Region::from_offsets(&text, 0..text.len())?,
        text,
        context: None,
        supporting: Vec::new(),
        meta_variables: None,
    };
    candidate.validate()?;
    Ok(candidate)
}

/// Parse one candidate JSON line.
fn parse_candidate(bytes: &[u8]) -> Result<Candidate, CliError> {
    let value = parse_json(bytes)?;
    let candidate = from_value::<Candidate>(value).map_err(CliError::CandidateFields)?;
    candidate.validate()?;
    Ok(candidate)
}

struct CandidateLines<Reader> {
    done: bool,
    line: usize,
    path: PathBuf,
    reader: Reader,
}

impl<Reader> CandidateLines<Reader>
where
    Reader: AsyncBufRead + Unpin,
{
    async fn read_next(&mut self) -> Option<Input> {
        while !self.done {
            let mut bytes = Vec::new();
            let read = (&mut self.reader)
                .take(
                    u64::try_from(MAX_RECORD_BYTES)
                        .unwrap_or(u64::MAX)
                        .saturating_add(1),
                )
                .read_until(b'\n', &mut bytes)
                .await;
            self.line = self.line.saturating_add(1);
            let parsed = match read {
                Ok(0) => {
                    self.done = true;
                    return None;
                },
                Ok(_) if bytes.len() > MAX_RECORD_BYTES => {
                    self.done = true;
                    Err(CliError::CandidateLineTooLarge)
                },
                Ok(_) if bytes.iter().all(u8::is_ascii_whitespace) => continue,
                Ok(_) => parse_candidate(&bytes),
                Err(error) => {
                    self.done = true;
                    Err(CliError::ReadCandidates(error))
                },
            };
            return Some(match parsed {
                Ok(candidate) => Input::Candidate(candidate),
                Err(error) => {
                    Input::Error(FileError::new(
                        &format!("{}:{}", self.path.display(), self.line),
                        None,
                        &error,
                    ))
                },
            });
        }
        None
    }
}

pub(crate) fn read_supporting(arguments: &[String]) -> Result<Vec<SupportingSource>, CliError> {
    let mut sources = Vec::new();
    let mut bytes = 0_usize;
    for argument in arguments {
        let target = Target::parse(argument)?;
        let SourceRead::Text(source) = read_target(&target).map_err(|source| {
            CliError::Supporting {
                argument: argument.clone(),
                source,
            }
        })?
        else {
            return Err(CliError::BinarySupporting {
                argument: argument.clone(),
            });
        };
        let supporting = SupportingSource::from_source(source)?;
        bytes = bytes.saturating_add(supporting.text.len());
        if bytes > MAX_FILE_BYTES {
            return Err(CliError::SupportingTooLarge);
        }
        sources.push(supporting);
    }
    Ok(sources)
}
