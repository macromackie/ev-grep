use alloc::collections::BTreeMap;
use core::time::Duration;
use std::net::TcpListener;

use ev_grep_core::{
    Candidate,
    Evaluator as _,
    Focus,
    LineRange,
    MAX_FILE_BYTES,
    Outcome,
    Region,
    Source,
    SupportingSource,
    Uncertainty,
};
use serde_json::{Map, Value, json, value::RawValue};
use wiremock::{
    Mock,
    MockServer,
    ResponseTemplate,
    matchers::{method, path},
};

use crate::{FailureKind, Jev, JevError, Provider, RequestError, protocol};

fn build_response(model: &str) -> Value {
    json!({"model": model, "answers": {"match": {
        "type": "choice", "choice": "match", "confidence": 0.95_f64,
        "probabilities": {"match": 0.98_f64, "no_match": 0.01_f64, "uncertain": 0.01_f64}
    }}, "usage": {"input_tokens": 100_u64, "output_tokens": 5_u64}})
}

/// The value at a JSON pointer, or null when it is missing, as `Value`'s index reads it.
fn read_at<'value>(value: &'value Value, pointer: &str) -> &'value Value {
    value.pointer(pointer).unwrap_or(&Value::Null)
}

fn set(value: &mut Value, pointer: &str, replacement: Value) {
    *value.pointer_mut(pointer).unwrap() = replacement;
}

fn request_failure(error: &JevError) -> Option<&RequestError> {
    if let JevError::Request(failure) = error {
        Some(failure)
    } else {
        None
    }
}

fn read_kind(error: &JevError) -> Option<FailureKind> {
    request_failure(error).map(|failure| failure.kind)
}

fn build_cached_source() -> Candidate {
    Candidate::from_source(Source {
        path: "cache.ts".into(),
        text: "return cached;".into(),
        focus: None,
    })
    .unwrap()
}

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
    let envelope: BTreeMap<String, &RawValue> = serde_json::from_slice(&body).unwrap();
    assert_eq!(envelope.get("state").expect("missing state").get(), nested);
}

#[test]
fn embedded_prompt_fills_the_request() {
    let prompt = protocol::prompt().unwrap();
    let source = Candidate::from_source(Source {
        path: "retry.ts".into(),
        text: "retry()\n".into(),
        focus: None,
    })
    .unwrap();
    let request = protocol::request("fixture", &prompt, "Retries requests", &source);
    // A whole-file search sends only the path and source, with no focus rule.
    assert_eq!(
        read_at(&request, "/state").as_object().map(Map::len),
        Some(2)
    );
    let question = read_at(&request, "/questions/match");
    assert_eq!(
        read_at(question, "/instructions").as_object().map(Map::len),
        Some(2)
    );
    assert_eq!(read_at(question, "/instructions/task"), &json!(prompt.task));
    assert_eq!(read_at(question, "/instructions/query"), "Retries requests");
    assert_eq!(
        read_at(question, "/criteria/match"),
        &json!(prompt.criteria.matches)
    );
    assert_eq!(
        read_at(question, "/criteria/no_match"),
        &json!(prompt.criteria.no_match)
    );
    assert_eq!(
        read_at(question, "/criteria/uncertain"),
        &json!(prompt.criteria.uncertain)
    );
    assert_eq!(
        read_at(question, "/criteria").as_object().map(Map::len),
        Some(3)
    );
}

#[test]
fn incomplete_prompts_are_rejected() {
    for invalid in [
        concat!(
            r#"{"supporting": "s", "task": "t", "focus": "f", "#,
            r#""criteria": {"match": "m", "no_match": "n"}}"#,
        ),
        concat!(
            r#"{"supporting": "s", "task": "t", "focus": "f", "#,
            r#""criteria": {"match": "m", "no_match": "n", "uncertain": "u", "other": "o"}}"#,
        ),
        concat!(
            r#"{"supporting": "s", "task": " ", "focus": "f", "#,
            r#""criteria": {"match": "m", "no_match": "n", "uncertain": "u"}}"#,
        ),
        concat!(
            r#"{"supporting": "s", "task": "t", "#,
            r#""criteria": {"match": "m", "no_match": "n", "uncertain": "u"}}"#,
        ),
    ] {
        assert!(
            matches!(
                protocol::parse_prompt(invalid),
                Err(JevError::Prompt(_) | JevError::EmptyPrompt)
            ),
            "{invalid:?}"
        );
    }
}

/// `users.py` with its `save` function in focus.
fn build_users_source() -> Candidate {
    Candidate::from_source(Source {
        path: "users.py".into(),
        text: "import db\n\ndef save(user):\n    db.insert(user)\n".into(),
        focus: Some(Focus {
            lines: LineRange { end: 4, start: 3 },
            text: "def save(user):\n    db.insert(user)\n".into(),
        }),
    })
    .unwrap()
}

#[test]
fn ranged_request_sends_the_whole_file_and_the_focus_lines() {
    let prompt = protocol::prompt().unwrap();
    let source = build_users_source();
    let request = protocol::request("fixture", &prompt, "Writes to the database", &source);
    assert_eq!(
        read_at(&request, "/state/source").as_str(),
        source.context.as_deref()
    );
    assert_eq!(read_at(&request, "/state/focus/start_line"), 3_u64);
    assert_eq!(read_at(&request, "/state/focus/end_line"), 4_u64);
    assert_eq!(
        read_at(&request, "/state/focus/text"),
        "def save(user):\n    db.insert(user)\n"
    );
    assert_eq!(
        read_at(&request, "/questions/match/instructions/focus"),
        &json!(prompt.focus)
    );
}

#[test]
fn supporting_sources_join_the_ranged_request() {
    let prompt = protocol::prompt().unwrap();
    let mut source = build_users_source();
    let request = protocol::request("fixture", &prompt, "Writes to the database", &source);
    source.supporting.push(
        SupportingSource::from_source(Source {
            path: "db.py".into(),
            text: "unrelated()\ndef insert(user): write(user)\n".into(),
            focus: Some(Focus {
                lines: LineRange { end: 2, start: 2 },
                text: "def insert(user): write(user)\n".into(),
            }),
        })
        .unwrap(),
    );
    let supported = protocol::request("fixture", &prompt, "Writes to the database", &source);
    assert_eq!(
        read_at(&supported, "/state/focus"),
        read_at(&request, "/state/focus")
    );
    assert_eq!(
        read_at(&supported, "/state/source"),
        read_at(&request, "/state/source")
    );
    assert_eq!(
        read_at(&supported, "/state/supporting/0/text"),
        "def insert(user): write(user)\n"
    );
    assert_eq!(
        read_at(&supported, "/state/supporting/0/range/start/line"),
        1_u64
    );
    assert_eq!(
        read_at(&supported, "/questions/match/instructions/supporting"),
        &json!(prompt.supporting)
    );
}

#[test]
fn uncertainty_and_invalid_protocol_cannot_become_confident_matches() {
    let model = Provider::TypeSafe.read_default_model();
    let mut body = build_response(model);
    set(&mut body, "/answers/match/confidence", json!(0.5_f64));
    let raw = protocol::parse_assessment(&serde_json::to_vec(&body).unwrap(), model, &["match"])
        .unwrap()
        .into_single()
        .unwrap();
    assert_eq!(raw.choice, Outcome::Match);
    let routed = raw.route(0.8);
    assert_eq!(routed.outcome, Outcome::Uncertain);
    assert_eq!(raw.choice, Outcome::Match);
    assert_eq!(routed.reason, Some(Uncertainty::LowConfidence));
    assert_eq!(raw.route(0.5).outcome, Outcome::Match);

    set(&mut body, "/answers/match/choice", json!("uncertain"));
    set(
        &mut body,
        "/answers/match/probabilities",
        json!({"match":0.01_f64,"no_match":0.01_f64,"uncertain":0.98_f64}),
    );
    assert_eq!(
        protocol::parse_assessment(&serde_json::to_vec(&body).unwrap(), model, &["match"])
            .unwrap()
            .into_single()
            .unwrap()
            .route(0.0)
            .outcome,
        Outcome::Uncertain
    );
}

#[test]
fn invalid_answers_and_unsupported_models_are_rejected() {
    let model = Provider::TypeSafe.read_default_model();
    for (pointer, replacement) in [
        ("/model", json!("another-model")),
        (
            "/answers/match/probabilities",
            json!({"match":0.98_f64,"no_match":0.01_f64,"uncertain":0.01_f64,"other":0.0_f64}),
        ),
        ("/answers/match/probabilities/match", json!(0.3_f64)),
        ("/answers/match/choice", json!("no_match")),
        ("/answers", json!({})),
    ] {
        let mut body = build_response(model);
        set(&mut body, pointer, replacement);
        assert!(
            matches!(
                protocol::parse_assessment(&serde_json::to_vec(&body).unwrap(), model, &["match"]),
                Err(RequestError {
                    kind: FailureKind::InvalidResponse | FailureKind::ModelMismatch,
                    ..
                })
            ),
            "{pointer:?}"
        );
    }
    for provider in [Provider::OpenRouter, Provider::TypeSafe] {
        assert!(matches!(
            provider.select_model(Some("jev-latest")),
            Err(JevError::UnsupportedModel { .. })
        ));
        assert_eq!(
            provider
                .select_model(Some(provider.read_default_model()))
                .unwrap(),
            provider.read_default_model()
        );
        assert_eq!(
            protocol::parse_assessment(
                &serde_json::to_vec(&build_response(provider.read_default_model())).unwrap(),
                provider.read_default_model(),
                &["match"]
            )
            .unwrap()
            .into_single()
            .unwrap()
            .model,
            provider.read_default_model()
        );
    }
}

#[test]
fn endpoint_rejects_non_http_or_credential_bearing_urls() {
    for endpoint in [
        "https://user:secret@example.com/jev",
        "https://example.com/jev?key=secret",
        "file:///secret",
    ] {
        assert!(matches!(
            Jev::with_endpoint(
                Provider::TypeSafe,
                Provider::TypeSafe.read_default_model(),
                "key",
                endpoint
            ),
            Err(JevError::UnsupportedEndpoint)
        ));
    }
}

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

#[tokio::test]
async fn transport_failures_name_their_cause() {
    let model = Provider::TypeSafe.read_default_model();
    let source = build_cached_source();
    let unused = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let refused = Jev::connect(
        Provider::TypeSafe,
        &format!("http://{unused}/jev"),
        model,
        "test-key",
    )
    .unwrap()
    .assess("query", &source)
    .await
    .expect_err("expected a refused connection");
    assert_eq!(
        read_kind(&refused),
        Some(FailureKind::Connect),
        "{refused:?}"
    );
    assert!(
        refused
            .to_string()
            .starts_with("Jev transport failed (connect): "),
        "{refused:?}"
    );

    // A plain HTTP server answers the TLS handshake with bytes that are not TLS.
    let server = MockServer::start().await;
    let handshake = Jev::connect(
        Provider::TypeSafe,
        &server.uri().replace("http://", "https://"),
        model,
        "test-key",
    )
    .unwrap()
    .assess("query", &source)
    .await
    .expect_err("expected a TLS failure");
    assert_eq!(
        read_kind(&handshake),
        Some(FailureKind::Tls),
        "{handshake:?}"
    );
    assert!(
        handshake
            .to_string()
            .starts_with("Jev transport failed (tls): "),
        "{handshake:?}"
    );
}

/// Paused time reaches every attempt timeout before the provider's delayed answer, which arrives in
/// real time.
#[tokio::test(start_paused = true)]
async fn timeouts_are_typed_for_attempts_and_the_request_budget() {
    let model = Provider::TypeSafe.read_default_model();
    let source = build_cached_source();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(build_response(model))
                .set_delay(Duration::from_secs(30)),
        )
        .mount(&server)
        .await;
    let slow = Jev::connect(Provider::TypeSafe, &server.uri(), model, "test-key").unwrap();
    // With a hedge, the third attempt starts at 6 seconds and times out within the budget.
    let attempt = slow
        .assess("query", &source)
        .await
        .expect_err("expected an attempt timeout");
    assert_eq!(
        read_kind(&attempt),
        Some(FailureKind::Timeout),
        "{attempt:?}"
    );
    assert!(
        attempt
            .to_string()
            .starts_with("Jev transport failed (timeout): "),
        "{attempt:?}"
    );
    // One slot runs the three attempts in sequence, so the 15-second budget ends the third.
    let budget = slow
        .with_jobs(1)
        .unwrap()
        .assess("query", &source)
        .await
        .expect_err("expected the budget to expire");
    assert_eq!(read_kind(&budget), Some(FailureKind::Timeout), "{budget:?}");
    assert!(
        budget
            .to_string()
            .contains("timed out after its 15-second budget"),
        "{budget:?}"
    );
}

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
