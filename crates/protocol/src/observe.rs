// SPDX-License-Identifier: Apache-2.0

//! Helpers for the passthrough path, which forwards upstream bytes unchanged and only
//! observes them.

use bytes::Bytes;
use serde_json::{Value, json};

use crate::sse::{encode, encode_json};
use crate::{Protocol, Usage, chat, messages, responses};

/// A standalone error event in `protocol`'s streaming format, for when an upstream stream
/// breaks after bytes were already sent to the client.
pub fn error_event(protocol: Protocol, message: &str) -> Bytes {
    match protocol {
        Protocol::Chat => encode_json(
            None,
            &json!({ "error": { "message": message, "type": "upstream_error" } }),
        ),
        Protocol::Messages => encode_json(
            Some("error"),
            &json!({ "type": "error", "error": { "type": "api_error", "message": message } }),
        ),
        Protocol::Responses => encode_json(
            Some("error"),
            &json!({ "type": "error", "code": "upstream_error", "message": message, "param": null }),
        ),
    }
}

/// Terminator a Chat stream must end with.
pub fn chat_done() -> Bytes {
    encode(None, "[DONE]")
}

/// Usage from a complete (non-streaming) response body.
pub fn usage_from_response(protocol: Protocol, body: &Value) -> Option<Usage> {
    let u = body.get("usage").filter(|u| u.is_object())?;
    Some(match protocol {
        Protocol::Chat => chat::decode_usage(u),
        Protocol::Messages => messages::decode_usage(u),
        Protocol::Responses => responses::decode_usage(u),
    })
}

/// Response id from a complete body (used to pin stateful Responses conversations).
pub fn response_id(body: &Value) -> Option<&str> {
    body.get("id").and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sse::SseParser;

    #[test]
    fn error_events_parse_in_each_protocol() {
        for p in Protocol::ALL {
            let bytes = error_event(p, "upstream went away");
            let mut parser = SseParser::new();
            let events = parser.feed(&bytes);
            assert_eq!(events.len(), 1, "{p}");
            let v: Value = serde_json::from_str(&events[0].data).expect("json");
            assert!(v.to_string().contains("upstream went away"));
        }
    }

    #[test]
    fn extracts_usage_from_bodies() {
        let chat = json!({"id": "c", "usage": {"prompt_tokens": 3, "completion_tokens": 4}});
        assert_eq!(
            usage_from_response(Protocol::Chat, &chat).and_then(|u| u.output_tokens),
            Some(4)
        );
        let msg =
            json!({"usage": {"input_tokens": 3, "cache_read_input_tokens": 2, "output_tokens": 1}});
        assert_eq!(
            usage_from_response(Protocol::Messages, &msg).and_then(|u| u.input_tokens),
            Some(5)
        );
        let resp = json!({"id": "resp_1", "usage": {"input_tokens": 1, "output_tokens": 2}});
        assert_eq!(response_id(&resp), Some("resp_1"));
        assert_eq!(usage_from_response(Protocol::Responses, &json!({})), None);
    }
}
