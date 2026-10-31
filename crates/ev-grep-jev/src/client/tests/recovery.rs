use ev_grep_core::{Evaluator as _, Outcome};
use serde_json::json;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

use super::support::{build_cached_source, build_response, read_kind, request_failure};
use crate::{FailureKind, Jev, Provider};

/// A Jev for `OpenRouter`'s default model that sends to `route` on `server`.
fn build_openrouter(server: &MockServer, route: &str) -> Jev {
    Jev::connect(
        Provider::OpenRouter,
        &format!("{}{route}", server.uri()),
        Provider::OpenRouter.read_default_model(),
        "test-key",
    )
    .unwrap()
}

#[tokio::test]
async fn a_transient_failure_is_retried() {
    let server = MockServer::start().await;
    let model = Provider::OpenRouter.read_default_model();
    Mock::given(path("/recover"))
        .respond_with(ResponseTemplate::new(520).insert_header("Retry-After", "0"))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(path("/recover"))
        .respond_with(ResponseTemplate::new(200).set_body_json(build_response(model)))
        .mount(&server)
        .await;
    let recovering = build_openrouter(&server, "/recover");
    let answer = recovering
        .assess("Returns cached values", &build_cached_source())
        .await
        .unwrap();
    assert_eq!(answer.choice, Outcome::Match);
}

#[tokio::test]
async fn retries_are_bounded_and_keep_only_safe_details() {
    let server = MockServer::start().await;
    Mock::given(path("/down"))
        .respond_with(
            ResponseTemplate::new(520)
                .insert_header("Retry-After", "0")
                .insert_header("cf-ray", "example-SJC")
                .set_body_string("private provider response"),
        )
        .expect(3)
        .mount(&server)
        .await;
    let message = build_openrouter(&server, "/down")
        .assess("Returns cached values", &build_cached_source())
        .await
        .expect_err("expected failure")
        .to_string();
    assert!(message.contains("520") && message.contains("cf-ray=example-SJC"));
    assert!(!message.contains("private provider response"));
    server.verify().await;
}

#[tokio::test]
async fn a_rejected_credential_stops_later_requests() {
    let server = MockServer::start().await;
    Mock::given(path("/unauthorized"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    // A rejected credential is sent once, and no later request that shares it starts.
    let unauthorized = build_openrouter(&server, "/unauthorized");
    let source = build_cached_source();
    for _ in 0_u8..2 {
        let rejected = unauthorized
            .assess("query", &source)
            .await
            .expect_err("expected a rejected credential");
        assert_eq!(
            read_kind(&rejected),
            Some(FailureKind::CredentialRejected),
            "{rejected:?}"
        );
    }
    server.verify().await;
}

#[tokio::test]
async fn a_model_mismatch_is_not_retried() {
    let server = MockServer::start().await;
    // Every attempt would reach the same renamed model, so a mismatch is sent once.
    let renamed = "typesafe/jev-1.14-20261101";
    Mock::given(path("/renamed"))
        .respond_with(ResponseTemplate::new(200).set_body_json(build_response(renamed)))
        .expect(1)
        .mount(&server)
        .await;
    let mismatch = build_openrouter(&server, "/renamed")
        .assess("query", &build_cached_source())
        .await
        .expect_err("expected a model mismatch");
    let failure = request_failure(&mismatch).expect("expected a request error");
    assert_eq!(failure.kind, FailureKind::ModelMismatch);
    assert!(!failure.kind.is_retryable());
    let message = mismatch.to_string();
    assert!(
        message.contains(renamed) && message.contains(Provider::OpenRouter.read_default_model()),
        "{message:?}"
    );
    server.verify().await;
}

#[tokio::test]
async fn moderation_keeps_only_reason_codes() {
    let server = MockServer::start().await;
    // A moderation 403 keeps its reason codes, never the flagged input or message.
    Mock::given(path("/moderated"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({"error": {
            "code": 403_u64, "message": "private provider message",
            "metadata": {
                "reasons": ["violence", "private reason text"],
                "flagged_input": "private source"
            }
        }})))
        .expect(1)
        .mount(&server)
        .await;
    let moderated = build_openrouter(&server, "/moderated")
        .assess("query", &build_cached_source())
        .await
        .expect_err("expected a moderation error");
    let failure = request_failure(&moderated).expect("expected a request error");
    assert_eq!(failure.kind, FailureKind::Moderation);
    assert_eq!(failure.moderation, Some(vec!["violence".to_owned()]));
    assert!(!format!("{moderated:?}").contains("private"));
    server.verify().await;
}
