use std::{fs::File, io::Read, path::Path};

use crate::{Error, LineRange, Target, label};

pub const MAX_FILE_BYTES: usize = 64 * 1024;
pub const MAX_QUERY_BYTES: usize = 8 * 1024;

#[derive(Debug)]
pub struct Source {
    pub path: String,
    /// The whole file, even when a line range narrows the search; the rest of the file is context.
    pub text: String,
    pub focus: Option<Focus>,
}

/// The lines a ranged search assesses, copied from the file with their line endings.
#[derive(Debug)]
pub struct Focus {
    pub lines: LineRange,
    pub text: String,
}

impl Source {
    pub fn label(&self) -> String {
        label(&self.path, self.focus.as_ref().map(|focus| focus.lines))
    }
}

#[derive(Debug)]
pub enum SourceRead {
    Text(Source),
    Binary,
}

fn check_size(bytes: u64) -> Result<(), Error> {
    if bytes > MAX_FILE_BYTES as u64 {
        return Err(Error::SourceTooLarge { bytes });
    }
    Ok(())
}

pub fn read_source(path: &Path) -> Result<SourceRead, Error> {
    let metadata = path.symlink_metadata().map_err(Error::Inspect)?;
    if !metadata.is_file() {
        return Err(Error::NotRegularFile);
    }
    let mut file = File::open(path).map_err(Error::Open)?;
    let mut prefix = Vec::new();
    file.by_ref()
        .take(8192)
        .read_to_end(&mut prefix)
        .map_err(Error::Read)?;
    if prefix.contains(&0) {
        return Ok(SourceRead::Binary);
    }
    check_size(metadata.len())?;
    file.take((MAX_FILE_BYTES + 1 - prefix.len()) as u64)
        .read_to_end(&mut prefix)
        .map_err(Error::Read)?;
    check_size(prefix.len() as u64)?;
    if prefix.contains(&0) {
        return Ok(SourceRead::Binary);
    }
    let text = String::from_utf8(prefix).map_err(Error::SourceNotUtf8)?;
    Ok(SourceRead::Text(Source {
        path: path.to_str().ok_or(Error::PathNotUtf8)?.into(),
        text,
        focus: None,
    }))
}

pub fn read_target(target: &Target) -> Result<SourceRead, Error> {
    let SourceRead::Text(mut source) = read_source(&target.path)? else {
        return Ok(SourceRead::Binary);
    };
    if let Some(lines) = target.lines {
        let text = excerpt(&source.text, lines)?;
        source.focus = Some(Focus { lines, text });
    }
    Ok(SourceRead::Text(source))
}

/// Each line ends at `\n`, and a final line without one still counts. Ranges past the end are errors.
fn excerpt(text: &str, lines: LineRange) -> Result<String, Error> {
    let all: Vec<&str> = text.split_inclusive('\n').collect();
    let selected = lines
        .start
        .checked_sub(1)
        .and_then(|first| all.get(first..lines.end));
    match selected {
        Some(selected) => Ok(selected.concat()),
        None => Err(Error::LinesOutsideFile {
            lines,
            total: all.len(),
        }),
    }
}

pub fn read_text(reader: impl Read, limit: usize) -> Result<String, Error> {
    let mut bytes = Vec::new();
    reader
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(Error::ReadText)?;
    if bytes.len() > limit {
        return Err(Error::TextTooLarge { limit });
    }
    let text = String::from_utf8(bytes).map_err(Error::TextNotUtf8)?;
    if text.trim().is_empty() {
        return Err(Error::EmptyText);
    }
    Ok(text)
}
