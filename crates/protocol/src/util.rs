// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use crate::ProtocolError;

pub(crate) fn str_field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

pub(crate) fn u32_field(v: &Value, key: &str) -> Option<u32> {
    v.get(key)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
}

pub(crate) fn u64_field(v: &Value, key: &str) -> Option<u64> {
    v.get(key).and_then(Value::as_u64)
}

pub(crate) fn f64_field(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}

pub(crate) fn bool_field(v: &Value, key: &str) -> Option<bool> {
    v.get(key).and_then(Value::as_bool)
}

pub(crate) fn require_str<'a>(
    v: &'a Value,
    key: &str,
    ctx: &str,
) -> Result<&'a str, ProtocolError> {
    str_field(v, key).ok_or_else(|| ProtocolError::invalid(format!("{ctx}: missing `{key}`")))
}

/// `stop` may be a string or an array of strings.
pub(crate) fn string_or_array(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

/// Parses tool-call arguments into a JSON object; malformed or non-object arguments become
/// `{}` wrapped with the raw text so nothing is silently lost.
pub(crate) fn arguments_to_object(arguments: &str) -> Value {
    if arguments.trim().is_empty() {
        return Value::Object(Default::default());
    }
    match serde_json::from_str::<Value>(arguments) {
        Ok(v @ Value::Object(_)) => v,
        Ok(other) => serde_json::json!({ "value": other }),
        Err(_) => serde_json::json!({ "_raw_arguments": arguments }),
    }
}

/// Current Unix time in seconds (for `created` fields).
pub(crate) fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Generates an id with a protocol-appropriate prefix when upstream gave none.
pub(crate) fn fallback_id(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}{:x}{:06x}", unix_now(), n & 0xff_ffff)
}
