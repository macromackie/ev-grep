use core::num::ParseFloatError;
use std::{fs::File, io, path::PathBuf};

use clap::Parser;
use ev_grep_core::{MAX_QUERY_BYTES, read_text};
use ev_grep_jev::Provider;

use crate::{error::CliError, sort::Sort, telemetry::Environment};

#[derive(Debug, Parser)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "clap prints the options in field order, which groups them for --help"
)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is one command-line flag"
)]
#[command(
    version,
    about = "Search text by meaning with Jev",
    override_usage = "ev-grep [OPTIONS] <QUERY> [PATHS]...\n       ev-grep [OPTIONS] --query-file \
                      <FILE> [PATHS]..."
)]
pub(crate) struct Cli {
    /// Query followed by search paths; with --query-file, all values are paths.
    /// A file path may end in :N or :N-M to assess only those lines, counting from 1.
    #[arg(value_name = "QUERY_OR_PATH")]
    pub inputs: Vec<String>,

    /// Read one multiline query from FILE, or from stdin when FILE is '-'.
    #[arg(short = 'f', long, value_name = "FILE")]
    pub query_file: Option<PathBuf>,

    /// Read stdin as one text candidate.
    #[arg(long, conflicts_with_all = ["candidates", "glob"])]
    pub stdin: bool,

    /// Read candidate JSON Lines from FILE, or stdin with '-'.
    #[arg(long, value_name = "FILE", conflicts_with = "glob")]
    pub candidates: Option<PathBuf>,

    /// Assess one JSON value from FILE, or stdin with '-', using the context-question prompt.
    #[arg(
        long,
        value_name = "FILE",
        conflicts_with_all = [
            "stdin", "candidates", "context", "glob", "sort", "strict", "max_candidates",
        ]
    )]
    pub state: Option<PathBuf>,

    /// Supply a supporting file or PATH:N-M range for every candidate; repeat for more sources.
    #[arg(long, value_name = "PATH[:N-M]")]
    pub context: Vec<String>,

    /// Include or exclude paths using gitignore globs; prefix exclusions with '!'.
    #[arg(short = 'g', long, value_name = "GLOB")]
    pub glob: Vec<String>,

    /// Service receiving requests: openrouter or typesafe.
    #[arg(long, env = "EV_GREP_PROVIDER", default_value = "openrouter")]
    pub provider: Provider,

    /// Supported pinned model ID for the selected provider.
    #[arg(long, env = "EV_GREP_MODEL")]
    pub model: Option<String>,

    /// Send --model even when the supported-model table omits it; the begin record marks it
    /// unvalidated.
    #[arg(long, requires = "model")]
    pub unvalidated_model: bool,

    /// Send requests to this trusted, protocol-compatible endpoint.
    #[arg(long, env = "EV_GREP_ENDPOINT")]
    pub endpoint: Option<String>,

    /// Maximum concurrent requests (1 to 256).
    #[arg(
        short = 'j',
        long,
        default_value_t = 4,
        value_parser = clap::value_parser!(u16).range(1..=256)
    )]
    pub jobs: u16,

    /// Route answers below this confidence to uncertain (0 to 1).
    #[arg(long, default_value_t = ev_grep_core::MIN_CONFIDENCE, value_parser = parse_confidence)]
    pub min_confidence: f64,

    /// Result order: none streams immediately; score waits and sorts by match probability.
    #[arg(long, value_enum, default_value_t = Sort::None)]
    pub sort: Sort,

    /// Emit versioned JSON Lines, including nonmatches, errors, and a final summary.
    #[arg(long)]
    pub json: bool,

    /// Inspect file selection and size limits without credentials or API requests.
    #[arg(long)]
    pub dry_run: bool,

    /// Exit 2 when a file is skipped for exceeding the 64 `KiB` file limit.
    #[arg(long)]
    pub strict: bool,

    /// Fail before any request when more than N files, line ranges, or candidate records are
    /// selected.
    #[arg(long, value_name = "N")]
    pub max_candidates: Option<usize>,

    /// Write a JSON Lines trace of spans and events to PATH, or to stderr with '-'.
    #[arg(long, env = "EV_GREP_TRACE", value_name = "PATH")]
    pub trace: Option<PathBuf>,

    /// Environment that the trace's first record names; the default is ci when CI is set,
    /// otherwise local.
    #[arg(
        long,
        env = "EV_GREP_TRACE_ENVIRONMENT",
        value_enum,
        default_value_t = Environment::detect(),
        hide_default_value = true
    )]
    pub trace_environment: Environment,
}

fn parse_confidence(value: &str) -> Result<f64, String> {
    let number: f64 = value
        .parse()
        .map_err(|error: ParseFloatError| format!("expected a number from 0 to 1: {error:#}"))?;
    if !number.is_finite() || !(0.0_f64..=1.0_f64).contains(&number) {
        return Err("confidence must be between 0 and 1".into());
    }
    Ok(number)
}

impl Cli {
    pub(crate) fn query_and_targets(&self) -> Result<(String, Vec<String>), CliError> {
        let piped = self.stdin || self.candidates.is_some() || self.state.is_some();
        let source_stdin = self.stdin
            || self
                .state
                .as_ref()
                .is_some_and(|path| path.as_os_str() == "-")
            || self
                .candidates
                .as_ref()
                .is_some_and(|path| path.as_os_str() == "-");
        if source_stdin
            && self
                .query_file
                .as_ref()
                .is_some_and(|path| path.as_os_str() == "-")
        {
            return Err(CliError::StdinTwice);
        }
        let (query, paths) = match &self.query_file {
            Some(path) if path.as_os_str() == "-" => {
                (
                    read_text(io::stdin().lock(), MAX_QUERY_BYTES)?,
                    self.inputs.as_slice(),
                )
            },
            Some(path) => {
                let file = File::open(path).map_err(|source| {
                    CliError::QueryFile {
                        path: path.clone(),
                        source,
                    }
                })?;
                (read_text(file, MAX_QUERY_BYTES)?, self.inputs.as_slice())
            },
            None => {
                let (query, paths) = self.inputs.split_first().ok_or(CliError::MissingQuery)?;
                (read_text(query.as_bytes(), MAX_QUERY_BYTES)?, paths)
            },
        };
        if piped && !paths.is_empty() {
            return Err(CliError::PathsWithInput);
        }
        let targets = if paths.is_empty() && !piped {
            vec![".".to_owned()]
        } else {
            paths.to_vec()
        };
        Ok((query, targets))
    }
}
