use serde_json::Value;

pub(in crate::client::tests) fn set(value: &mut Value, pointer: &str, replacement: Value) {
    *value.pointer_mut(pointer).unwrap() = replacement;
}
