mod control;
mod encoding;
mod records;
mod state;

use std::{io, process::ExitCode};

pub(crate) use encoding::serialize_raw_assessment;
use ev_grep_core::{Assessment, Candidate};
use serde::Serialize;

use crate::sort::Sort;

/// The version of the JSON Lines record shapes, which every record names.
const SCHEMA_VERSION: u32 = 2;

/// How a run ends, as the process exit status reports it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExitStatus {
    /// An input, provider, or output error happened, or `--strict` saw a file over the file limit.
    Error,
    /// A candidate matched, or a dry run selected a file it would assess.
    Match,
    /// No candidate matched and none was uncertain.
    NoMatch,
    /// No error happened and a candidate was uncertain.
    Uncertain,
}

impl ExitStatus {
    /// The process exit code, which the trace's search span also records.
    pub(crate) const fn to_code(self) -> u8 {
        match self {
            Self::Match => 0,
            Self::NoMatch => 1,
            Self::Error => 2,
            Self::Uncertain => 3,
        }
    }
}

impl From<ExitStatus> for ExitCode {
    #[inline]
    fn from(status: ExitStatus) -> Self {
        Self::from(status.to_code())
    }
}

/// Standard output, which receives only the search's results.
#[expect(
    clippy::disallowed_methods,
    reason = "the output module opens standard output, as cli-output-channels requires"
)]
pub(crate) fn lock_stdout() -> io::StdoutLock<'static> {
    io::stdout().lock()
}

/// Standard error, which receives errors, skip notices, the summary, and a trace written to `-`.
#[expect(
    clippy::disallowed_methods,
    reason = "the output module opens standard error, as cli-output-channels requires"
)]
pub(crate) fn open_stderr() -> io::Stderr {
    io::stderr()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Format {
    Json,
    Text,
}

#[derive(Default, Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the field order is the order of the summary's JSON keys"
)]
pub(crate) struct Summary {
    pub selected: usize,
    pub evaluated: usize,
    pub matches: usize,
    pub no_match: usize,
    pub uncertain: usize,
    pub skipped: usize,
    pub errors: usize,
    pub dry_run: bool,
    /// Files skipped for exceeding the file limit, also counted in `skipped`.
    #[serde(skip)]
    pub too_large: usize,
    /// Whether a file skipped for exceeding the file limit fails the run, as `--strict` asks.
    #[serde(skip)]
    pub strict: bool,
}

impl Summary {
    pub(crate) const fn exit_status(&self) -> ExitStatus {
        if self.errors > 0 || (self.strict && self.too_large > 0) {
            ExitStatus::Error
        } else if self.uncertain > 0 {
            ExitStatus::Uncertain
        } else if self.matches > 0 || (self.dry_run && self.selected > self.skipped) {
            ExitStatus::Match
        } else {
            ExitStatus::NoMatch
        }
    }
}

pub(crate) struct Output<Writer> {
    format: Format,
    min_confidence: f64,
    pending: Vec<(Candidate, Assessment)>,
    sort: Sort,
    summary: Summary,
    writer: Writer,
}

/// A record's data: the keys of `base` and the keys `added` to them.
#[derive(Serialize)]
struct Extended<Added, Base> {
    #[serde(flatten)]
    added: Added,
    #[serde(flatten)]
    base: Base,
}

#[cfg(test)]
mod tests;
