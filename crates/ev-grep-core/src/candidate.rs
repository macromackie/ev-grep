use serde::{Deserialize, Serialize};

use crate::{
    FileError,
    InputError,
    MAX_FILE_BYTES,
    Region,
    Source,
    SourceRead,
    Target,
    label,
    read_target,
};

/// Caller-selected text. A file is an identifier; it is never opened by candidate evaluation.
#[derive(Debug, Deserialize, Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the field order is the order of the candidate's JSON keys"
)]
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
    /// Read a whole source, or its focused lines, as validated supporting evidence.
    #[inline]
    pub fn from_source(source: Source) -> Result<Self, InputError> {
        let file = source.path.clone();
        let candidate = Candidate::from_source(source)?;
        Ok(Self {
            file,
            range: candidate.range,
            text: candidate.text,
        })
    }

    /// Check that the label, text and range are well formed.
    #[inline]
    pub fn validate(&self) -> Result<(), InputError> {
        if self.file.is_empty() || self.file.contains('\0') {
            return Err(InputError::InvalidSupportingFile);
        }
        if self.text.contains('\0') {
            return Err(InputError::SupportingNul);
        }
        self.range.validate(&self.text, None)
    }
}

impl Candidate {
    /// The bytes a request carries for this candidate: its context or text, plus its supporting
    /// text.
    #[inline]
    pub fn count_bytes(&self) -> usize {
        self.supporting.iter().fold(
            self.context.as_ref().map_or(self.text.len(), String::len),
            |bytes, source| bytes.saturating_add(source.text.len()),
        )
    }

    /// Build a validated candidate from a read source, keeping the whole file as context when a
    /// line range narrows it.
    #[inline]
    pub fn from_source(source: Source) -> Result<Self, InputError> {
        let range = if let Some(focus) = &source.focus {
            let start: usize = source
                .text
                .split_inclusive('\n')
                .take(
                    focus
                        .lines
                        .start
                        .checked_sub(1)
                        .ok_or(InputError::InvalidFocus)?,
                )
                .map(str::len)
                .sum();
            Region::from_offsets(
                &source.text,
                start
                    ..start
                        .checked_add(focus.text.len())
                        .ok_or(InputError::PositionOverflow)?,
            )?
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

    /// The candidate's file, or `<stdin>`, with its line range.
    #[inline]
    #[must_use]
    pub fn label(&self) -> String {
        label(
            self.file.as_deref().unwrap_or("<stdin>"),
            Some(self.range.to_lines()),
        )
    }

    /// Check the label, size, NUL bytes, range, metadata and supporting sources against the request
    /// limits.
    #[inline]
    pub fn validate(&self) -> Result<(), InputError> {
        if self
            .file
            .as_ref()
            .is_some_and(|file| file.is_empty() || file.contains('\0'))
        {
            return Err(InputError::InvalidCandidateFile);
        }
        if self.count_bytes() > MAX_FILE_BYTES {
            return Err(InputError::CandidateTooLarge);
        }
        if self.text.contains('\0')
            || self
                .context
                .as_ref()
                .is_some_and(|text| text.contains('\0'))
        {
            return Err(InputError::CandidateNul);
        }
        self.range.validate(&self.text, self.context.as_deref())?;
        if let Some(metadata) = &self.meta_variables {
            let bytes = serde_json::to_vec(metadata).map_err(InputError::EncodeMetadata)?;
            if bytes.len() > MAX_FILE_BYTES {
                return Err(InputError::MetadataTooLarge);
            }
            drop(crate::parse_json(&bytes)?);
        }
        for source in &self.supporting {
            source.validate()?;
        }
        Ok(())
    }
}

/// One unit of scan input: a candidate the caller supplied, a target to read, or an error found
/// while selecting it.
#[derive(Debug)]
pub enum Input {
    Candidate(Candidate),
    Error(FileError),
    File(Target),
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

/// An input after its file was read: a candidate to assess, or a file the scan skips.
#[derive(Debug)]
pub enum Loaded {
    Candidate(Candidate),
    /// A file that is binary or over `MAX_FILE_BYTES`. A scan reports it as skipped and sends no
    /// request for it.
    Skipped {
        target: Target,
        reason: SkipReason,
    },
}

impl Input {
    /// Read a file input and attach `supporting` to the candidate, returning a skip for a binary or
    /// oversized file.
    #[inline]
    pub fn read(self, supporting: &[SupportingSource]) -> Result<Loaded, FileError> {
        let loaded = self.read_target()?;
        if let Loaded::Candidate(mut candidate) = loaded {
            candidate.supporting.extend_from_slice(supporting);
            candidate.validate().map_err(|error| {
                FileError::new(
                    candidate.file.as_deref().unwrap_or("<stdin>"),
                    Some(candidate.range.to_lines()),
                    &error,
                )
            })?;
            Ok(Loaded::Candidate(candidate))
        } else {
            Ok(loaded)
        }
    }

    fn read_target(self) -> Result<Loaded, FileError> {
        match self {
            Self::Error(error) => Err(error),
            Self::Candidate(candidate) => {
                candidate.validate().map_err(|error| {
                    FileError::new(candidate.file.as_deref().unwrap_or("<stdin>"), None, &error)
                })?;
                Ok(Loaded::Candidate(candidate))
            },
            Self::File(target) => {
                let loaded = read_target(&target).and_then(|read| {
                    match read {
                        SourceRead::Binary => Ok(None),
                        SourceRead::Text(source) => Candidate::from_source(source).map(Some),
                    }
                });
                match loaded {
                    Ok(Some(candidate)) => Ok(Loaded::Candidate(candidate)),
                    Ok(None) => {
                        Ok(Loaded::Skipped {
                            target,
                            reason: SkipReason::Binary,
                        })
                    },
                    Err(InputError::SourceTooLarge { bytes }) => {
                        Ok(Loaded::Skipped {
                            target,
                            reason: SkipReason::TooLarge { bytes },
                        })
                    },
                    Err(error) => {
                        Err(FileError::new(
                            &target.path.display().to_string(),
                            target.lines,
                            &error,
                        ))
                    },
                }
            },
        }
    }
}
