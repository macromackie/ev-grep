use std::{
    fmt,
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};

/// Lines counted from 1, inclusive at both ends.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LineRange {
    pub start: usize,
    pub end: usize,
}

impl fmt::Display for LineRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.start == self.end {
            write!(f, "{}", self.start)
        } else {
            write!(f, "{}-{}", self.start, self.end)
        }
    }
}

/// A path to search, optionally narrowed to lines of one file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Target {
    pub path: PathBuf,
    pub lines: Option<LineRange>,
}

impl Target {
    /// Parses `PATH`, `PATH:N`, or `PATH:N-M`. An argument naming an existing path is never split,
    /// so a file whose name ends in `:12` stays searchable.
    pub fn parse(argument: &str) -> Result<Self> {
        let whole = Self {
            path: PathBuf::from(argument),
            lines: None,
        };
        if Path::new(argument).symlink_metadata().is_ok() {
            return Ok(whole);
        }
        let Some((path, range)) = argument.rsplit_once(':') else {
            return Ok(whole);
        };
        let looks_like_range =
            !range.is_empty() && range.bytes().all(|b| b.is_ascii_digit() || b == b'-');
        if path.is_empty() || !looks_like_range {
            return Ok(whole);
        }
        let line = |text: &str| text.parse::<usize>().ok().filter(|n| *n > 0);
        let (first, last) = range.split_once('-').unwrap_or((range, range));
        match (line(first), line(last)) {
            (Some(start), Some(end)) if start <= end => Ok(Self {
                path: PathBuf::from(path),
                lines: Some(LineRange { start, end }),
            }),
            _ => bail!("invalid line range {range:?}; use N or N-M, counting lines from 1"),
        }
    }
}

/// Formats a location the way ev-grep accepts it: `PATH` or `PATH:N-M`.
pub fn label(path: &str, lines: Option<LineRange>) -> String {
    match lines {
        Some(lines) => format!("{path}:{lines}"),
        None => path.to_owned(),
    }
}
