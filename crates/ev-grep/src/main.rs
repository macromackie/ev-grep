//! The ev-grep command: search text by meaning with Jev.

extern crate alloc;

mod cli;
mod error;
mod inputs;
mod output;
mod telemetry;

use core::future;
use std::{
    env::{self, VarError},
    io,
    path::Path,
    process::ExitCode,
};

use clap::Parser as _;
use cli::Cli;
use error::CliError;
use ev_grep_core::{
    Evaluator as _,
    Failure as _,
    Input,
    Loaded,
    ScanEvent,
    SkipReason,
    SupportingSource,
    scan_inputs,
};
use ev_grep_jev::Jev;
use futures_util::{StreamExt as _, stream};
use inputs::Inputs;
use output::{ExitStatus, Format, Output};
use serde_json::json;
use tracing::Instrument as _;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let format = if cli.json { Format::Json } else { Format::Text };
    let mut output = Output::new(
        output::lock_stdout(),
        format,
        cli.dry_run,
        cli.strict,
        cli.sort,
        cli.min_confidence,
    );
    let traced = match &cli.trace {
        Some(path) => telemetry::install(path, cli.trace_environment),
        None => Ok(()),
    };
    let span = telemetry::search();
    let result = async {
        traced?;
        run(&cli, &mut output).await
    }
    .instrument(span.clone())
    .await;
    // A run error is reported before the summary; the run ends in an error when stdout cannot be
    // written.
    let status = match result {
        Err(CliError::Output(failure)) if failure.kind() == io::ErrorKind::BrokenPipe => {
            Err(failure)
        },
        Err(error) => {
            output
                .write_error(None, None, error.classify(), &error.format_message())
                .and_then(|()| output.finish())
        },
        Ok(()) => output.finish(),
    }
    .unwrap_or(ExitStatus::Error);
    let _: &tracing::Span = span.record("exit_code", status.to_code());
    ExitCode::from(status)
}

#[expect(
    clippy::future_not_send,
    reason = "the runtime is current_thread, and the output holds the stdout lock for the whole \
              run"
)]
async fn run(cli: &Cli, output: &mut Output<impl io::Write>) -> Result<(), CliError> {
    let (query, arguments) = cli.query_and_targets()?;
    let model = match &cli.model {
        Some(model) if cli.unvalidated_model => model.clone(),
        requested => cli.provider.select_model(requested.as_deref())?,
    };
    let model_validated = cli
        .provider
        .list_supported_models()
        .contains(&model.as_str());
    if let Some(path) = &cli.state {
        return run_state(cli, output, path, &query, &model, model_validated).await;
    }
    output.begin(
        json!({"query": query, "version": env!("CARGO_PKG_VERSION"), "prompt_id": ev_grep_jev::search_prompt_id(), "provider": cli.provider, "requested_model": model, "model_validated": model_validated, "min_confidence": cli.min_confidence, "dry_run": cli.dry_run, "strict": cli.strict, "max_candidates": cli.max_candidates, "jobs": cli.jobs, "sort": cli.sort}),
    )?;
    let supporting = inputs::read_supporting(&cli.context)?;
    let (selected, errors, symlinks) = inputs::select(cli, &arguments).await?;
    for error in errors {
        output.write_error(Some(&error.path), error.lines, error.kind, &error.message)?;
    }
    for path in symlinks {
        output.scan_event(ScanEvent::Skipped {
            path,
            lines: None,
            reason: SkipReason::Symlink,
        })?;
    }
    let mut inputs = inputs::limit(selected, cli.max_candidates).await?;
    if cli.dry_run {
        return run_dry(inputs, &supporting, output).await;
    }
    let first = loop {
        match inputs.next().await {
            None => return Ok(()),
            Some(Input::Error(error)) => output.scan_event(ScanEvent::Error(error))?,
            Some(input) => break input,
        }
    };
    let jev = build_evaluator(cli, &model)?;
    scan_inputs(
        stream::once(future::ready(first)).chain(inputs),
        &query,
        &supporting,
        &jev,
        usize::from(cli.jobs),
        |event| output.scan_event(event).map_err(CliError::Output),
    )
    .await
}

/// Report each input a dry run would assess, without credentials or requests.
#[expect(
    clippy::future_not_send,
    reason = "the runtime is current_thread, and the output holds the stdout lock for the whole \
              run"
)]
async fn run_dry(
    mut inputs: Inputs,
    supporting: &[SupportingSource],
    output: &mut Output<impl io::Write>,
) -> Result<(), CliError> {
    while let Some(input) = inputs.next().await {
        match input.read(supporting) {
            Ok(Loaded::Candidate(candidate)) => output.write_selected(&candidate)?,
            Ok(Loaded::Skipped { target, reason }) => {
                output.scan_event(ScanEvent::Skipped {
                    path: target.path.display().to_string(),
                    lines: target.lines,
                    reason,
                })?;
            },
            Err(error) => output.scan_event(ScanEvent::Error(error))?,
        }
    }
    Ok(())
}

/// Assess one `--state` value with the context-question prompt.
async fn run_state(
    cli: &Cli,
    output: &mut Output<impl io::Write>,
    path: &Path,
    query: &str,
    model: &str,
    model_validated: bool,
) -> Result<(), CliError> {
    let input = inputs::read_state(path)?;
    output.begin(
        json!({"mode": "state", "query": query, "version": env!("CARGO_PKG_VERSION"), "prompt_id": ev_grep_jev::build_context_prompt_id(), "provider": cli.provider, "requested_model": model, "model_validated": model_validated, "min_confidence": cli.min_confidence, "dry_run": cli.dry_run, "jobs": cli.jobs}),
    )?;
    output.select_state(&input)?;
    if cli.dry_run {
        return Ok(());
    }
    let assessment = build_evaluator(cli, model)?
        .assess_context(query, &input.state)
        .await?;
    Ok(output.write_state_result(&input, &assessment)?)
}

fn build_evaluator(cli: &Cli, model: &str) -> Result<Jev, CliError> {
    let variable = cli.provider.read_key_variable();
    let key = match env::var(variable) {
        Ok(key) => key,
        Err(VarError::NotPresent | VarError::NotUnicode(_)) => {
            return Err(CliError::MissingKey {
                provider: cli.provider,
                variable,
            });
        },
    };
    let endpoint = cli
        .endpoint
        .as_deref()
        .unwrap_or_else(|| cli.provider.read_endpoint());
    let jev = if cli.unvalidated_model {
        Jev::with_unvalidated_model(cli.provider, model, &key, endpoint)?
    } else {
        Jev::with_endpoint(cli.provider, model, &key, endpoint)?
    };
    Ok(jev.with_jobs(usize::from(cli.jobs))?)
}
