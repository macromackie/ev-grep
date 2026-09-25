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

    /// Emit versioned JSON Lines, including nonmatches, errors, and a final summary.
    #[arg(long)]
    pub json: bool,

    /// Inspect file selection and size limits without credentials or API requests.
    #[arg(long)]
    pub dry_run: bool,
}

impl Cli {
    pub(crate) fn query_and_paths(&self) -> Result<(String, Vec<PathBuf>)> {
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
        let paths = if paths.is_empty() {
            vec![PathBuf::from(".")]
        } else {
            paths.iter().map(PathBuf::from).collect()
        };
        Ok((query, paths))
    }
}
