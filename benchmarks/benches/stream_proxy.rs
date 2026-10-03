// SPDX-License-Identifier: Apache-2.0

//! Streaming overhead (ExecPlan 002 §8):
//! - `stream_proxy/*`: in-memory processing of a 200-event upstream stream (parse + observe
//!   for passthrough; parse + decode + re-encode for conversion).
//! - `ttft/*`: time to first response byte over loopback HTTP, directly from a mock upstream
//!   versus through the gateway. The difference is the gateway's added TTFT
//!   (target P95 < 10 ms).

use std::hint::black_box;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::response::Response;
use axum::routing::post;
use criterion::{Criterion, criterion_group, criterion_main};
use loadout_gateway::{
    ClientId, ClientKeyIndex, DataPlane, Gateway, GatewayConfig, generate_client_key,
};
use loadout_protocol::sse::SseParser;
use loadout_protocol::{Protocol, StreamDecoder, StreamEncoder, chat, messages};
use loadout_provider_sdk::{
    Capabilities, HttpClient, HttpConfig, ProviderConfig, ProviderModule, TrustLevel,
};
use loadout_providers::OpenAiCompatible;
use loadout_router::{HealthRegistry, Prices, RouteTable, RouteTableHandle, RouteTarget, TargetId};
use loadout_secrets::Secret;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

fn upstream_stream(events: usize) -> String {
    let mut s = format!(
        "data: {}\n\n",
        json!({"id": "c", "model": "m", "choices": [{"index": 0, "delta": {"role": "assistant", "content": ""}}]})
    );
    for i in 0..events {
        s.push_str(&format!(
            "data: {}\n\n",
            json!({"choices": [{"index": 0, "delta": {"content": format!("token{i} ")}}]})
        ));
    }
    s.push_str(&format!(
        "data: {}\n\n",
        json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]})
    ));
    s.push_str(&format!(
        "data: {}\n\n",
        json!({"choices": [], "usage": {"prompt_tokens": 10, "completion_tokens": 200}})
    ));
    s.push_str("data: [DONE]\n\n");
    s
}

fn in_memory(c: &mut Criterion) {
    let stream = upstream_stream(200);
    let chunks: Vec<&[u8]> = stream.as_bytes().chunks(512).collect();
    c.bench_function("stream_proxy/passthrough_200_events", |b| {
        b.iter(|| {
            let mut parser = SseParser::new();
            let mut decoder = chat::ChatStreamDecoder::default();
            let mut out = 0usize;
            for chunk in &chunks {
                for e in parser.feed(black_box(chunk)) {
                    let events = decoder.feed(&e).expect("decode");
                    black_box(&events);
                    out += e.raw.len();
                }
            }
            out
        })
    });
    c.bench_function("stream_proxy/convert_chat_to_messages_200_events", |b| {
        b.iter(|| {
            let mut parser = SseParser::new();
            let mut decoder = chat::ChatStreamDecoder::default();
            let mut encoder = messages::MessagesStreamEncoder::new("loadout/x");
            let mut out = 0usize;
            for chunk in &chunks {
                for e in parser.feed(black_box(chunk)) {
                    for ev in decoder.feed(&e).expect("decode") {
                        out += encoder.encode(&ev).len();
                    }
                }
            }
            out + encoder.finish().len()
        })
    });
}

async fn mock_upstream() -> SocketAddr {
    let body = upstream_stream(20);
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            let body = body.clone();
            async move {
                Response::builder()
                    .header("content-type", "text/event-stream")
                    .body(Body::from(body))
                    .expect("response")
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    addr
}

async fn gateway(upstream: SocketAddr) -> (SocketAddr, String, CancellationToken) {
    let reg = HealthRegistry::default();
    let module: Arc<dyn ProviderModule> = Arc::new(OpenAiCompatible);
    let config = ProviderConfig {
        slug: "mock".into(),
        kind: "openai-compatible".into(),
        base_url: Some(format!("http://{upstream}/v1")),
        settings: Value::Object(Default::default()),
    };
    let endpoint = module
        .endpoint(&config, Protocol::Chat, Some(&Secret::new("sk-bench")))
        .expect("endpoint");
    let id: TargetId = Arc::from("m1:chat");
    let target = Arc::new(RouteTarget {
        health: reg.get_or_create(&id),
        id,
        provider_slug: Arc::from("mock"),
        account_id: Arc::from("a"),
        account_alias: Arc::from("main"),
        account_position: 0,
        upstream_model: Arc::from("m"),
        protocol: Protocol::Chat,
        trust: TrustLevel::Local,
        opted_in: false,
        context_window: None,
        max_output_tokens: None,
        capabilities: Capabilities::DEFAULT_MODEL,
        prices: Prices::default(),
        endpoint,
        module,
        config: Arc::new(config),
    });
    let plane = DataPlane::new(
        Arc::new(RouteTableHandle::new(RouteTable::new(1, vec![target]))),
        HttpClient::new(&HttpConfig::default()).expect("http"),
    );
    let key = generate_client_key();
    let keys = Arc::new(ClientKeyIndex::new([(key.hash, ClientId::from("bench"))]));
    let gw = Gateway::bind(
        GatewayConfig {
            bind: SocketAddr::from(([127, 0, 0, 1], 0)),
            ..GatewayConfig::default()
        },
        keys,
        Arc::new(plane),
    )
    .await
    .expect("bind");
    let addr = gw.local_addr();
    let shutdown = CancellationToken::new();
    tokio::spawn(gw.serve(shutdown.clone()));
    (addr, key.key, shutdown)
}

/// Time until the first body chunk arrives.
async fn first_byte(client: &reqwest::Client, url: &str, key: &str, body: &Value) {
    let mut resp = client
        .post(url)
        .bearer_auth(key)
        .header("x-api-key", key)
        .body(body.to_string())
        .header("content-type", "application/json")
        .send()
        .await
        .expect("send");
    assert!(resp.status().is_success(), "{}", resp.status());
    let first = resp.chunk().await.expect("chunk");
    assert!(first.is_some());
    // Drain so the connection can be reused.
    while resp.chunk().await.expect("chunk").is_some() {}
}

fn ttft(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("runtime");
    let (upstream, (gw, key, _shutdown)) = rt.block_on(async {
        let upstream = mock_upstream().await;
        (upstream, gateway(upstream).await)
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("client");
    let chat_body = json!({"model": "mock/main/m", "stream": true, "messages": [{"role": "user", "content": "hi"}]});
    let direct_body =
        json!({"model": "m", "stream": true, "messages": [{"role": "user", "content": "hi"}]});
    let messages_body = json!({"model": "mock/main/m", "max_tokens": 100, "stream": true, "messages": [{"role": "user", "content": "hi"}]});

    let direct_url = format!("http://{upstream}/v1/chat/completions");
    let chat_url = format!("http://{gw}/v1/chat/completions");
    let messages_url = format!("http://{gw}/v1/messages");

    let mut group = c.benchmark_group("ttft");
    group.bench_function("direct_upstream", |b| {
        b.to_async(&rt)
            .iter(|| first_byte(&client, &direct_url, "sk-bench", &direct_body))
    });
    group.bench_function("via_gateway_passthrough", |b| {
        b.to_async(&rt)
            .iter(|| first_byte(&client, &chat_url, &key, &chat_body))
    });
    group.bench_function("via_gateway_converted_messages", |b| {
        b.to_async(&rt)
            .iter(|| first_byte(&client, &messages_url, &key, &messages_body))
    });
    group.finish();
}

criterion_group!(benches, in_memory, ttft);
criterion_main!(benches);
