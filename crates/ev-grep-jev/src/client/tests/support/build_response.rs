use serde_json::{Value, json};

pub(in crate::client::tests) fn build_response(model: &str) -> Value {
    json!({"model": model, "answers": {"match": {
        "type": "choice", "choice": "match", "confidence": 0.95_f64,
        "probabilities": {"match": 0.98_f64, "no_match": 0.01_f64, "uncertain": 0.01_f64}
    }}, "usage": {"input_tokens": 100_u64, "output_tokens": 5_u64}})
}
