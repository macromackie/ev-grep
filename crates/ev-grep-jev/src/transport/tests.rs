use alloc::sync::Arc;
use core::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use ev_grep_core::{Evaluator as _, Outcome, Question};
use serde_json::{Map, Value, json};
use tokio::time::{error::Elapsed, timeout};
use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers::path};

use crate::{FailureKind, Jev, JevError, Provider, RequestError, protocol};

const QUESTIONS: [Question<'static>; 2] = [
    Question {
        id: "relevant",
        query: "Relevant?",
    },
    Question {
        id: "violation",
        query: "Broken?",
    },
];

/// A valid answer for each of `ids`.
fn reply(ids: &[&str]) -> Value {
    let answer = json!({"type":"choice","choice":"uncertain","confidence":0.9_f64,
        "probabilities":{"match":0.05_f64,"no_match":0.05_f64,"uncertain":0.9_f64}});
    let answers: Map<String, Value> = ids
        .iter()
        .map(|id| ((*id).to_owned(), answer.clone()))
        .collect();
    json!({"id":"request-123","model":Provider::TypeSafe.read_default_model(),
        "answers":answers,
        "usage":{"input_tokens":100_u64,"output_tokens":2_u64,"cost":0.001_f64}})
}

fn build_jev(server: &MockServer) -> Jev {
    Jev::with_endpoint(
        Provider::TypeSafe,
        Provider::TypeSafe.read_default_model(),
        "test",
        &format!("{}/decision", server.uri()),
    )
    .unwrap()
}

#[tokio::test]
async fn a_hedge_accepts_uncertainty_and_accounts_for_the_batch_once() {
    let server = MockServer::start().await;
    let attempts = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&attempts);
    Mock::given(path("/decision"))
        .respond_with(move |_: &Request| {
            let response =
                ResponseTemplate::new(200).set_body_json(reply(&["relevant", "violation"]));
            if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                response.set_delay(Duration::from_secs(4))
            } else {
                response
            }
        })
        .mount(&server)
        .await;
    let hedged = build_jev(&server).with_jobs(2).unwrap();
    let result = timeout(
        Duration::from_secs(3),
        hedged.assess_questions(&QUESTIONS, &json!({"file":"example"})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result.request.attempts, 2);
    assert_eq!(result.request.hedges, 1);
    assert_eq!(result.request.input_tokens, 100);
    assert_eq!(result.request.provider_id.as_deref(), Some("request-123"));
    assert_eq!(result.answers.len(), 2);
    assert!(
        result
            .answers
            .values()
            .all(|answer| answer.choice == Outcome::Uncertain && answer.request.is_none())
    );
    // Completing the race releases both slots, including the cancelled loser.
    let next = timeout(
        Duration::from_secs(1),
        hedged.assess_questions(&QUESTIONS, &json!({})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(next.request.attempts, 1);
}

#[tokio::test]
async fn one_slot_never_hedges_and_cancellation_releases_it() {
    let server = MockServer::start().await;
    Mock::given(path("/decision"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(reply(&["relevant", "violation"]))
                .set_delay(Duration::from_millis(1200)),
        )
        .mount(&server)
        .await;
    let single = build_jev(&server).with_jobs(1).unwrap();
    let state = json!({});
    let _: Elapsed = timeout(
        Duration::from_millis(100),
        single.assess_questions(&QUESTIONS, &state),
    )
    .await
    .expect_err("expected the caller's timeout");
    let result = single.assess_questions(&QUESTIONS, &state).await.unwrap();
    assert_eq!(result.request.attempts, 1);
    assert_eq!(result.request.hedges, 0);
    assert_eq!(
        server.received_requests().await.unwrap_or_default().len(),
        2
    );
}

#[tokio::test]
async fn invalid_answers_are_retried_within_the_attempt_limit() {
    let invalid = json!({"model":Provider::TypeSafe.read_default_model(),
        "answers":{"relevant":{"type":"choice","choice":"match","confidence":0.9_f64,
            "probabilities":{"match":0.1_f64,"no_match":0.8_f64,"uncertain":0.1_f64}}},
        "usage":{"input_tokens":100_u64,"output_tokens":2_u64}});
    let server = MockServer::start().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let valid = reply(&["relevant"]);
    Mock::given(path("/decision"))
        .respond_with(move |_: &Request| {
            let body = if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                &invalid
            } else {
                &valid
            };
            ResponseTemplate::new(200).set_body_json(body)
        })
        .mount(&server)
        .await;
    let question = [Question {
        id: "relevant",
        query: "Relevant?",
    }];
    let result = build_jev(&server)
        .assess_questions(&question, &json!({}))
        .await
        .unwrap();
    assert_eq!(result.request.attempts, 2);

    // A provider that never answers validly is an error after three attempts, not a verdict.
    let broken = MockServer::start().await;
    Mock::given(path("/decision"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"model": "other"})))
        .mount(&broken)
        .await;
    assert!(matches!(
        build_jev(&broken)
            .assess_questions(&question, &json!({}))
            .await,
        Err(JevError::Request(RequestError {
            kind: FailureKind::InvalidResponse,
            ..
        }))
    ));
    assert_eq!(
        broken.received_requests().await.unwrap_or_default().len(),
        3
    );
}

#[test]
fn partial_batches_are_protocol_errors() {
    let value = reply(&["relevant", "violation"]);
    let model = Provider::TypeSafe.read_default_model();
    let failure = protocol::parse_assessment(
        &serde_json::to_vec(&value).unwrap(),
        model,
        &["relevant", "missing"],
    )
    .expect_err("expected a partial batch to fail");
    assert_eq!(failure.kind, FailureKind::InvalidResponse);
}
