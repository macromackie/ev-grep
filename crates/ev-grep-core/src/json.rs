use serde::{
    Deserializer,
    de::{Error as _, MapAccess, Visitor},
};
use serde_json::{Value, value::RawValue};

use crate::Error;

struct ObjectValues;

impl<'de> Visitor<'de> for ObjectValues {
    type Value = Vec<&'de RawValue>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("JSON object")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut object: M) -> Result<Self::Value, M::Error> {
        let mut values = Vec::new();
        while let Some(key) = object.next_key::<String>()? {
            if key == "$serde_json::private::Number" || key == "$serde_json::private::RawValue" {
                return Err(M::Error::custom(format!(
                    "reserved JSON object key {key} is not supported"
                )));
            }
            values.push(object.next_value()?);
        }
        Ok(values)
    }
}

/// Parse JSON without confusing object keys with serde's internal value representations.
/// Callers bound input bytes; nesting is limited to 128 levels here.
pub fn parse_json(bytes: &[u8]) -> Result<Value, Error> {
    let raw: &RawValue = serde_json::from_slice(bytes).map_err(Error::Json)?;
    let mut pending = vec![(raw, 0)];
    while let Some((raw, depth)) = pending.pop() {
        if depth >= 128 {
            return Err(Error::JsonDepth);
        }
        match raw.get().as_bytes().first() {
            Some(b'{') => {
                let values = serde_json::Deserializer::from_str(raw.get())
                    .deserialize_map(ObjectValues)
                    .map_err(Error::Json)?;
                pending.extend(values.into_iter().map(|value| (value, depth + 1)));
            }
            Some(b'[') => {
                let array: Vec<&RawValue> = serde_json::from_str(raw.get()).map_err(Error::Json)?;
                pending.extend(array.into_iter().map(|value| (value, depth + 1)));
            }
            _ => {}
        }
    }
    serde_json::from_slice(bytes).map_err(Error::Json)
}
