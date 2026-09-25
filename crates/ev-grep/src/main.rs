mod cli;
mod output;

use std::{io, process::ExitCode};

use anyhow::{Context, Result};
use clap::Parser;
use ev_grep_core::{ScanEvent, SourceRead, discover, read_source, scan};
use ev_grep_jev::{Jev, MIN_CONFIDENCE};
use serde_json::json;

use cli::Cli;
use output::Output;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let stdout = io::stdout();
    let mut output = Output::new(stdout.lock(), cli.json, cli.dry_run);
    if let Err(error) = run(&cli, &mut output).await {
        if error
            .downcast_ref::<io::Error>()
            .is_some_and(|e| e.kind() == io::ErrorKind::BrokenPipe)
        {
            return ExitCode::from(2);
        }
        if output.error(None, &format!("{error:#}")).is_err() {
            return ExitCode::from(2);
        }
    }
    match output.finish() {
        Ok(code) => ExitCode::from(code),
        Err(_) => ExitCode::from(2),
    }
}

async fn run(cli: &Cli, output: &mut Output<impl io::Write>) -> Result<()> {
    let (query, roots) = cli.query_and_paths()?;
    let model = cli.provider.model(cli.model.as_deref())?;
    output.event("begin", json!({"provider": cli.provider, "requested_model": model, "min_confidence": MIN_CONFIDENCE, "dry_run": cli.dry_run}))?;
    let files = discover(&roots, &cli.glob)?;
    output.summary.selected = files.paths.len();
    for error in files.errors {
        output.error(Some(&error.path), &error.message)?;
    }
    if cli.dry_run {
        for path in files.paths {
            match read_source(&path) {
                Ok(SourceRead::Text(source)) => output.selected(&source.path, source.text.len())?,
                Ok(SourceRead::Binary) => output.scan_event(ScanEvent::Skipped {
                    path: path.display().to_string(),
                })?,
                Err(error) => {
                    output.error(Some(&path.display().to_string()), &format!("{error:#}"))?
                }
            }
        }
        return Ok(());
    }
    if files.paths.is_empty() {
        return Ok(());
    }
    let key_name = cli.provider.key_variable();
    let key = std::env::var(key_name)
        .with_context(|| format!("set {key_name} for provider {}", cli.provider))?;
    let jev = Jev::new(cli.provider, &model, &key)?;
    scan(files.paths, &query, &jev, |event| output.scan_event(event)).await
}
