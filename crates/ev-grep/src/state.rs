use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the field order is the order of the record's JSON keys"
)]
pub(crate) struct StateInput {
    pub state: Value,
    pub input_id: String,
    pub bytes: usize,
}
