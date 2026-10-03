// SPDX-License-Identifier: Apache-2.0

//! Concurrency through a real daemon (ExecPlan 007 §3): many simultaneous streaming and
//! non-streaming requests in two protocols, all succeed, every one is recorded, and the
//! gateway's own overhead stays small.

use std::net::SocketAddr;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use axum::Json;
use axum::Router;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::{Value, json};

mod common;

use common::Daemon;

const CONCURRENCY: usize = 32;
const ROUNDS: usize = 4;
/// The mock waits this long before its first byte.
const UPSTREAM_DELAY: Duration = Duration::from_millis(50);

fn start_upstream() -> String {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .expect("runtime");
        rt.block_on(async move {
            let app = Router::new()
                .route(
                    "/v1/models",
                    get(|| async { Json(json!({"object": "list", "data": [{"id": "m"}]})) }),
                )
                .route(
                    "/v1/chat/completions",
                    post(|Json(body): Json<Value>| async move {
                        tokio::time::sleep(UPSTREAM_DELAY).await;
                        let mut sse = String::new();
                        sse.push_str(&format!(
                            "data: {}\n\n",
                            json!({"id": "c", "model": "m", "choices": [{"index": 0, "delta": {"role": "assistant", "content": ""}}]})
                        ));
                        for i in 0..20 {
                            sse.push_str(&format!(
                                "data: {}\n\n",
                                json!({"choices": [{"index": 0, "delta": {"content": format!("tok{i} ")}}]})
                            ));
                        }
                        sse.push_str(&format!(
                            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                            json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]}),
                            json!({"choices": [], "usage": {"prompt_tokens": 7, "completion_tokens": 20}})
                        ));
                        let _ = body;
                        Response::builder()
                            .header("content-type", "text/event-stream")
                            .body(axum::body::Body::from(sse))
                            .expect("response")
                            .into_response()
                    }),
                );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
            tx.send(listener.local_addr().expect("addr")).expect("send");
            let _ = axum::serve(listener, app).await;
        });
    });
    let addr: SocketAddr = rx.recv_timeout(Duration::from_secs(10)).expect("upstream");
    format!("http://{addr}/v1")
}

#[test]
fn concurrent_requests_all_succeed_and_are_recorded() {
    let base = start_upstream();
    let d = Daemon::start(None);
    d.json(
        &[
            "provider",
            "add",
            "mock",
            "--kind",
            "openai-compatible",
            "--base-url",
            &base,
        ],
        None,
    );
    d.json(
        &["account", "add", "mock", "main", "--api-key-stdin"],
        Some("sk-e2e-stress-000000000000"),
    );
    let key = d.json(&["client", "create", "stress"], None)["key"]
        .as_str()
        .expect("key")
        .to_owned();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(8)
        .enable_all()
        .build()
        .expect("runtime");
    let http = reqwest::Client::new();
    let gateway = d.gateway.clone();
    let started = Instant::now();
    let latencies: Vec<Duration> = rt.block_on(async {
        let mut all = Vec::new();
        for round in 0..ROUNDS {
            let tasks: Vec<_> = (0..CONCURRENCY)
                .map(|i| {
                    let (http, gateway, key) = (http.clone(), gateway.clone(), key.clone());
                    tokio::spawn(async move {
                        let t0 = Instant::now();
                        let anthropic = (i + round) % 2 == 1;
                        let stream = i % 3 != 0;
                        let resp = if anthropic {
                            http.post(format!("http://{gateway}/v1/messages"))
                                .header("x-api-key", &key)
                                .header("anthropic-version", "2023-06-01")
                                .json(&json!({"model": "loadout/default", "max_tokens": 64, "stream": stream,
                                              "messages": [{"role": "user", "content": "hi"}]}))
                                .send()
                                .await
                        } else {
                            http.post(format!("http://{gateway}/v1/chat/completions"))
                                .bearer_auth(&key)
                                .json(&json!({"model": "mock/main/m", "stream": stream,
                                              "messages": [{"role": "user", "content": "hi"}]}))
                                .send()
                                .await
                        }
                        .expect("send");
                        assert_eq!(resp.status(), 200);
                        let body = resp.text().await.expect("body");
                        assert!(body.contains("tok19"), "complete response: {body}");
                        t0.elapsed()
                    })
                })
                .collect();
            for t in tasks {
                all.push(t.await.expect("task"));
            }
        }
        all
    });
    let total = started.elapsed();
    let n = CONCURRENCY * ROUNDS;
    assert_eq!(latencies.len(), n);

    let mut sorted = latencies.clone();
    sorted.sort();
    let p95 = sorted[n * 95 / 100];
    // Generous bound for a debug build on a shared CI machine: the upstream takes 50 ms.
    assert!(
        p95 < Duration::from_secs(3),
        "p95 {p95:?} over {n} requests in {total:?}"
    );

    // Every request is recorded (telemetry is written asynchronously).
    let mut recorded = 0;
    for _ in 0..100 {
        let logs = d.json(&["logs", "--limit", "1000"], None);
        recorded = logs.as_array().map_or(0, Vec::len);
        if recorded >= n {
            assert!(
                logs.as_array()
                    .expect("logs")
                    .iter()
                    .all(|l| l["status"] == "ok")
            );
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(recorded, n);
    let stats = d.json(&["stats", "--by", "model", "--days", "1"], None);
    let total_requests: i64 = stats
        .as_array()
        .expect("stats")
        .iter()
        .map(|r| r["requests"].as_i64().expect("n"))
        .sum();
    assert_eq!(total_requests as usize, n);
    // Cache fields are always present (zero without cached tokens).
    assert!(
        stats
            .as_array()
            .expect("stats")
            .iter()
            .all(|r| r["cache_read_tokens"] == 0 && r["cache_savings_usd"] == 0.0)
    );
    eprintln!(
        "{n} requests, concurrency {CONCURRENCY}: total {total:?}, p50 {:?}, p95 {p95:?}",
        sorted[n / 2]
    );
}
