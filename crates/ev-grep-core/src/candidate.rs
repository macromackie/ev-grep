use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    FileError, Focus, MAX_FILE_BYTES, Region, Source, SourceRead, Target, label, read_target,
};

/// Caller-selected text. A file is an identifier; it is never opened by candidate evaluation.
#[derive(Debug, Deserialize, Serialize)]
pub struct Candidate {
    pub file: Option<String>,
    pub range: Region,
    pub text: String,
    /// Complete original file, supplied explicitly when the target needs surrounding context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

impl Candidate {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.file
                .as_ref()
                .is_none_or(|file| !file.is_empty() && !file.contains('\0')),
            "invalid candidate file"
        );
        ensure!(
            self.bytes() <= MAX_FILE_BYTES,
            "candidate source exceeds 65536 bytes; input was not truncated"
        );
        ensure!(
            !self.text.contains('\0')
                && self
                    .context
                    .as_ref()
                    .is_none_or(|text| !text.contains('\0')),
            "candidate contains NUL bytes"
        );
        self.range.validate(&self.text, self.context.as_deref())
    }

    pub fn bytes(&self) -> usize {
        self.context.as_ref().map_or(self.text.len(), String::len)
    }

    pub fn from_source(source: Source) -> Result<Self> {
        let range = if let Some(focus) = &source.focus {
            let start: usize = source
                .text
                .split_inclusive('\n')
                .take(
                    focus
                        .lines
                        .start
                        .checked_sub(1)
                        .context("invalid focus start")?,
                )
                .map(str::len)
                .sum();
            Region::from_offsets(&source.text, start..start + focus.text.len())?
        } else {
            Region::from_offsets(&source.text, 0..source.text.len())?
        };
        let (text, context) = match source.focus {
            Some(focus) => (focus.text, Some(source.text)),
            None => (source.text, None),
        };
        let candidate = Self {
            file: Some(source.path),
            range,
            text,
            context,
        };
        candidate.validate()?;
        Ok(candidate)
    }

    pub fn source(&self) -> Source {
        Source {
            path: self.file.clone().unwrap_or_else(|| "<stdin>".into()),
            text: self.context.as_ref().unwrap_or(&self.text).clone(),
            focus: self.context.as_ref().map(|_| Focus {
                lines: self.range.lines(),
                text: self.text.clone(),
            }),
        }
    }

    pub fn label(&self) -> String {
        label(
            self.file.as_deref().unwrap_or("<stdin>"),
            Some(self.range.lines()),
        )
    }
}

pub enum Input {
    File(Target),
    Candidate(Candidate),
    Error(FileError),
}

pub enum Loaded {
    Candidate(Candidate),
    Binary(Target),
}

impl Input {
    pub fn load(self) -> Result<Loaded, FileError> {
        match self {
            Self::Error(error) => Err(error),
            Self::Candidate(candidate) => {
                candidate.validate().map_err(|error| {
                    FileError::new(candidate.file.as_deref().unwrap_or("<stdin>"), None, error)
                })?;
                Ok(Loaded::Candidate(candidate))
            }
            Self::File(target) => {
                let loaded = read_target(&target).and_then(|source| match source {
                    SourceRead::Binary => Ok(None),
                    SourceRead::Text(source) => Candidate::from_source(source).map(Some),
                });
                match loaded {
                    Ok(Some(candidate)) => Ok(Loaded::Candidate(candidate)),
                    Ok(None) => Ok(Loaded::Binary(target)),
                    Err(error) => Err(FileError::new(
                        &target.path.display().to_string(),
                        target.lines,
                        error,
                    )),
                }
            }
        }
    }
}
