// SPDX-License-Identifier: Apache-2.0

//! Typical agent request (system prompt, 12 turns, tool calls, 3 tools) converted from Chat
//! to Anthropic Messages (target P95 < 1 ms).

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use loadout_protocol::{Protocol, chat, hints, messages};
use serde_json::{Value, json};

fn request() -> Value {
    let mut msgs =
        vec![json!({"role": "system", "content": "You are a coding agent. ".repeat(40)})];
    for i in 0..6 {
        msgs.push(json!({"role": "user", "content": format!("Step {i}: please read the file and fix the bug. ").repeat(10)}));
        msgs.push(json!({"role": "assistant", "content": null, "tool_calls": [
            {"id": format!("call_{i}"), "type": "function", "function": {"name": "read_file", "arguments": "{\"path\":\"src/main.rs\"}"}}
        ]}));
        msgs.push(json!({"role": "tool", "tool_call_id": format!("call_{i}"), "content": "fn main() { println!(\"hi\"); }\n".repeat(30)}));
    }
    let tool = |name: &str| json!({"type": "function", "function": {"name": name, "description": "tool", "parameters": {"type": "object", "properties": {"path": {"type": "string"}}}}});
    json!({
        "model": "p/a/m",
        "stream": true,
        "messages": msgs,
        "tools": [tool("read_file"), tool("write_file"), tool("run")],
        "max_tokens": 4096,
    })
}

fn canonical_conversion(c: &mut Criterion) {
    let body = request();
    let bytes = serde_json::to_vec(&body).expect("json");
    c.bench_function("canonical_conversion/chat_to_messages", |b| {
        b.iter(|| {
            let v: Value = serde_json::from_slice(black_box(&bytes)).expect("parse");
            let canonical = chat::decode_request(&v).expect("decode");
            let out =
                messages::encode_request(&canonical, "claude", messages::EncodeOptions::default());
            serde_json::to_vec(&out).expect("serialize")
        })
    });
    c.bench_function("canonical_conversion/passthrough_rewrite", |b| {
        b.iter(|| {
            let mut v: Value = serde_json::from_slice(black_box(&bytes)).expect("parse");
            let h = hints::extract(Protocol::Chat, &v, bytes.len()).expect("hints");
            hints::rewrite_for_passthrough(Protocol::Chat, &mut v, "m", &h);
            serde_json::to_vec(&v).expect("serialize")
        })
    });
}

criterion_group!(benches, canonical_conversion);
criterion_main!(benches);
