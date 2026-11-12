//! The stored form of the template stores: one JSON object per key in the UI storage, read over
//! its defaults so that a value saved by an older version keeps the fields it lacks.

use serde::Serialize;
use serde_json::{Map, Value};

/// A value to put in the UI storage.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct StorageWrite {
    pub key: String,
    pub value: String,
}

impl StorageWrite {
    /// The write storing `value` as JSON under `key`.
    pub fn json<T: Serialize>(key: impl Into<String>, value: &T) -> Self {
        // A value that cannot be written as JSON is stored as an empty object, which reads as the defaults.
        let value = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_owned());
        Self { key: key.into(), value }
    }
}

/// The object stored as `raw` laid over `fallback`: stored fields win, missing ones come from the
/// fallback. Nothing stored, text that is not JSON, and JSON that is not an object give the fallback.
pub fn load_object(raw: Option<&str>, fallback: Map<String, Value>) -> Map<String, Value> {
    let Some(raw) = raw.filter(|raw| !raw.is_empty()) else {
        return fallback;
    };
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Object(stored)) => {
            let mut merged = fallback;
            merged.extend(stored);
            merged
        }
        _ => fallback,
    }
}

/// A boolean field of a stored object; `default` when it is missing or not a boolean.
pub fn bool_field(object: &Map<String, Value>, key: &str, default: bool) -> bool {
    object.get(key).and_then(Value::as_bool).unwrap_or(default)
}

/// The strings of an array field of a stored object, in order and without repeats.
pub fn string_list(object: &Map<String, Value>, key: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in object.get(key).and_then(Value::as_array).into_iter().flatten() {
        if let Some(text) = item.as_str()
            && !out.iter().any(|seen| seen == text)
        {
            out.push(text.to_owned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap_or_default()
    }

    // Not from a spec: stored fields win over the defaults, and anything unreadable gives the defaults.
    #[test]
    fn stored_objects_are_read_over_their_defaults() {
        let fallback = object(json!({ "enabled": true, "disabled": [] }));
        assert_eq!(load_object(None, fallback.clone()), fallback);
        assert_eq!(load_object(Some(""), fallback.clone()), fallback);
        assert_eq!(load_object(Some("{ not json"), fallback.clone()), fallback);
        assert_eq!(load_object(Some("[1, 2]"), fallback.clone()), fallback);
        let merged = load_object(Some(r#"{"enabled":false,"extra":1}"#), fallback);
        assert_eq!(Value::Object(merged), json!({ "enabled": false, "disabled": [], "extra": 1 }));
    }

    // Not from a spec: fields of the wrong type read as their default, lists drop non-strings and repeats.
    #[test]
    fn fields_of_the_wrong_type_read_as_their_default() {
        let stored = object(json!({ "on": "yes", "off": false, "ids": ["a", 3, "a", "b"] }));
        assert!(bool_field(&stored, "on", true));
        assert!(!bool_field(&stored, "off", true));
        assert!(bool_field(&stored, "missing", true));
        assert_eq!(string_list(&stored, "ids"), ["a", "b"]);
        assert!(string_list(&stored, "on").is_empty());
        let write = StorageWrite::json("k", &json!({ "a": 1 }));
        assert_eq!(write, StorageWrite { key: "k".to_owned(), value: r#"{"a":1}"#.to_owned() });
    }
}
