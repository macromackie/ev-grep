use std::{fs::File, io::Read, path::Path};

use anyhow::{Context, Result, ensure};

use crate::{LineRange, Target, label};

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

pub fn read_source(path: &Path) -> Result<SourceRead> {
    let metadata = path.symlink_metadata().context("cannot inspect file")?;
    ensure!(
        metadata.is_file(),
        "expected a regular file; symbolic links are not followed"
    );
    let mut file = File::open(path).context("cannot open file")?;
    let mut prefix = Vec::new();
    file.by_ref()
        .take(8192)
        .read_to_end(&mut prefix)
        .context("cannot read file")?;
    if prefix.contains(&0) {
        return Ok(SourceRead::Binary);
    }
    ensure!(
        metadata.len() <= MAX_FILE_BYTES as u64,
        "file exceeds 65536 bytes; source was not truncated"
    );
    file.take((MAX_FILE_BYTES + 1 - prefix.len()) as u64)
        .read_to_end(&mut prefix)?;
    ensure!(
        prefix.len() <= MAX_FILE_BYTES,
        "file exceeds 65536 bytes; source was not truncated"
    );
    if prefix.contains(&0) {
        return Ok(SourceRead::Binary);
    }
    let text = String::from_utf8(prefix).context("source is not valid UTF-8")?;
    Ok(SourceRead::Text(Source {
        path: path
            .to_str()
            .context("file path is not valid UTF-8")?
            .into(),
        text,
        focus: None,
    }))
}

pub fn read_target(target: &Target) -> Result<SourceRead> {
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
fn excerpt(text: &str, lines: LineRange) -> Result<String> {
    let all: Vec<&str> = text.split_inclusive('\n').collect();
    let selected = lines
        .start
        .checked_sub(1)
        .and_then(|first| all.get(first..lines.end));
    match selected {
        Some(selected) => Ok(selected.concat()),
        None => anyhow::bail!(
            "line range {lines} is outside the file, which has {} lines",
            all.len()
        ),
    }
}

pub fn read_text(reader: impl Read, limit: usize) -> Result<String> {
    let mut bytes = Vec::new();
    reader.take((limit + 1) as u64).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= limit,
        "query exceeds {limit} bytes; query was not truncated"
    );
    let text = String::from_utf8(bytes).context("query is not valid UTF-8")?;
    ensure!(!text.trim().is_empty(), "query must not be empty");
    Ok(text)
}
