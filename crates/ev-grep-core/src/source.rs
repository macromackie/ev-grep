use std::{fs::File, io::Read, path::Path};

use crate::{InputError, LineRange, Target, label};

/// The largest file, candidate or request input, in bytes, that a search reads.
pub const MAX_FILE_BYTES: usize = 64 * 1024;
/// The largest query, in bytes.
pub const MAX_QUERY_BYTES: usize = 8 * 1024;
/// `MAX_FILE_BYTES` in the type of file lengths and read limits.
#[expect(
    clippy::as_conversions,
    reason = "64 KiB fits in a u64 on every target"
)]
const FILE_LIMIT: u64 = MAX_FILE_BYTES as u64;
/// A read stops one byte past the limit, so a file that grew past it is still caught.
const READ_LIMIT: u64 = FILE_LIMIT + 1;

/// A file read for a search, with the lines a ranged search focuses on.
#[derive(Debug)]
pub struct Source {
    pub focus: Option<Focus>,
    pub path: String,
    /// The whole file, even when a line range narrows the search; the rest of the file is context.
    pub text: String,
}

/// The lines a ranged search assesses, copied from the file with their line endings.
#[derive(Debug)]
pub struct Focus {
    pub lines: LineRange,
    pub text: String,
}

impl Source {
    /// The file's path, with its focused line range when it has one.
    #[inline]
    #[must_use]
    pub fn label(&self) -> String {
        label(&self.path, self.focus.as_ref().map(|focus| focus.lines))
    }
}

/// A read file: binary, which a scan skips, or UTF-8 text.
#[derive(Debug)]
pub enum SourceRead {
    Binary,
    Text(Source),
}

const fn check_size(bytes: u64) -> Result<(), InputError> {
    if bytes > FILE_LIMIT {
        return Err(InputError::SourceTooLarge { bytes });
    }
    Ok(())
}

/// Read a regular file up to `MAX_FILE_BYTES`, reporting a file with a NUL byte as binary.
#[inline]
pub fn read_source(path: &Path) -> Result<SourceRead, InputError> {
    let metadata = path.symlink_metadata().map_err(InputError::Inspect)?;
    if !metadata.is_file() {
        return Err(InputError::NotRegularFile);
    }
    let mut file = File::open(path).map_err(InputError::Open)?.take(READ_LIMIT);
    let mut prefix = Vec::new();
    let _: usize = file
        .by_ref()
        .take(8192)
        .read_to_end(&mut prefix)
        .map_err(InputError::Read)?;
    if prefix.contains(&0) {
        return Ok(SourceRead::Binary);
    }
    check_size(metadata.len())?;
    let _: usize = file.read_to_end(&mut prefix).map_err(InputError::Read)?;
    if prefix.len() > MAX_FILE_BYTES {
        return Err(InputError::SourceTooLarge { bytes: READ_LIMIT });
    }
    if prefix.contains(&0) {
        return Ok(SourceRead::Binary);
    }
    let text = String::from_utf8(prefix).map_err(InputError::SourceNotUtf8)?;
    Ok(SourceRead::Text(Source {
        focus: None,
        path: path.to_str().ok_or(InputError::PathNotUtf8)?.into(),
        text,
    }))
}

/// Read a target's file, and copy out its line range as the focus when it names one.
#[inline]
pub fn read_target(target: &Target) -> Result<SourceRead, InputError> {
    let SourceRead::Text(mut source) = read_source(&target.path)? else {
        return Ok(SourceRead::Binary);
    };
    if let Some(lines) = target.lines {
        let text = extract_excerpt(&source.text, lines)?;
        source.focus = Some(Focus { lines, text });
    }
    Ok(SourceRead::Text(source))
}

/// Each line ends at `\n`, and a final line without one still counts. Ranges past the end are
/// errors.
fn extract_excerpt(text: &str, lines: LineRange) -> Result<String, InputError> {
    let all: Vec<&str> = text.split_inclusive('\n').collect();
    let selected = lines
        .start
        .checked_sub(1)
        .and_then(|first| all.get(first..lines.end));
    match selected {
        Some(found) => Ok(found.concat()),
        None => {
            Err(InputError::LinesOutsideFile {
                lines,
                total: all.len(),
            })
        },
    }
}

/// Read non-empty UTF-8 text of at most `limit` bytes from `reader`.
#[inline]
pub fn read_text<Reader>(reader: Reader, limit: usize) -> Result<String, InputError>
where
    Reader: Read,
{
    let mut bytes = Vec::new();
    let _: usize = reader
        .take(u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(InputError::ReadText)?;
    if bytes.len() > limit {
        return Err(InputError::TextTooLarge { limit });
    }
    let text = String::from_utf8(bytes).map_err(InputError::TextNotUtf8)?;
    if text.trim().is_empty() {
        return Err(InputError::EmptyText);
    }
    Ok(text)
}
