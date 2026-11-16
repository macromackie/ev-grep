use ev_grep_core::{ContextProfile, Evaluator as _, Question};
use serde_json::{Map, Value, json};
use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers::method};

use super::support::build_response;
use crate::{Jev, JevError, MAX_QUESTIONS, Provider};

async fn serve() -> (MockServer, Jev) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(|request: &Request| {
            let sent: Value = serde_json::from_slice(&request.body).unwrap();
            let mut response = build_response(Provider::TypeSafe.read_default_model());
            let answer = response.pointer("/answers/match").unwrap().clone();
            let answers: Map<String, Value> = sent
                .get("questions")
                .unwrap()
                .as_object()
                .unwrap()
                .keys()
                .map(|id| (id.clone(), answer.clone()))
                .collect();
            *response.get_mut("answers").unwrap() = Value::Object(answers);
            ResponseTemplate::new(200).set_body_json(response)
        })
        .mount(&server)
        .await;
    let client = Jev::with_endpoint(
        Provider::TypeSafe,
        Provider::TypeSafe.read_default_model(),
        "test-key",
        &server.uri(),
    )
    .unwrap();
    (server, client)
}

#[tokio::test]
async fn large_context_requires_opt_in_and_measurement_matches_the_wire() {
    let (server, standard) = serve().await;
    let state = json!({"escaped": "\t\n\"\\", "source": "x".repeat(128 * 1024)});
    let questions = [Question {
        id: "match",
        query: "Is the source supplied?",
    }];
    assert!(matches!(
        standard.assess_questions(&questions, &state).await,
        Err(JevError::StateTooLarge {
            limit_bytes: 98304,
            ..
        })
    ));
    assert!(server.received_requests().await.unwrap().is_empty());
    let long = standard.with_context_profile(ContextProfile::Long);
    let measured = long.measure_questions(&questions, &state).unwrap();
    let result = long.assess_questions(&questions, &state).await.unwrap();
    assert_eq!(result.answers.len(), 1);
    assert_eq!(result.request.input_tokens, 100);
    let requests = server.received_requests().await.unwrap();
    let request = requests.first().unwrap();
    assert_eq!(request.body.len(), measured);
    assert_eq!(
        request
            .headers
            .get("typesafe-danger-zone-long-context")
            .unwrap(),
        "true"
    );
    let sent: Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(sent.get("state").unwrap(), &state);
}

#[tokio::test]
async fn bounded_batches_preserve_the_named_answer_set_and_single_usage_record() {
    let (server, client) = serve().await;
    let ids: Vec<String> = (0_usize..129).map(|index| format!("q{index}")).collect();
    let questions: Vec<Question<'_>> = ids
        .iter()
        .map(|id| Question { id, query: "True?" })
        .collect();
    let batch = client
        .assess_questions(
            questions.get(..128).expect("generated 129 questions"),
            &json!({}),
        )
        .await
        .unwrap();
    assert_eq!(batch.answers.len(), 128);
    assert_eq!(batch.request.input_tokens, 100);
    assert!(
        batch
            .answers
            .values()
            .all(|answer| answer.request.is_none())
    );
    assert!(matches!(
        client.assess_questions(&questions, &json!({})).await,
        Err(JevError::QuestionCount)
    ));
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert!(
        !requests
            .first()
            .unwrap()
            .headers
            .contains_key("typesafe-danger-zone-long-context")
    );
}

#[tokio::test]
async fn long_context_remains_bounded_without_truncating_state() {
    let (server, client) = serve().await;
    let state = json!({"source": "x".repeat(ContextProfile::Long.read_max_state_bytes())});
    let error = client
        .with_context_profile(ContextProfile::Long)
        .assess_context("Supplied?", &state)
        .await
        .unwrap_err();
    assert!(
        matches!(error, JevError::StateTooLarge { bytes, limit_bytes }
        if bytes > limit_bytes && limit_bytes == 1024 * 1024)
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn long_context_rejects_combined_wire_overflow_before_sending() {
    let (server, client) = serve().await;
    let source = "x".repeat(
        ContextProfile::Long
            .read_max_state_bytes()
            .saturating_sub(64),
    );
    let state = json!({"source": source});
    let ids: Vec<String> = (0..MAX_QUESTIONS)
        .map(|index| format!("q{index}"))
        .collect();
    let query = "x".repeat(ev_grep_core::MAX_QUERY_BYTES);
    let questions: Vec<Question<'_>> = ids
        .iter()
        .map(|id| Question { id, query: &query })
        .collect();
    let error = client
        .with_context_profile(ContextProfile::Long)
        .assess_questions(&questions, &state)
        .await
        .unwrap_err();
    assert!(
        matches!(error, JevError::RequestTooLarge { bytes, limit_bytes }
        if bytes > limit_bytes && limit_bytes == 2 * 1024 * 1024)
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}
