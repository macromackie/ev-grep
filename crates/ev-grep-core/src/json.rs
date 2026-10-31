use core::fmt;

use serde::{
    Deserializer as _,
    de::{Error as _, MapAccess, Visitor},
};
use serde_json::{Value, value::RawValue};

use crate::InputError;

const RESERVED_OBJECT_KEYS: &[&str] = &[
    "$serde_json::private::Number",
    "$serde_json::private::RawValue",
];

struct ObjectValues;

impl<'de> Visitor<'de> for ObjectValues {
    type Value = Vec<&'de RawValue>;

    #[inline]
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON object")
    }

    #[inline]
    fn visit_map<Access>(self, mut object: Access) -> Result<Self::Value, Access::Error>
    where
        Access: MapAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(key) = object.next_key::<String>()? {
            if RESERVED_OBJECT_KEYS.contains(&key.as_str()) {
                return Err(Access::Error::custom(format!(
                    "reserved JSON object key {key:?} is not supported"
                )));
            }
            values.push(object.next_value()?);
        }
        Ok(values)
    }
}

/// Parse JSON without confusing object keys with serde's internal value representations.
/// Callers bound input bytes; nesting is limited to 128 levels here.
#[inline]
pub fn parse_json(bytes: &[u8]) -> Result<Value, InputError> {
    let raw: &RawValue = serde_json::from_slice(bytes).map_err(InputError::Json)?;
    let mut pending = vec![(raw, 0_u32)];
    while let Some((node, depth)) = pending.pop() {
        if depth >= 128_u32 {
            return Err(InputError::JsonDepth);
        }
        match node.get().as_bytes().first() {
            Some(b'{') => {
                let values = serde_json::Deserializer::from_str(node.get())
                    .deserialize_map(ObjectValues)
                    .map_err(InputError::Json)?;
                pending.extend(
                    values
                        .into_iter()
                        .map(|value| (value, depth.saturating_add(1))),
                );
            },
            Some(b'[') => {
                let array: Vec<&RawValue> =
                    serde_json::from_str(node.get()).map_err(InputError::Json)?;
                pending.extend(
                    array
                        .into_iter()
                        .map(|value| (value, depth.saturating_add(1))),
                );
            },
            _ => {},
        }
    }
    serde_json::from_slice(bytes).map_err(InputError::Json)
}
