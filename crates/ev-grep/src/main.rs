mod cli;
mod inputs;
mod output;
mod sarif;

use std::{io, process::ExitCode};

use anyhow::{Context, Result};
use clap::Parser;
use ev_grep_core::{Input, Loaded, ScanEvent, scan_inputs};
use ev_grep_jev::Jev;
use futures_util::{StreamExt, stream};
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
    let mut output = Output::new(stdout.lock(), format, cli.dry_run, cli.sort);
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
        json!({"provider": cli.provider, "requested_model": model, "min_confidence": cli.min_confidence, "dry_run": cli.dry_run, "jobs": cli.jobs, "sort": cli.sort}),
    )?;
    let (mut inputs, errors) = inputs::select(cli, &arguments).await?;
    for error in errors {
        output.error(Some(&error.path), error.lines, &error.message)?;
    }
    if cli.dry_run {
        while let Some(input) = inputs.next().await {
            output.summary.selected += 1;
            match input.load() {
                Ok(Loaded::Candidate(candidate)) => output.selected(&candidate)?,
                Ok(Loaded::Binary(target)) => output.scan_event(ScanEvent::Skipped {
                    path: target.path.display().to_string(),
                    lines: target.lines,
                })?,
                Err(error) => output.error(Some(&error.path), error.lines, &error.message)?,
            }
        }
        return Ok(());
    }
    let first = loop {
        match inputs.next().await {
            None => return Ok(()),
            Some(Input::Error(error)) => {
                output.summary.selected += 1;
                output.error(Some(&error.path), error.lines, &error.message)?;
            }
            Some(input) => break input,
        }
    };
    let key_name = cli.provider.key_variable();
    let key = std::env::var(key_name)
        .with_context(|| format!("set {key_name} for provider {}", cli.provider))?;
    let jev = Jev::with_endpoint(
        cli.provider,
        &model,
        &key,
        cli.endpoint.as_deref().unwrap_or(cli.provider.endpoint()),
    )?
    .with_min_confidence(cli.min_confidence)?
    .with_jobs(usize::from(cli.jobs))?;
    scan_inputs(
        stream::once(std::future::ready(first)).chain(inputs),
        &query,
        &jev,
        usize::from(cli.jobs),
        |event| {
            output.summary.selected += 1;
            output.scan_event(event)
        },
    )
    .await
}
