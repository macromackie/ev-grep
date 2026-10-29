use core::fmt;
use std::path::{Path, PathBuf};

use crate::InputError;

/// Lines counted from 1, inclusive at both ends.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the derived order compares the start line first"
)]
pub struct LineRange {
    pub start: usize,
    pub end: usize,
}

impl fmt::Display for LineRange {
    #[inline]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.start == self.end {
            write!(formatter, "{}", self.start)
        } else {
            write!(formatter, "{}-{}", self.start, self.end)
        }
    }
}

/// A path to search, optionally narrowed to lines of one file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Target {
    pub lines: Option<LineRange>,
    pub path: PathBuf,
}

impl Target {
    /// Parses `PATH`, `PATH:N`, or `PATH:N-M`. An argument naming an existing path is never split,
    /// so a file whose name ends in `:12` stays searchable.
    #[inline]
    pub fn parse(argument: &str) -> Result<Self, InputError> {
        let whole = Self {
            lines: None,
            path: PathBuf::from(argument),
        };
        if Path::new(argument).symlink_metadata().is_ok() {
            return Ok(whole);
        }
        let Some((path, range)) = argument.rsplit_once(':') else {
            return Ok(whole);
        };
        let looks_like_range = !range.is_empty()
            && range
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'-');
        if path.is_empty() || !looks_like_range {
            return Ok(whole);
        }
        let line = |text: &str| text.parse::<usize>().ok().filter(|number| *number > 0);
        let (first, last) = range.split_once('-').unwrap_or((range, range));
        match (line(first), line(last)) {
            (Some(start), Some(end)) if start <= end => {
                Ok(Self {
                    lines: Some(LineRange { start, end }),
                    path: PathBuf::from(path),
                })
            },
            _ => {
                Err(InputError::InvalidLineRange {
                    range: range.to_owned(),
                })
            },
        }
    }
}

/// Formats a location the way ev-grep accepts it: `PATH` or `PATH:N-M`.
#[inline]
#[must_use]
pub fn label(path: &str, lines: Option<LineRange>) -> String {
    match lines {
        Some(range) => format!("{path}:{range}"),
        None => path.to_owned(),
    }
}
