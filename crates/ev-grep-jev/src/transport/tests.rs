use crate::{Jev, Provider};
use anyhow::Result;
use ev_grep_core::{Evaluator, Outcome, Question};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

fn reply() -> serde_json::Value {
    let answer = json!({"type":"choice","choice":"uncertain","confidence":0.9,
        "probabilities":{"match":0.05,"no_match":0.05,"uncertain":0.9}});
    json!({"id":"request-123","model":Provider::TypeSafe.default_model(),
        "answers":{"relevant":answer,"violation":answer},
        "usage":{"input_tokens":100,"output_tokens":2,"cost":0.001}})
}

#[tokio::test]
async fn a_hedge_accepts_uncertainty_and_accounts_for_the_batch_once() -> Result<()> {
    let server = MockServer::start().await;
    let attempts = Arc::new(AtomicUsize::new(0));
    let seen = attempts.clone();
    Mock::given(path("/decision"))
        .respond_with(move |_: &wiremock::Request| {
            let response = ResponseTemplate::new(200).set_body_json(reply());
            if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                response.set_delay(Duration::from_secs(4))
            } else {
                response
            }
        })
        .mount(&server)
        .await;
    let jev = Jev::with_endpoint(
        Provider::TypeSafe,
        Provider::TypeSafe.default_model(),
        "test",
        &format!("{}/decision", server.uri()),
    )?
    .with_jobs(2)?;
    let questions = [
        Question {
            id: "relevant",
            query: "Relevant?",
        },
        Question {
            id: "violation",
            query: "Broken?",
        },
    ];
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        jev.assess_questions(&questions, &json!({"file":"example"})),
    )
    .await??;
    assert_eq!(result.request.attempts, 2);
    assert_eq!(result.request.hedges, 1);
    assert_eq!(result.request.input_tokens, 100);
    assert_eq!(result.request.provider_id.as_deref(), Some("request-123"));
    assert_eq!(result.answers.len(), 2);
    assert!(
        result
            .answers
            .values()
            .all(|a| a.outcome == Outcome::Uncertain && a.request.is_none())
    );
    // Completing the race releases both slots, including the cancelled loser.
    let next = tokio::time::timeout(
        Duration::from_secs(1),
        jev.assess_questions(&questions, &json!({})),
    )
    .await??;
    assert_eq!(next.request.attempts, 1);
    Ok(())
}

#[tokio::test]
async fn one_slot_never_hedges_and_cancellation_releases_it() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(path("/decision"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(reply())
                .set_delay(Duration::from_millis(1200)),
        )
        .mount(&server)
        .await;
    let jev = Jev::with_endpoint(
        Provider::TypeSafe,
        Provider::TypeSafe.default_model(),
        "test",
        &format!("{}/decision", server.uri()),
    )?
    .with_jobs(1)?;
    let questions = [
        Question {
            id: "relevant",
            query: "Relevant?",
        },
        Question {
            id: "violation",
            query: "Broken?",
        },
    ];
    let state = json!({});
    assert!(
        tokio::time::timeout(
            Duration::from_millis(100),
            jev.assess_questions(&questions, &state)
        )
        .await
        .is_err()
    );
    let result = jev.assess_questions(&questions, &state).await?;
    assert_eq!(result.request.attempts, 1);
    assert_eq!(result.request.hedges, 0);
    assert_eq!(
        server.received_requests().await.unwrap_or_default().len(),
        2
    );
    Ok(())
}

#[test]
fn partial_batches_are_protocol_errors() -> Result<()> {
    let value = reply();
    let model = Provider::TypeSafe.default_model();
    assert!(
        crate::protocol::assessment(
            &serde_json::to_vec(&value)?,
            model,
            &["relevant", "missing"]
        )
        .is_err()
    );
    Ok(())
}
