use std::time::Instant;

use anyhow::{Context, Result, ensure};
use ev_grep_core::{Evaluator, Outcome, Source};
use ev_grep_jev::{Jev, Provider};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    name: String,
    query: String,
    path: String,
    source: String,
    expected: Outcome,
}

#[tokio::test]
#[ignore = "calls a paid provider; requires its API key"]
async fn evaluate_search_fixtures() -> Result<()> {
    let provider: Provider = std::env::var("EV_GREP_PROVIDER")
        .unwrap_or_else(|_| "openrouter".into())
        .parse()
        .map_err(anyhow::Error::msg)?;
    let model = provider.model(std::env::var("EV_GREP_MODEL").ok().as_deref())?;
    let key = std::env::var(provider.key_variable())
        .context("configure the selected provider's API key")?;
    let evaluator = Jev::new(provider, &model, &key)?;
    let cases: Vec<Case> = serde_json::from_str(include_str!("fixtures/evals.json"))?;
    let started = Instant::now();
    let mut failures = 0;
    for case in &cases {
        // Expectations stay in the harness, never in the model's source or query.
        let source = Source {
            path: case.path.clone(),
            text: case.source.clone(),
        };
        let assessment = evaluator.assess(&case.query, &source).await?;
        let passed = assessment.outcome == case.expected;
        if !passed {
            failures += 1;
        }
        println!(
            "{}: expected={:?} actual={:?} confidence={:.3} model={}",
            case.name, case.expected, assessment.outcome, assessment.confidence, assessment.model
        );
    }
    println!(
        "{} fixtures, {} mismatches, {:.2}s elapsed",
        cases.len(),
        failures,
        started.elapsed().as_secs_f64()
    );
    ensure!(
        failures == 0,
        "{failures} fixture assessments differed from expectations"
    );
    Ok(())
}
