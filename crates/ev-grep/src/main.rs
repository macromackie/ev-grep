mod cli;
mod output;
mod sarif;

use std::{io, process::ExitCode};

use anyhow::{Context, Result};
use clap::Parser;
use ev_grep_core::{ScanEvent, SourceRead, discover, read_target, scan_with_jobs};
use ev_grep_jev::{Jev, MIN_CONFIDENCE};
use serde_json::json;

use cli::Cli;
use output::{Format, Output};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let stdout = io::stdout();
    let format = match (cli.sarif, cli.json) {
        (true, _) => Format::Sarif,
        (false, true) => Format::Json,
        (false, false) => Format::Text,
    };
    let mut output = Output::new(stdout.lock(), format, cli.dry_run);
    if let Err(error) = run(&cli, &mut output).await {
        if error
            .downcast_ref::<io::Error>()
            .is_some_and(|e| e.kind() == io::ErrorKind::BrokenPipe)
        {
            return ExitCode::from(2);
        }
        if output.error(None, None, &format!("{error:#}")).is_err() {
            return ExitCode::from(2);
        }
    }
    match output.finish() {
        Ok(code) => ExitCode::from(code),
        Err(_) => ExitCode::from(2),
    }
}

async fn run(cli: &Cli, output: &mut Output<impl io::Write>) -> Result<()> {
    let (query, arguments) = cli.query_and_targets()?;
    let model = cli.provider.model(cli.model.as_deref())?;
    output.begin(
        &query,
        json!({"provider": cli.provider, "requested_model": model, "min_confidence": MIN_CONFIDENCE, "dry_run": cli.dry_run, "jobs": cli.jobs}),
    )?;
    let found = discover(&arguments, &cli.glob)?;
    output.summary.selected = found.targets.len();
    for error in found.errors {
        output.error(Some(&error.path), error.lines, &error.message)?;
    }
    if cli.dry_run {
        for target in found.targets {
            let path = target.path.display().to_string();
            match read_target(&target) {
                Ok(SourceRead::Text(source)) => output.selected(&source)?,
                Ok(SourceRead::Binary) => output.scan_event(ScanEvent::Skipped {
                    path,
                    lines: target.lines,
                })?,
                Err(error) => output.error(Some(&path), target.lines, &format!("{error:#}"))?,
            }
        }
        return Ok(());
    }
    if found.targets.is_empty() {
        return Ok(());
    }
    let key_name = cli.provider.key_variable();
    let key = std::env::var(key_name)
        .with_context(|| format!("set {key_name} for provider {}", cli.provider))?;
    let jev = Jev::with_endpoint(
        cli.provider,
        &model,
        &key,
        cli.endpoint.as_deref().unwrap_or(cli.provider.endpoint()),
    )?;
    scan_with_jobs(
        found.targets,
        &query,
        &jev,
        usize::from(cli.jobs),
        |event| output.scan_event(event),
    )
    .await
}
