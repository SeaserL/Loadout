// SPDX-License-Identifier: Apache-2.0

//! Stable conversation ids for upstreams that ask for one (ADR-013).
//!
//! Computed after route selection, from in-memory request data only. The upstream never sees
//! the client's own id or any request text: only a hash bound to the client and the account.

use axum::http::{HeaderMap, HeaderValue};
use loadout_protocol::Protocol;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Client headers that already name the conversation, in priority order.
const CLIENT_HEADERS: [&str; 5] = [
    "x-opencode-session",
    "x-loadout-session-id",
    "x-claude-code-session-id",
    "session_id",
    "conversation_id",
];

/// Request-level digest of the conversation identity. Computed at most once per request;
/// [`ConversationSeed::for_account`] derives the value sent to each account.
#[derive(Clone)]
pub struct ConversationSeed([u8; 32]);

impl std::fmt::Debug for ConversationSeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ConversationSeed(..)")
    }
}

impl ConversationSeed {
    /// Source priority: a client session header, then `prompt_cache_key`, then the opening of
    /// the conversation (system prompt and first user message, text only).
    pub fn new(ingress: Protocol, client: &str, headers: &HeaderMap, body: &Value) -> Self {
        let mut h = Sha256::new();
        h.update(b"loadout-conversation-v1\0");
        h.update(client.as_bytes());
        h.update([0]);
        if let Some(v) = client_session(headers) {
            h.update(b"header\0");
            h.update(v);
        } else if let Some(key) = body
            .get("prompt_cache_key")
            .and_then(Value::as_str)
            .filter(|k| !k.trim().is_empty())
        {
            h.update(b"cache-key\0");
            h.update(key.as_bytes());
        } else {
            h.update(b"opening\0");
            opening(ingress, body, &mut h);
        }
        Self(h.finalize().into())
    }

    /// The id sent to one account: a UUID (version 8) string.
    pub fn for_account(&self, account_id: &str) -> HeaderValue {
        let digest: [u8; 32] = Sha256::new()
            .chain_update(self.0)
            .chain_update(account_id.as_bytes())
            .finalize()
            .into();
        let mut b = [0u8; 16];
        b.copy_from_slice(&digest[..16]);
        b[6] = (b[6] & 0x0f) | 0x80;
        b[8] = (b[8] & 0x3f) | 0x80;
        let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
        let id = format!(
            "{}-{}-{}-{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        );
        HeaderValue::from_str(&id).expect("hex and dashes are a valid header value")
    }
}

fn client_session(headers: &HeaderMap) -> Option<&[u8]> {
    CLIENT_HEADERS.iter().find_map(|name| {
        headers
            .get(*name)
            .map(HeaderValue::as_bytes)
            .filter(|v| !v.trim_ascii().is_empty())
    })
}

/// Feeds the parts of a request that stay the same for every turn of a conversation.
fn opening(ingress: Protocol, body: &Value, h: &mut Sha256) {
    let (preamble, items) = match ingress {
        Protocol::Chat => (None, body.get("messages")),
        Protocol::Messages => (body.get("system"), body.get("messages")),
        Protocol::Responses => (body.get("instructions"), body.get("input")),
    };
    if let Some(p) = preamble {
        h.update(b"preamble\0");
        feed_text(p, h);
    }
    match items {
        // Responses accepts a plain string as the (single) user message.
        Some(Value::String(s)) => {
            h.update(b"user\0");
            h.update(s.as_bytes());
        }
        Some(Value::Array(items)) => {
            for item in items {
                let role = item.get("role").and_then(Value::as_str).unwrap_or("");
                if !matches!(role, "system" | "developer" | "user") {
                    continue;
                }
                h.update(role.as_bytes());
                h.update([0]);
                if let Some(content) = item.get("content") {
                    feed_text(content, h);
                }
                if role == "user" {
                    break;
                }
            }
        }
        _ => {}
    }
}

/// Text only: strings, and the `text` / `content` fields of content blocks. Markers that move
/// between turns (`cache_control`) and non-text parts are ignored.
fn feed_text(v: &Value, h: &mut Sha256) {
    match v {
        Value::String(s) => {
            h.update(s.as_bytes());
            h.update([0]);
        }
        Value::Array(parts) => parts.iter().for_each(|p| feed_text(p, h)),
        Value::Object(o) => {
            for key in ["text", "content"] {
                if let Some(inner) = o.get(key) {
                    feed_text(inner, h);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn id(ingress: Protocol, client: &str, headers: &HeaderMap, body: &Value) -> String {
        ConversationSeed::new(ingress, client, headers, body)
            .for_account("acct-1")
            .to_str()
            .expect("ascii")
            .to_owned()
    }

    fn none() -> HeaderMap {
        HeaderMap::new()
    }

    #[test]
    fn later_turns_of_a_chat_conversation_keep_the_id() {
        let turn1 = json!({"messages": [
            {"role": "system", "content": "You are terse."},
            {"role": "user", "content": [{"type": "text", "text": "Fix the bug"}]}
        ]});
        let turn3 = json!({"messages": [
            {"role": "system", "content": "You are terse."},
            {"role": "user", "content": [{"type": "text", "text": "Fix the bug"}]},
            {"role": "assistant", "content": "Which one?"},
            {"role": "user", "content": "The parser one"},
            {"role": "assistant", "content": "Done."},
            {"role": "user", "content": "Thanks"}
        ]});
        assert_eq!(
            id(Protocol::Chat, "c", &none(), &turn1),
            id(Protocol::Chat, "c", &none(), &turn3)
        );
    }

    #[test]
    fn moving_cache_markers_do_not_change_a_messages_conversation() {
        let turn1 = json!({
            "system": [{"type": "text", "text": "sys", "cache_control": {"type": "ephemeral"}}],
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "hello", "cache_control": {"type": "ephemeral"}}
            ]}]
        });
        let turn2 = json!({
            "system": [{"type": "text", "text": "sys", "cache_control": {"type": "ephemeral"}}],
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "hello"}]},
                {"role": "assistant", "content": [{"type": "text", "text": "hi"}]},
                {"role": "user", "content": [
                    {"type": "text", "text": "more", "cache_control": {"type": "ephemeral"}}
                ]}
            ]
        });
        assert_eq!(
            id(Protocol::Messages, "c", &none(), &turn1),
            id(Protocol::Messages, "c", &none(), &turn2)
        );
    }

    #[test]
    fn responses_string_and_item_inputs() {
        let turn1 = json!({"instructions": "be brief", "input": "hello"});
        let items = json!({"instructions": "be brief", "input": [
            {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "ctx"}]},
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hello"}]},
            {"type": "function_call", "call_id": "c1", "name": "f", "arguments": "{}"},
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "next"}]}
        ]});
        let items_later = json!({"instructions": "be brief", "input": [
            {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "ctx"}]},
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hello"}]},
            {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "a"}]},
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "other"}]}
        ]});
        let a = id(Protocol::Responses, "c", &none(), &turn1);
        assert_eq!(a.len(), 36);
        assert_eq!(
            id(Protocol::Responses, "c", &none(), &items),
            id(Protocol::Responses, "c", &none(), &items_later)
        );
        assert_ne!(a, id(Protocol::Responses, "c", &none(), &items));
    }

    #[test]
    fn different_conversations_clients_and_accounts_differ() {
        let a = json!({"messages": [{"role": "user", "content": "one"}]});
        let b = json!({"messages": [{"role": "user", "content": "two"}]});
        assert_ne!(
            id(Protocol::Chat, "c", &none(), &a),
            id(Protocol::Chat, "c", &none(), &b)
        );
        assert_ne!(
            id(Protocol::Chat, "c1", &none(), &a),
            id(Protocol::Chat, "c2", &none(), &a)
        );
        let seed = ConversationSeed::new(Protocol::Chat, "c", &none(), &a);
        assert_ne!(seed.for_account("x"), seed.for_account("y"));
        assert_eq!(seed.for_account("x"), seed.for_account("x"));
    }

    #[test]
    fn client_header_wins_over_body_and_is_not_forwarded_verbatim() {
        let a = json!({"messages": [{"role": "user", "content": "one"}], "prompt_cache_key": "k1"});
        let b = json!({"messages": [{"role": "user", "content": "two"}], "prompt_cache_key": "k2"});
        let mut h = HeaderMap::new();
        h.insert(
            "x-claude-code-session-id",
            HeaderValue::from_static("sess-123"),
        );
        let ia = id(Protocol::Chat, "c", &h, &a);
        assert_eq!(ia, id(Protocol::Chat, "c", &h, &b), "header decides");
        assert!(!ia.contains("sess-123"));

        // Without a header, prompt_cache_key decides; blank values are ignored.
        let same_key =
            json!({"messages": [{"role": "user", "content": "two"}], "prompt_cache_key": "k1"});
        assert_eq!(
            id(Protocol::Chat, "c", &none(), &a),
            id(Protocol::Chat, "c", &none(), &same_key)
        );
        let mut blank = HeaderMap::new();
        blank.insert("x-opencode-session", HeaderValue::from_static("  "));
        assert_eq!(
            id(Protocol::Chat, "c", &blank, &a),
            id(Protocol::Chat, "c", &none(), &a)
        );
    }

    #[test]
    fn ids_are_version_8_uuids() {
        let v = id(Protocol::Chat, "c", &none(), &json!({}));
        let parts: Vec<&str> = v.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            [8, 4, 4, 4, 12]
        );
        assert!(parts[2].starts_with('8'));
        assert!(matches!(&parts[3][..1], "8" | "9" | "a" | "b"));
    }
}
