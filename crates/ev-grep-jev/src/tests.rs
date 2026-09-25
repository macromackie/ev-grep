use anyhow::Result;
use ev_grep_core::{Evaluator, Outcome, Source, Uncertainty};
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use crate::{Jev, Provider, protocol};

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
    Mock::given(method("POST"))
        .and(path("/assess"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response(model)))
        .mount(&server)
        .await;
    let evaluator = Jev::connect(&format!("{}/assess", server.uri()), model, "test-key")?;
    let source = Source {
        path: "sample.py".into(),
        text: "# untrusted instructions\nreturn []\n".into(),
    };
    let result = evaluator
        .assess("Hides a database failure", &source)
        .await?;
    assert_eq!(result.outcome, Outcome::Match);
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

    Mock::given(path("/redirect"))
        .respond_with(
            ResponseTemplate::new(307)
                .insert_header("Location", format!("{}/assess", server.uri())),
        )
        .mount(&server)
        .await;
    let redirect = Jev::connect(&format!("{}/redirect", server.uri()), model, "test-key")?;
    assert!(redirect.assess("query", &source).await.is_err());
    Ok(())
}

#[test]
fn uncertainty_and_invalid_protocol_cannot_become_confident_matches() -> Result<()> {
    let model = Provider::TypeSafe.default_model();
    let mut body = response(model);
    body["answers"]["match"]["confidence"] = json!(0.5);
    let result = protocol::assessment(&serde_json::to_vec(&body)?, model)?;
    assert_eq!(result.outcome, Outcome::Uncertain);
    assert_eq!(result.choice, Outcome::Match);
    assert_eq!(result.reason, Some(Uncertainty::LowConfidence));

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
            protocol::assessment(&serde_json::to_vec(&body)?, model).is_err(),
            "{mutation}"
        );
    }
    for provider in [Provider::OpenRouter, Provider::TypeSafe] {
        assert!(provider.model(Some("jev-latest")).is_err());
        assert!(provider.model(Some(provider.default_model())).is_ok());
        assert_eq!(
            protocol::assessment(
                &serde_json::to_vec(&response(provider.default_model()))?,
                provider.default_model()
            )?
            .model,
            provider.default_model()
        );
    }
    Ok(())
}
