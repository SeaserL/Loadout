// SPDX-License-Identifier: Apache-2.0

//! Conversation id derivation for session-aware upstreams (ADR-013, ExecPlan 016): the
//! fallback path hashes the opening of the conversation, here a coding-agent sized request
//! (50 KB system prompt, long history).

use axum::http::HeaderMap;
use criterion::{Criterion, criterion_group, criterion_main};
use loadout_gateway::ConversationSeed;
use loadout_protocol::Protocol;
use serde_json::{Value, json};

fn agent_request() -> Value {
    let system = "You are a coding agent. Follow the rules. ".repeat(1200);
    let mut messages = vec![json!({"role": "user", "content": [
        {"type": "text", "text": "Fix the failing test in src/parser.rs", "cache_control": {"type": "ephemeral"}}
    ]})];
    for i in 0..40 {
        messages.push(json!({"role": "assistant", "content": [{"type": "text", "text": format!("step {i}")}]}));
        messages.push(
            json!({"role": "user", "content": [{"type": "text", "text": format!("result {i}")}]}),
        );
    }
    json!({
        "model": "m",
        "max_tokens": 1024,
        "system": [{"type": "text", "text": system}],
        "messages": messages
    })
}

fn conversation_session(c: &mut Criterion) {
    let body = agent_request();
    let none = HeaderMap::new();
    let mut with_header = HeaderMap::new();
    with_header.insert(
        "x-claude-code-session-id",
        "3f2a9c1e-0000-4000-8000-000000000001"
            .parse()
            .expect("value"),
    );

    c.bench_function("conversation_session/opening_50kb", |b| {
        b.iter(|| {
            ConversationSeed::new(Protocol::Messages, "client", &none, &body).for_account("acct")
        })
    });
    c.bench_function("conversation_session/client_header", |b| {
        b.iter(|| {
            ConversationSeed::new(Protocol::Messages, "client", &with_header, &body)
                .for_account("acct")
        })
    });
}

criterion_group!(benches, conversation_session);
criterion_main!(benches);
