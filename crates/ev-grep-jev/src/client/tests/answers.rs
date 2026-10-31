use ev_grep_core::{Outcome, Uncertainty};
use serde_json::json;

use super::support::{build_response, set};
use crate::{FailureKind, Jev, JevError, Provider, RequestError, protocol};

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
