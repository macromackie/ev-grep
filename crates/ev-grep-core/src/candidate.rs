use serde::{Deserialize, Serialize};

use crate::{
    Error, FileError, MAX_FILE_BYTES, Region, SkipReason, Source, SourceRead, Target, label,
    read_target,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supporting: Vec<SupportingSource>,
    /// Caller provenance retained for composition; never included in search model input.
    #[serde(rename = "metaVariables", skip_serializing_if = "Option::is_none")]
    pub meta_variables: Option<serde_json::Value>,
}

/// Explicit dependency evidence. The file is a label and is never read during assessment.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SupportingSource {
    pub file: String,
    pub range: Region,
    pub text: String,
}

impl SupportingSource {
    pub fn validate(&self) -> Result<(), Error> {
        if self.file.is_empty() || self.file.contains('\0') {
            return Err(Error::InvalidSupportingFile);
        }
        if self.text.contains('\0') {
            return Err(Error::SupportingNul);
        }
        self.range.validate(&self.text, None)
    }

    pub fn from_source(source: Source) -> Result<Self, Error> {
        let file = source.path.clone();
        let candidate = Candidate::from_source(source)?;
        Ok(Self {
            file,
            range: candidate.range,
            text: candidate.text,
        })
    }
}

impl Candidate {
    pub fn validate(&self) -> Result<(), Error> {
        if self
            .file
            .as_ref()
            .is_some_and(|file| file.is_empty() || file.contains('\0'))
        {
            return Err(Error::InvalidCandidateFile);
        }
        if self.bytes() > MAX_FILE_BYTES {
            return Err(Error::CandidateTooLarge);
        }
        if self.text.contains('\0')
            || self
                .context
                .as_ref()
                .is_some_and(|text| text.contains('\0'))
        {
            return Err(Error::CandidateNul);
        }
        self.range.validate(&self.text, self.context.as_deref())?;
        if let Some(metadata) = &self.meta_variables {
            let bytes = serde_json::to_vec(metadata).map_err(Error::EncodeMetadata)?;
            if bytes.len() > MAX_FILE_BYTES {
                return Err(Error::MetadataTooLarge);
            }
            crate::parse_json(&bytes)?;
        }
        for source in &self.supporting {
            source.validate()?;
        }
        Ok(())
    }

    pub fn bytes(&self) -> usize {
        self.supporting.iter().fold(
            self.context.as_ref().map_or(self.text.len(), String::len),
            |bytes, source| bytes.saturating_add(source.text.len()),
        )
    }

    pub fn from_source(source: Source) -> Result<Self, Error> {
        let range = if let Some(focus) = &source.focus {
            let start: usize = source
                .text
                .split_inclusive('\n')
                .take(
                    focus
                        .lines
                        .start
                        .checked_sub(1)
                        .ok_or(Error::InvalidFocus)?,
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
            supporting: Vec::new(),
            meta_variables: None,
        };
        candidate.validate()?;
        Ok(candidate)
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
    /// A file that is binary or over `MAX_FILE_BYTES`. A scan reports it as skipped and sends no request for it.
    Skipped {
        target: Target,
        reason: SkipReason,
    },
}

impl Input {
    pub fn load(self, supporting: &[SupportingSource]) -> Result<Loaded, FileError> {
        let loaded = self.load_target()?;
        if let Loaded::Candidate(mut candidate) = loaded {
            candidate.supporting.extend_from_slice(supporting);
            candidate.validate().map_err(|error| {
                FileError::new(
                    candidate.file.as_deref().unwrap_or("<stdin>"),
                    Some(candidate.range.lines()),
                    &error,
                )
            })?;
            Ok(Loaded::Candidate(candidate))
        } else {
            Ok(loaded)
        }
    }

    fn load_target(self) -> Result<Loaded, FileError> {
        match self {
            Self::Error(error) => Err(error),
            Self::Candidate(candidate) => {
                candidate.validate().map_err(|error| {
                    FileError::new(candidate.file.as_deref().unwrap_or("<stdin>"), None, &error)
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
                    Ok(None) => Ok(Loaded::Skipped {
                        target,
                        reason: SkipReason::Binary,
                    }),
                    Err(Error::SourceTooLarge { bytes }) => Ok(Loaded::Skipped {
                        target,
                        reason: SkipReason::TooLarge { bytes },
                    }),
                    Err(error) => Err(FileError::new(
                        &target.path.display().to_string(),
                        target.lines,
                        &error,
                    )),
                }
            }
        }
    }
}
