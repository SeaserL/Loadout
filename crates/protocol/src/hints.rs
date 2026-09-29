// SPDX-License-Identifier: Apache-2.0

//! Routing hints read from a request body without a full decode, and the minimal rewrites
//! the passthrough path applies (ADR-006 §1).

use serde_json::{Value, json};

use crate::util::{bool_field, str_field};
use crate::{Protocol, ProtocolError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteHints {
    pub model: String,
    pub stream: bool,
    pub has_tools: bool,
    pub has_images: bool,
    pub wants_reasoning: bool,
    /// Responses `previous_response_id` (stateful request, ADR-006 §3).
    pub previous_response_id: Option<String>,
    /// Responses `conversation` (also stateful).
    pub has_conversation: bool,
    /// Rough input size (bytes / 4) for context-window eligibility.
    pub estimated_input_tokens: u32,
    /// Chat only: the client asked for a usage chunk.
    pub client_wants_usage: bool,
}

impl RouteHints {
    pub fn is_stateful(&self) -> bool {
        self.previous_response_id.is_some() || self.has_conversation
    }
}

pub fn extract(
    protocol: Protocol,
    body: &Value,
    body_len: usize,
) -> Result<RouteHints, ProtocolError> {
    if !body.is_object() {
        return Err(ProtocolError::invalid("request body must be a JSON object"));
    }
    let model = str_field(body, "model")
        .filter(|m| !m.is_empty())
        .ok_or_else(|| ProtocolError::invalid("missing `model`"))?
        .to_owned();
    let has_tools = body
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|t| !t.is_empty());
    let (content_key, image_types): (&str, &[&str]) = match protocol {
        Protocol::Chat => ("messages", &["image_url"]),
        Protocol::Messages => ("messages", &["image"]),
        Protocol::Responses => ("input", &["input_image"]),
    };
    let wants_reasoning = match protocol {
        Protocol::Chat => body.get("reasoning_effort").is_some_and(|v| !v.is_null()),
        Protocol::Responses => body.get("reasoning").is_some_and(|v| !v.is_null()),
        Protocol::Messages => body
            .get("thinking")
            .and_then(|t| str_field(t, "type"))
            .is_some_and(|t| t != "disabled"),
    };
    Ok(RouteHints {
        model,
        stream: bool_field(body, "stream").unwrap_or(false),
        has_tools,
        has_images: body
            .get(content_key)
            .is_some_and(|v| contains_type(v, image_types, 0)),
        wants_reasoning,
        previous_response_id: (protocol == Protocol::Responses)
            .then(|| str_field(body, "previous_response_id").map(str::to_owned))
            .flatten(),
        has_conversation: protocol == Protocol::Responses
            && body.get("conversation").is_some_and(|v| !v.is_null()),
        estimated_input_tokens: u32::try_from(body_len / 4).unwrap_or(u32::MAX),
        client_wants_usage: protocol == Protocol::Chat
            && body
                .get("stream_options")
                .and_then(|o| bool_field(o, "include_usage"))
                .unwrap_or(false),
    })
}

/// Depth-limited search for an object whose `type` is one of `types`.
fn contains_type(v: &Value, types: &[&str], depth: u8) -> bool {
    if depth > 6 {
        return false;
    }
    match v {
        Value::Object(map) => {
            map.get("type")
                .and_then(Value::as_str)
                .is_some_and(|t| types.contains(&t))
                || map.values().any(|c| contains_type(c, types, depth + 1))
        }
        Value::Array(items) => items.iter().any(|c| contains_type(c, types, depth + 1)),
        _ => false,
    }
}

/// Rewrites a passthrough request for the upstream. Returns `true` when Loadout injected a
/// usage request the client did not ask for, so the matching usage chunk must be dropped
/// from the forwarded stream.
pub fn rewrite_for_passthrough(
    protocol: Protocol,
    body: &mut Value,
    upstream_model: &str,
    hints: &RouteHints,
) -> bool {
    body["model"] = json!(upstream_model);
    if protocol == Protocol::Chat && hints.stream && !hints.client_wants_usage {
        let opts = body
            .as_object_mut()
            .expect("checked in extract")
            .entry("stream_options")
            .or_insert_with(|| json!({}));
        if opts.is_object() {
            opts["include_usage"] = json!(true);
            return true;
        }
    }
    false
}

/// Whether a forwarded Chat chunk is the usage-only chunk Loadout injected.
pub fn is_injected_usage_chunk(data: &str) -> bool {
    if !data.contains("\"usage\"") {
        return false;
    }
    serde_json::from_str::<Value>(data).is_ok_and(|v| {
        v.get("usage").is_some_and(Value::is_object)
            && v.get("choices")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_hints_per_protocol() {
        let chat = json!({
            "model": "p/a/m", "stream": true,
            "messages": [{"role": "user", "content": [{"type": "image_url", "image_url": {"url": "x"}}]}],
            "tools": [{"type": "function", "function": {"name": "f"}}],
            "reasoning_effort": "high",
            "stream_options": {"include_usage": true}
        });
        let h = extract(Protocol::Chat, &chat, 400).expect("hints");
        assert_eq!(h.model, "p/a/m");
        assert!(
            h.stream && h.has_tools && h.has_images && h.wants_reasoning && h.client_wants_usage
        );
        assert_eq!(h.estimated_input_tokens, 100);

        let resp = json!({"model": "m", "input": "hi", "previous_response_id": "resp_1"});
        let h = extract(Protocol::Responses, &resp, 10).expect("hints");
        assert!(h.is_stateful());
        assert!(!h.has_images);

        let msgs = json!({"model": "m", "messages": [], "thinking": {"type": "disabled"}});
        assert!(
            !extract(Protocol::Messages, &msgs, 10)
                .expect("hints")
                .wants_reasoning
        );

        assert!(extract(Protocol::Chat, &json!({"messages": []}), 1).is_err());
        assert!(extract(Protocol::Chat, &json!([1]), 1).is_err());
    }

    #[test]
    fn passthrough_rewrite_only_touches_model_and_usage_option() {
        let original =
            json!({"model": "a/b/c", "stream": true, "messages": [], "zzz_custom": {"k": 1}});
        let mut body = original.clone();
        let hints = extract(Protocol::Chat, &body, 10).expect("hints");
        assert!(rewrite_for_passthrough(
            Protocol::Chat,
            &mut body,
            "c",
            &hints
        ));
        assert_eq!(body["model"], "c");
        assert_eq!(body["stream_options"]["include_usage"], true);
        assert_eq!(body["zzz_custom"], original["zzz_custom"]);
        // Key order is preserved (serde_json preserve_order).
        let keys: Vec<&String> = body.as_object().expect("obj").keys().collect();
        assert_eq!(keys[..4], ["model", "stream", "messages", "zzz_custom"]);

        let mut messages = json!({"model": "x", "stream": true, "messages": []});
        let h = extract(Protocol::Messages, &messages, 1).expect("hints");
        assert!(!rewrite_for_passthrough(
            Protocol::Messages,
            &mut messages,
            "up",
            &h
        ));
        assert!(messages.get("stream_options").is_none());
    }

    #[test]
    fn detects_injected_usage_chunk() {
        assert!(is_injected_usage_chunk(
            r#"{"choices":[],"usage":{"prompt_tokens":1}}"#
        ));
        assert!(!is_injected_usage_chunk(
            r#"{"choices":[{"delta":{}}],"usage":{"prompt_tokens":1}}"#
        ));
        assert!(!is_injected_usage_chunk(
            r#"{"choices":[{"delta":{"content":"usage"}}]}"#
        ));
        assert!(!is_injected_usage_chunk("[DONE]"));
    }
}
