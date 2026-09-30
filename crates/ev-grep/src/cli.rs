use std::{fs::File, io, path::PathBuf};

use anyhow::{Context, Result, ensure};
use clap::Parser;
use ev_grep_core::{MAX_QUERY_BYTES, read_text};
use ev_grep_jev::Provider;

use crate::output::Sort;

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Search text by meaning with Jev",
    override_usage = "ev-grep [OPTIONS] <QUERY> [PATHS]...\n       ev-grep [OPTIONS] --query-file <FILE> [PATHS]..."
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

    /// Include or exclude paths using gitignore globs; prefix exclusions with '!'.
    #[arg(short = 'g', long, value_name = "GLOB")]
    pub glob: Vec<String>,

    /// Service receiving requests: openrouter or typesafe.
    #[arg(long, env = "EV_GREP_PROVIDER", default_value = "openrouter")]
    pub provider: Provider,

    /// Supported pinned model ID for the selected provider.
    #[arg(long, env = "EV_GREP_MODEL")]
    pub model: Option<String>,

    /// Send requests to this trusted, protocol-compatible endpoint.
    #[arg(long, env = "EV_GREP_ENDPOINT")]
    pub endpoint: Option<String>,

    /// Maximum concurrent requests (1 to 256).
    #[arg(short = 'j', long, default_value_t = 4, value_parser = clap::value_parser!(u16).range(1..=256))]
    pub jobs: u16,

    /// Route answers below this confidence to uncertain (0 to 1).
    #[arg(long, default_value_t = ev_grep_jev::MIN_CONFIDENCE, value_parser = confidence)]
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
}

fn confidence(value: &str) -> std::result::Result<f64, String> {
    let value: f64 = value.parse().map_err(|_| "expected a number from 0 to 1")?;
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err("confidence must be between 0 and 1".into());
    }
    Ok(value)
}

impl Cli {
    pub(crate) fn query_and_targets(&self) -> Result<(String, Vec<String>)> {
        let piped = self.stdin || self.candidates.is_some();
        let source_stdin = self.stdin
            || self
                .candidates
                .as_ref()
                .is_some_and(|path| path.as_os_str() == "-");
        ensure!(
            !(source_stdin
                && self
                    .query_file
                    .as_ref()
                    .is_some_and(|path| path.as_os_str() == "-")),
            "stdin cannot contain both query and source"
        );
        let (query, paths) = if let Some(path) = &self.query_file {
            let query = if path.as_os_str() == "-" {
                read_text(io::stdin().lock(), MAX_QUERY_BYTES)?
            } else {
                read_text(
                    File::open(path)
                        .with_context(|| format!("cannot read query file {}", path.display()))?,
                    MAX_QUERY_BYTES,
                )?
            };
            (query, self.inputs.as_slice())
        } else {
            let (query, paths) = self
                .inputs
                .split_first()
                .context("provide a query or --query-file")?;
            (read_text(query.as_bytes(), MAX_QUERY_BYTES)?, paths)
        };
        ensure!(
            !piped || paths.is_empty(),
            "stdin and candidate inputs cannot be combined with paths"
        );
        let targets = if paths.is_empty() && !piped {
            vec![".".to_owned()]
        } else {
            paths.to_vec()
        };
        Ok((query, targets))
    }
}
