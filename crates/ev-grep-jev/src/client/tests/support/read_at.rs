use serde_json::Value;

/// The value at a JSON pointer, or null when it is missing, as `Value`'s index reads it.
pub(in crate::client::tests) fn read_at<'value>(
    value: &'value Value,
    pointer: &str,
) -> &'value Value {
    value.pointer(pointer).unwrap_or(&Value::Null)
}
