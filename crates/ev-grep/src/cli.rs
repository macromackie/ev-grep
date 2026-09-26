use std::{fs::File, io, path::PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use ev_grep_core::{MAX_QUERY_BYTES, read_text};
use ev_grep_jev::Provider;

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Find files by meaning with Jev",
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

    /// Include or exclude paths using gitignore globs; prefix exclusions with '!'.
    #[arg(short = 'g', long, value_name = "GLOB")]
    pub glob: Vec<String>,

    /// Service receiving requests: openrouter or typesafe.
    #[arg(long, env = "EV_GREP_PROVIDER", default_value = "openrouter")]
    pub provider: Provider,

    /// Supported pinned model ID for the selected provider.
    #[arg(long, env = "EV_GREP_MODEL")]
    pub model: Option<String>,

    /// Maximum concurrent requests (1 to 256).
    #[arg(short = 'j', long, default_value_t = 4, value_parser = clap::value_parser!(u16).range(1..=256))]
    pub jobs: u16,

    /// Emit versioned JSON Lines, including nonmatches, errors, and a final summary.
    #[arg(long)]
    pub json: bool,

    /// Write one SARIF 2.1.0 log to stdout when the search ends, for code scanning tools.
    #[arg(long, conflicts_with = "json")]
    pub sarif: bool,

    /// Inspect file selection and size limits without credentials or API requests.
    #[arg(long)]
    pub dry_run: bool,
}

impl Cli {
    pub(crate) fn query_and_targets(&self) -> Result<(String, Vec<String>)> {
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
        let targets = if paths.is_empty() {
            vec![".".to_owned()]
        } else {
            paths.to_vec()
        };
        Ok((query, targets))
    }
}
