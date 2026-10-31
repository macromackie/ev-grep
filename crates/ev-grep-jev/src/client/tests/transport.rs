use core::time::Duration;
use std::net::TcpListener;

use ev_grep_core::Evaluator as _;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

use super::support::{build_cached_source, build_response, read_kind};
use crate::{FailureKind, Jev, Provider};

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
