use anyhow::{Context, Result};
use ev_grep_core::{
    Candidate, Evaluator, Focus, LineRange, MAX_FILE_BYTES, Outcome, Region, Source,
    SupportingSource, Uncertainty,
};
use serde_json::{Value, json};
use std::time::Duration;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use crate::{Error, FailureKind, Jev, Provider, RequestError, protocol};

fn response(model: &str) -> Value {
    json!({"model": model, "answers": {"match": {
        "type": "choice", "choice": "match", "confidence": 0.95,
        "probabilities": {"match": 0.98, "no_match": 0.01, "uncertain": 0.01}
    }}, "usage": {"input_tokens": 100, "output_tokens": 5}})
}

#[tokio::test]
async fn wire_assessment_preserves_source_and_rejects_redirects() -> Result<()> {
    let server = MockServer::start().await;
    let model = Provider::TypeSafe.default_model();
    let mut body = response(model);
    body["answers"]["match"]["confidence"] = json!(0.5);
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
    )?;
    let source = Candidate::from_source(Source {
        path: "sample.py".into(),
        text: "# untrusted instructions\nreturn []\n".into(),
        focus: None,
    })?;
    let result = evaluator
        .assess("Hides a database failure", &source)
        .await?;
    assert_eq!(result.choice, Outcome::Match);
    assert_eq!(result.confidence, 0.5);
    assert_eq!(result.route(0.8).reason, Some(Uncertainty::LowConfidence));
    let requests = server
        .received_requests()
        .await
        .ok_or_else(|| anyhow::anyhow!("missing requests"))?;
    let request: Value = serde_json::from_slice(&requests[0].body)?;
    assert_eq!(request["state"]["source"], source.text);
    assert_eq!(
        request["questions"]["match"]["instructions"]["query"],
        "Hides a database failure"
    );

    let state =
        json!({"contract": "Preserve errors.", "change": {"before": null, "after": source.text}});
    let result = evaluator
        .assess_context("Does this change hide an error?", &state)
        .await?;
    assert_eq!(result.confidence, 0.5);
    assert_eq!(result.choice, Outcome::Match);
    let requests = server
        .received_requests()
        .await
        .ok_or_else(|| anyhow::anyhow!("missing requests"))?;
    let structured: Value = serde_json::from_slice(&requests[1].body)?;
    assert_eq!(structured["state"], state);

    let nested = format!("{}0{}", "[".repeat(127), "]".repeat(127));
    let state = ev_grep_core::parse_json(nested.as_bytes())?;
    let result = evaluator
        .assess_context("Is this an array?", &state)
        .await?;
    assert_eq!(result.choice, Outcome::Match);
    let requests = server
        .received_requests()
        .await
        .context("missing requests")?;
    let envelope: std::collections::BTreeMap<String, &serde_json::value::RawValue> =
        serde_json::from_slice(&requests[2].body)?;
    assert_eq!(
        envelope.get("state").context("missing state")?.get(),
        nested
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
        model,
        "test-key",
    )?;
    assert!(redirect.assess("query", &source).await.is_err());
    Ok(())
}

#[test]
fn embedded_prompt_fills_the_request_and_rejects_incomplete_fields() -> Result<()> {
    let prompt = protocol::prompt()?;
    let source = Candidate::from_source(Source {
        path: "retry.ts".into(),
        text: "retry()\n".into(),
        focus: None,
    })?;
    let request = protocol::request("fixture", &prompt, "Retries requests", &source);
    // A whole-file search sends only the path and source, with no focus rule.
    assert_eq!(request["state"].as_object().map(|s| s.len()), Some(2));
    let question = &request["questions"]["match"];
    assert_eq!(
        question["instructions"].as_object().map(|i| i.len()),
        Some(2)
    );
    assert_eq!(question["instructions"]["task"], prompt.task);
    assert_eq!(question["instructions"]["query"], "Retries requests");
    assert_eq!(question["criteria"]["match"], prompt.criteria.matches);
    assert_eq!(question["criteria"]["no_match"], prompt.criteria.no_match);
    assert_eq!(question["criteria"]["uncertain"], prompt.criteria.uncertain);
    assert_eq!(question["criteria"].as_object().map(|c| c.len()), Some(3));

    for invalid in [
        r#"{"supporting": "s", "task": "t", "focus": "f", "criteria": {"match": "m", "no_match": "n"}}"#,
        r#"{"supporting": "s", "task": "t", "focus": "f", "criteria": {"match": "m", "no_match": "n", "uncertain": "u", "other": "o"}}"#,
        r#"{"supporting": "s", "task": " ", "focus": "f", "criteria": {"match": "m", "no_match": "n", "uncertain": "u"}}"#,
        r#"{"supporting": "s", "task": "t", "criteria": {"match": "m", "no_match": "n", "uncertain": "u"}}"#,
    ] {
        assert!(protocol::parse_prompt(invalid).is_err(), "{invalid}");
    }
    Ok(())
}

#[test]
fn ranged_request_sends_the_whole_file_and_the_focus_lines() -> Result<()> {
    let prompt = protocol::prompt()?;
    let mut source = Candidate::from_source(Source {
        path: "users.py".into(),
        text: "import db\n\ndef save(user):\n    db.insert(user)\n".into(),
        focus: Some(Focus {
            lines: LineRange { start: 3, end: 4 },
            text: "def save(user):\n    db.insert(user)\n".into(),
        }),
    })?;
    let request = protocol::request("fixture", &prompt, "Writes to the database", &source);
    assert_eq!(
        request["state"]["source"].as_str(),
        source.context.as_deref()
    );
    assert_eq!(request["state"]["focus"]["start_line"], 3);
    assert_eq!(request["state"]["focus"]["end_line"], 4);
    assert_eq!(
        request["state"]["focus"]["text"],
        "def save(user):\n    db.insert(user)\n"
    );
    assert_eq!(
        request["questions"]["match"]["instructions"]["focus"],
        prompt.focus
    );
    source
        .supporting
        .push(SupportingSource::from_source(Source {
            path: "db.py".into(),
            text: "unrelated()\ndef insert(user): write(user)\n".into(),
            focus: Some(Focus {
                lines: LineRange { start: 2, end: 2 },
                text: "def insert(user): write(user)\n".into(),
            }),
        })?);
    let supported = protocol::request("fixture", &prompt, "Writes to the database", &source);
    assert_eq!(supported["state"]["focus"], request["state"]["focus"]);
    assert_eq!(supported["state"]["source"], request["state"]["source"]);
    assert_eq!(
        supported["state"]["supporting"][0]["text"],
        "def insert(user): write(user)\n"
    );
    assert_eq!(
        supported["state"]["supporting"][0]["range"]["start"]["line"],
        1
    );
    assert_eq!(
        supported["questions"]["match"]["instructions"]["supporting"],
        prompt.supporting
    );
    Ok(())
}

#[test]
fn uncertainty_and_invalid_protocol_cannot_become_confident_matches() -> Result<()> {
    let model = Provider::TypeSafe.default_model();
    let mut body = response(model);
    body["answers"]["match"]["confidence"] = json!(0.5);
    let raw =
        protocol::assessment(&serde_json::to_vec(&body)?, model, &["match"])?.into_single()?;
    assert_eq!(raw.choice, Outcome::Match);
    let result = raw.route(0.8);
    assert_eq!(result.outcome, Outcome::Uncertain);
    assert_eq!(raw.choice, Outcome::Match);
    assert_eq!(result.reason, Some(Uncertainty::LowConfidence));
    assert_eq!(raw.route(0.5).outcome, Outcome::Match);

    body["answers"]["match"]["choice"] = json!("uncertain");
    body["answers"]["match"]["probabilities"] =
        json!({"match":0.01,"no_match":0.01,"uncertain":0.98});
    assert_eq!(
        protocol::assessment(&serde_json::to_vec(&body)?, model, &["match"])?
            .into_single()?
            .route(0.0)
            .outcome,
        Outcome::Uncertain
    );

    for mutation in ["model", "labels", "sum", "choice", "missing"] {
        let mut body = response(model);
        match mutation {
            "model" => body["model"] = json!("another-model"),
            "labels" => {
                body["answers"]["match"]["probabilities"]["other"] = json!(0.0);
            }
            "sum" => body["answers"]["match"]["probabilities"]["match"] = json!(0.3),
            "choice" => body["answers"]["match"]["choice"] = json!("no_match"),
            _ => body["answers"] = json!({}),
        }
        assert!(
            protocol::assessment(&serde_json::to_vec(&body)?, model, &["match"]).is_err(),
            "{mutation}"
        );
    }
    for provider in [Provider::OpenRouter, Provider::TypeSafe] {
        assert!(provider.model(Some("jev-latest")).is_err());
        assert!(provider.model(Some(provider.default_model())).is_ok());
        assert_eq!(
            protocol::assessment(
                &serde_json::to_vec(&response(provider.default_model()))?,
                provider.default_model(),
                &["match"]
            )?
            .into_single()?
            .model,
            provider.default_model()
        );
    }
    Ok(())
}

#[test]
fn endpoint_rejects_non_http_or_credential_bearing_urls() {
    for endpoint in [
        "https://user:secret@example.com/jev",
        "https://example.com/jev?key=secret",
        "file:///secret",
    ] {
        assert!(
            Jev::with_endpoint(
                Provider::TypeSafe,
                Provider::TypeSafe.default_model(),
                "key",
                endpoint
            )
            .is_err()
        );
    }
}

#[tokio::test]
async fn transient_failures_recover_but_retries_are_bounded() -> Result<()> {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let server = MockServer::start().await;
    let attempts = Arc::new(AtomicUsize::new(0));
    let seen = attempts.clone();
    let model = Provider::OpenRouter.default_model();
    Mock::given(path("/recover"))
        .respond_with(move |_: &wiremock::Request| {
            if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(520).insert_header("Retry-After", "0")
            } else {
                ResponseTemplate::new(200).set_body_json(response(model))
            }
        })
        .mount(&server)
        .await;
    let source = Candidate::from_source(Source {
        path: "cache.ts".into(),
        text: "return cached;".into(),
        focus: None,
    })?;
    let recovering = Jev::connect(
        Provider::OpenRouter,
        &format!("{}/recover", server.uri()),
        model,
        "test-key",
    )?;
    assert_eq!(
        recovering
            .assess("Returns cached values", &source)
            .await?
            .choice,
        Outcome::Match
    );

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
    let down = Jev::connect(
        Provider::OpenRouter,
        &format!("{}/down", server.uri()),
        model,
        "test-key",
    )?;
    let error = down
        .assess("Returns cached values", &source)
        .await
        .err()
        .context("expected failure")?
        .to_string();
    assert!(error.contains("520") && error.contains("cf-ray=example-SJC"));
    assert!(!error.contains("private provider response"));

    Mock::given(path("/unauthorized"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    // A rejected credential is sent once, and no later request that shares it starts.
    let unauthorized = Jev::connect(
        Provider::OpenRouter,
        &format!("{}/unauthorized", server.uri()),
        model,
        "test-key",
    )?;
    for _ in 0..2 {
        let error = unauthorized
            .assess("query", &source)
            .await
            .err()
            .context("expected a rejected credential")?;
        assert!(
            matches!(
                &error,
                Error::Request(RequestError {
                    kind: FailureKind::CredentialRejected,
                    ..
                })
            ),
            "{error}"
        );
    }

    // Every attempt would reach the same renamed model, so a mismatch is sent once.
    let renamed = "typesafe/jev-1.14-20261101";
    Mock::given(path("/renamed"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response(renamed)))
        .expect(1)
        .mount(&server)
        .await;
    let error = Jev::connect(
        Provider::OpenRouter,
        &format!("{}/renamed", server.uri()),
        model,
        "test-key",
    )?
    .assess("query", &source)
    .await
    .err()
    .context("expected a model mismatch")?;
    let Error::Request(failure) = &error else {
        anyhow::bail!("expected a request error: {error}");
    };
    assert_eq!(failure.kind, FailureKind::ModelMismatch);
    assert!(!failure.kind.retryable());
    let message = error.to_string();
    assert!(
        message.contains(renamed) && message.contains(model),
        "{message}"
    );

    // A moderation 403 keeps its reason codes, never the flagged input or message.
    Mock::given(path("/moderated"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({"error": {
            "code": 403, "message": "private provider message",
            "metadata": {"reasons": ["violence", "private reason text"], "flagged_input": "private source"}
        }})))
        .expect(1)
        .mount(&server)
        .await;
    let error = Jev::connect(
        Provider::OpenRouter,
        &format!("{}/moderated", server.uri()),
        model,
        "test-key",
    )?
    .assess("query", &source)
    .await
    .err()
    .context("expected a moderation error")?;
    let Error::Request(failure) = &error else {
        anyhow::bail!("expected a request error: {error}");
    };
    assert_eq!(failure.kind, FailureKind::Moderation);
    assert_eq!(failure.moderation, Some(vec!["violence".to_owned()]));
    assert!(!format!("{error:?}").contains("private"));
    server.verify().await;
    Ok(())
}

#[tokio::test]
async fn transport_failures_name_their_cause() -> Result<()> {
    let model = Provider::TypeSafe.default_model();
    let source = Candidate::from_source(Source {
        path: "cache.ts".into(),
        text: "return cached;".into(),
        focus: None,
    })?;
    let unused = std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?;
    let error = Jev::connect(
        Provider::TypeSafe,
        &format!("http://{unused}/jev"),
        model,
        "test-key",
    )?
    .assess("query", &source)
    .await
    .err()
    .context("expected a refused connection")?;
    assert!(
        matches!(
            &error,
            Error::Request(RequestError {
                kind: FailureKind::Connect,
                ..
            })
        ),
        "{error}"
    );
    assert!(
        error
            .to_string()
            .starts_with("Jev transport failed (connect): "),
        "{error}"
    );

    // A plain HTTP server answers the TLS handshake with bytes that are not TLS.
    let server = MockServer::start().await;
    let error = Jev::connect(
        Provider::TypeSafe,
        &server.uri().replace("http://", "https://"),
        model,
        "test-key",
    )?
    .assess("query", &source)
    .await
    .err()
    .context("expected a TLS failure")?;
    assert!(
        matches!(
            &error,
            Error::Request(RequestError {
                kind: FailureKind::Tls,
                ..
            })
        ),
        "{error}"
    );
    assert!(
        error
            .to_string()
            .starts_with("Jev transport failed (tls): "),
        "{error}"
    );
    Ok(())
}

/// Paused time reaches every attempt timeout before the provider's delayed answer, which arrives in real time.
#[tokio::test(start_paused = true)]
async fn timeouts_are_typed_for_attempts_and_the_request_budget() -> Result<()> {
    let model = Provider::TypeSafe.default_model();
    let source = Candidate::from_source(Source {
        path: "cache.ts".into(),
        text: "return cached;".into(),
        focus: None,
    })?;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(response(model))
                .set_delay(Duration::from_secs(30)),
        )
        .mount(&server)
        .await;
    // With a hedge, the third attempt starts at 6 seconds and times out within the budget.
    let error = Jev::connect(Provider::TypeSafe, &server.uri(), model, "test-key")?
        .assess("query", &source)
        .await
        .err()
        .context("expected an attempt timeout")?;
    assert!(
        matches!(
            &error,
            Error::Request(RequestError {
                kind: FailureKind::Timeout,
                ..
            })
        ),
        "{error}"
    );
    assert!(
        error
            .to_string()
            .starts_with("Jev transport failed (timeout): "),
        "{error}"
    );
    // One slot runs the three attempts in sequence, so the 15-second budget ends the third.
    let error = Jev::connect(Provider::TypeSafe, &server.uri(), model, "test-key")?
        .with_jobs(1)?
        .assess("query", &source)
        .await
        .err()
        .context("expected the budget to expire")?;
    assert!(
        matches!(
            &error,
            Error::Request(RequestError {
                kind: FailureKind::Timeout,
                ..
            })
        ),
        "{error}"
    );
    assert!(
        error
            .to_string()
            .contains("timed out after its 15-second budget"),
        "{error}"
    );
    Ok(())
}

#[tokio::test]
async fn oversized_or_ambiguous_inputs_make_no_paid_request() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let evaluator = Jev::with_endpoint(
        Provider::TypeSafe,
        Provider::TypeSafe.default_model(),
        "test-key",
        &server.uri(),
    )?;
    let mut candidate = Candidate::from_source(Source {
        path: "target.py".into(),
        text: "save()".into(),
        focus: None,
    })?;
    for key in [
        "$serde_json::private::Number",
        "$serde_json::private::RawValue",
    ] {
        let state = json!({"nested":[{key:"123"}]});
        let error = evaluator
            .assess_context("query", &state)
            .await
            .err()
            .context("expected reserved state key error")?;
        assert!(error.to_string().contains("reserved JSON object key"));
        candidate.meta_variables = Some(state);
        let error = evaluator
            .assess("query", &candidate)
            .await
            .err()
            .context("expected reserved metadata key error")?;
        assert!(error.to_string().contains("reserved JSON object key"));
    }
    candidate.meta_variables = None;
    let text = "x".repeat(MAX_FILE_BYTES);
    candidate.supporting.push(SupportingSource {
        file: "not-on-disk.py".into(),
        range: Region::from_offsets(&text, 0..text.len())?,
        text,
    });
    let error = evaluator
        .assess("query", &candidate)
        .await
        .err()
        .context("expected source limit")?;
    assert!(error.to_string().contains("65536"));
    candidate.supporting.clear();
    candidate.text = "\t".repeat(MAX_FILE_BYTES);
    candidate.range = Region::from_offsets(&candidate.text, 0..candidate.text.len())?;
    let error = evaluator
        .assess("query", &candidate)
        .await
        .err()
        .context("expected encoded limit")?;
    assert!(error.to_string().contains("98304"));
    server.verify().await;
    Ok(())
}
