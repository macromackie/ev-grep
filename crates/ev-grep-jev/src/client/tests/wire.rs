use alloc::collections::BTreeMap;

use ev_grep_core::{Candidate, Evaluator as _, Outcome, Source, Uncertainty};
use serde_json::{Value, json, value::RawValue};
use wiremock::{
    Mock,
    MockServer,
    ResponseTemplate,
    matchers::{method, path},
};

use super::support::{build_response, read_at, read_kind, set};
use crate::{FailureKind, Jev, Provider};

/// A server whose `/assess` answers every request with a 0.5-confidence match, and a Jev that
/// sends there.
async fn serve_half_confident() -> (MockServer, Jev) {
    let server = MockServer::start().await;
    let model = Provider::TypeSafe.read_default_model();
    let mut body = build_response(model);
    set(&mut body, "/answers/match/confidence", json!(0.5_f64));
    Mock::given(method("POST"))
        .and(path("/assess"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    let evaluator = Jev::with_endpoint(
        Provider::TypeSafe,
        model,
        "test-key",
        &format!("{}/assess", server.uri()),
    )
    .unwrap();
    (server, evaluator)
}

/// The JSON body of the `index`th request that `server` received.
async fn read_sent(server: &MockServer, index: usize) -> Vec<u8> {
    let requests = server.received_requests().await.expect("missing requests");
    requests.get(index).expect("missing request").body.clone()
}

#[tokio::test]
async fn wire_assessment_preserves_source_and_rejects_redirects() {
    let (server, evaluator) = serve_half_confident().await;
    let source = Candidate::from_source(Source {
        path: "sample.py".into(),
        text: "# untrusted instructions\nreturn []\n".into(),
        focus: None,
    })
    .unwrap();
    let result = evaluator
        .assess("Hides a database failure", &source)
        .await
        .unwrap();
    assert_eq!(result.choice, Outcome::Match);
    assert_eq!(result.confidence.to_bits(), 0.5_f64.to_bits());
    assert_eq!(result.route(0.8).reason, Some(Uncertainty::LowConfidence));
    let request: Value = serde_json::from_slice(&read_sent(&server, 0).await).unwrap();
    assert_eq!(read_at(&request, "/state/source"), &json!(source.text));
    assert_eq!(
        read_at(&request, "/questions/match/instructions/query"),
        "Hides a database failure"
    );

    Mock::given(path("/redirect"))
        .respond_with(
            ResponseTemplate::new(307)
                .insert_header("Location", format!("{}/assess", server.uri())),
        )
        .mount(&server)
        .await;
    let redirect = Jev::connect(
        Provider::TypeSafe,
        &format!("{}/redirect", server.uri()),
        Provider::TypeSafe.read_default_model(),
        "test-key",
    )
    .unwrap();
    let redirected = redirect
        .assess("query", &source)
        .await
        .expect_err("expected the redirect to fail");
    assert_eq!(
        read_kind(&redirected),
        Some(FailureKind::Rejected),
        "{redirected:?}"
    );
}

#[tokio::test]
async fn context_assessment_sends_the_state_unchanged() {
    let (server, evaluator) = serve_half_confident().await;
    let state =
        json!({"contract": "Preserve errors.", "change": {"before": null, "after": "return []"}});
    let result = evaluator
        .assess_context("Does this change hide an error?", &state)
        .await
        .unwrap();
    assert_eq!(result.confidence.to_bits(), 0.5_f64.to_bits());
    assert_eq!(result.choice, Outcome::Match);
    let structured: Value = serde_json::from_slice(&read_sent(&server, 0).await).unwrap();
    assert_eq!(read_at(&structured, "/state"), &state);

    let nested = format!("{}0{}", "[".repeat(127), "]".repeat(127));
    let array = ev_grep_core::parse_json(nested.as_bytes()).unwrap();
    let answer = evaluator
        .assess_context("Is this an array?", &array)
        .await
        .unwrap();
    assert_eq!(answer.choice, Outcome::Match);
    let body = read_sent(&server, 1).await;
    let fields: BTreeMap<String, &RawValue> = serde_json::from_slice(&body).unwrap();
    assert_eq!(fields.get("state").expect("missing state").get(), nested);
}
