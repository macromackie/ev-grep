mod cli;
mod error;
mod inputs;
mod output;
mod telemetry;

use std::{io, process::ExitCode};

use clap::Parser;
use ev_grep_core::{Evaluator, Failure, Input, Loaded, ScanEvent, SkipReason, scan_inputs};
use ev_grep_jev::Jev;
use futures_util::{StreamExt, stream};
use serde_json::json;
use tracing::Instrument;

use cli::Cli;
use error::Error;
use output::{ExitStatus, Format, Output};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let format = if cli.json { Format::Json } else { Format::Text };
    let mut output = Output::new(
        output::stdout(),
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
    // A run error is reported before the summary; the run ends in an error when stdout cannot be written.
    let status = match result {
        Err(Error::Output(failure)) if failure.kind() == io::ErrorKind::BrokenPipe => Err(failure),
        Err(error) => output
            .error(None, None, error.kind(), &error.message())
            .and_then(|()| output.finish()),
        Ok(()) => output.finish(),
    }
    .unwrap_or(ExitStatus::Error);
    span.record("exit_code", status.code());
    ExitCode::from(status)
}

async fn run(cli: &Cli, output: &mut Output<impl io::Write>) -> Result<(), Error> {
    let (query, arguments) = cli.query_and_targets()?;
    let model = match &cli.model {
        Some(model) if cli.unvalidated_model => model.clone(),
        requested => cli.provider.model(requested.as_deref())?,
    };
    let model_validated = cli.provider.supported_models().contains(&model.as_str());
    if let Some(path) = &cli.state {
        let input = inputs::state(path)?;
        output.begin(
            json!({"mode": "state", "query": query, "version": env!("CARGO_PKG_VERSION"), "prompt_id": ev_grep_jev::context_prompt_id(), "provider": cli.provider, "requested_model": model, "model_validated": model_validated, "min_confidence": cli.min_confidence, "dry_run": cli.dry_run, "jobs": cli.jobs}),
        )?;
        output.select_state(&input)?;
        if cli.dry_run {
            return Ok(());
        }
        let assessment = evaluator(cli, &model)?
            .assess_context(&query, &input.state)
            .await?;
        return Ok(output.state_result(&input, &assessment)?);
    }
    output.begin(
        json!({"query": query, "version": env!("CARGO_PKG_VERSION"), "prompt_id": ev_grep_jev::search_prompt_id(), "provider": cli.provider, "requested_model": model, "model_validated": model_validated, "min_confidence": cli.min_confidence, "dry_run": cli.dry_run, "strict": cli.strict, "max_candidates": cli.max_candidates, "jobs": cli.jobs, "sort": cli.sort}),
    )?;
    let supporting = inputs::supporting(&cli.context)?;
    let (mut inputs, errors, symlinks) = inputs::select(cli, &arguments).await?;
    for error in errors {
        output.error(Some(&error.path), error.lines, error.kind, &error.message)?;
    }
    for path in symlinks {
        output.scan_event(ScanEvent::Skipped {
            path,
            lines: None,
            reason: SkipReason::Symlink,
        })?;
    }
    if let Some(limit) = cli.max_candidates {
        let selected: Vec<Input> = inputs
            .by_ref()
            .take(limit.saturating_add(1))
            .collect()
            .await;
        if selected.len() > limit {
            return Err(Error::TooManyCandidates { limit });
        }
        inputs = Box::pin(stream::iter(selected));
    }
    if cli.dry_run {
        while let Some(input) = inputs.next().await {
            match input.load(&supporting) {
                Ok(Loaded::Candidate(candidate)) => output.selected(&candidate)?,
                Ok(Loaded::Skipped { target, reason }) => {
                    output.scan_event(ScanEvent::Skipped {
                        path: target.path.display().to_string(),
                        lines: target.lines,
                        reason,
                    })?
                }
                Err(error) => output.scan_event(ScanEvent::Error(error))?,
            }
        }
        return Ok(());
    }
    let first = loop {
        match inputs.next().await {
            None => return Ok(()),
            Some(Input::Error(error)) => output.scan_event(ScanEvent::Error(error))?,
            Some(input) => break input,
        }
    };
    let jev = evaluator(cli, &model)?;
    scan_inputs(
        stream::once(std::future::ready(first)).chain(inputs),
        &query,
        &supporting,
        &jev,
        usize::from(cli.jobs),
        |event| output.scan_event(event).map_err(Error::Output),
    )
    .await
}

fn evaluator(cli: &Cli, model: &str) -> Result<Jev, Error> {
    let variable = cli.provider.key_variable();
    let key = std::env::var(variable).map_err(|_| Error::MissingKey {
        variable,
        provider: cli.provider,
    })?;
    let endpoint = cli.endpoint.as_deref().unwrap_or(cli.provider.endpoint());
    let jev = if cli.unvalidated_model {
        Jev::with_unvalidated_model(cli.provider, model, &key, endpoint)?
    } else {
        Jev::with_endpoint(cli.provider, model, &key, endpoint)?
    };
    Ok(jev.with_jobs(usize::from(cli.jobs))?)
}
