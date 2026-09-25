use std::{fs::File, io::Read, path::Path};

use anyhow::{Context, Result, ensure};

pub const MAX_FILE_BYTES: usize = 64 * 1024;
pub const MAX_QUERY_BYTES: usize = 8 * 1024;

#[derive(Debug)]
pub struct Source {
    pub path: String,
    pub text: String,
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
    }))
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
