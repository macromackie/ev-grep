use ev_grep_core::{Candidate, Evaluator as _, MAX_FILE_BYTES, Region, Source, SupportingSource};
use serde_json::json;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

use crate::{Jev, Provider};

/// A Jev whose provider fails the test if any request reaches it.
async fn serve_unreachable_provider() -> (MockServer, Jev) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let evaluator = Jev::with_endpoint(
        Provider::TypeSafe,
        Provider::TypeSafe.read_default_model(),
        "test-key",
        &server.uri(),
    )
    .unwrap();
    (server, evaluator)
}

fn build_target() -> Candidate {
    Candidate::from_source(Source {
        path: "target.py".into(),
        text: "save()".into(),
        focus: None,
    })
    .unwrap()
}

#[tokio::test]
async fn reserved_json_keys_make_no_paid_request() {
    let (server, evaluator) = serve_unreachable_provider().await;
    let mut candidate = build_target();
    for key in [
        "$serde_json::private::Number",
        "$serde_json::private::RawValue",
    ] {
        let state = json!({"nested":[{key:"123"}]});
        let in_state = evaluator
            .assess_context("query", &state)
            .await
            .expect_err("expected reserved state key error");
        assert!(in_state.to_string().contains("reserved JSON object key"));
        candidate.meta_variables = Some(state);
        let in_metadata = evaluator
            .assess("query", &candidate)
            .await
            .expect_err("expected reserved metadata key error");
        assert!(in_metadata.to_string().contains("reserved JSON object key"));
    }
    server.verify().await;
}

#[tokio::test]
async fn oversized_inputs_make_no_paid_request() {
    let (server, evaluator) = serve_unreachable_provider().await;
    let mut candidate = build_target();
    let text = "x".repeat(MAX_FILE_BYTES);
    candidate.supporting.push(SupportingSource {
        file: "not-on-disk.py".into(),
        range: Region::from_offsets(&text, 0..text.len()).unwrap(),
        text,
    });
    let source_limit = evaluator
        .assess("query", &candidate)
        .await
        .expect_err("expected source limit");
    assert!(source_limit.to_string().contains("65536"));
    candidate.supporting.clear();
    candidate.text = "\t".repeat(MAX_FILE_BYTES);
    candidate.range = Region::from_offsets(&candidate.text, 0..candidate.text.len()).unwrap();
    let encoded_limit = evaluator
        .assess("query", &candidate)
        .await
        .expect_err("expected encoded limit");
    assert!(encoded_limit.to_string().contains("98304"));
    server.verify().await;
}
