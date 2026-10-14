// SPDX-License-Identifier: Apache-2.0

//! OpenCode Go through a real daemon (ExecPlan 016): a mock upstream that, like the real one,
//! rejects requests without `x-opencode-session`. Probes and client requests must carry it,
//! a conversation keeps its id across turns, and GPT models sync onto the Responses endpoint.

use std::net::SocketAddr;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::{Value, json};

mod common;

use common::Daemon;

const UPSTREAM_KEY: &str = "sk-e2e-opencode-go-0123456789";

/// (path, session header) of every inference request that reached the mock.
type Seen = Arc<Mutex<Vec<(String, String)>>>;

fn sse(events: &[Value], done: bool) -> Response {
    let mut body: String = events
        .iter()
        .map(|e| match e["type"].as_str() {
            Some(t) => format!("event: {t}\ndata: {e}\n\n"),
            None => format!("data: {e}\n\n"),
        })
        .collect();
    if done {
        body.push_str("data: [DONE]\n\n");
    }
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(axum::body::Body::from(body))
        .expect("response")
}

async fn inference(seen: Seen, req: Request) -> Response {
    let path = req.uri().path().to_owned();
    let Some(session) = req
        .headers()
        .get("x-opencode-session")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": {"type": "invalid_request_error",
                "message": "Request is missing x-opencode-session and cannot be routed efficiently."}})),
        )
            .into_response();
    };
    seen.lock().expect("lock").push((path.clone(), session));
    if path.ends_with("/responses") {
        sse(
            &[
                json!({"type": "response.created", "response": {"id": "resp_1", "model": "gpt-6-luna"}}),
                json!({"type": "response.output_text.delta", "item_id": "m", "delta": "OK"}),
                json!({"type": "response.completed", "response": {"id": "resp_1", "status": "completed",
                    "usage": {"input_tokens": 3, "output_tokens": 1}}}),
            ],
            false,
        )
    } else {
        sse(
            &[
                json!({"id": "c1", "model": "kimi-k3", "choices": [{"index": 0, "delta": {"role": "assistant", "content": ""}}]}),
                json!({"choices": [{"index": 0, "delta": {"content": "OK"}}]}),
                json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]}),
                json!({"choices": [], "usage": {"prompt_tokens": 5, "completion_tokens": 1}}),
            ],
            true,
        )
    }
}

fn start_upstream() -> (String, Seen) {
    let seen: Seen = Arc::default();
    let shared = seen.clone();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime");
        rt.block_on(async move {
            let chat_seen = shared.clone();
            let app = Router::new()
                .route(
                    "/v1/models",
                    get(|| async {
                        Json(json!({"object": "list", "data": [
                            {"id": "kimi-k3"}, {"id": "gpt-6-luna"}
                        ]}))
                    }),
                )
                .route(
                    "/v1/usage",
                    get(|| async {
                        Json(json!({"usage": {"rolling": {"usagePercent": 1, "resetInSec": 60}}}))
                    }),
                )
                .route(
                    "/v1/chat/completions",
                    post(move |req: Request| inference(chat_seen.clone(), req)),
                )
                .route(
                    "/v1/responses",
                    post(move |req: Request| inference(shared.clone(), req)),
                );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            tx.send(listener.local_addr().expect("addr"))
                .expect("send addr");
            let _ = axum::serve(listener, app).await;
        });
    });
    let addr: SocketAddr = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("upstream started");
    (format!("http://{addr}/v1"), seen)
}

const KIMI: &str = "opencode-go/main/kimi-k3";
const GPT: &str = "opencode-go/main/gpt-6-luna";

#[test]
fn opencode_go_requests_carry_a_stable_session() {
    let (base, seen) = start_upstream();
    let d = Daemon::start(None);
    d.json(&["config", "set", "benchmark.auto_probe", "false"], None);
    d.json(
        &["provider", "add", "opencode-go", "--base-url", &base],
        None,
    );
    let added = d.json(
        &["account", "add", "opencode-go", "main", "--api-key-stdin"],
        Some(UPSTREAM_KEY),
    );
    assert_eq!(added["sync"]["total"], 2, "{added}");

    // GPT models are served on the Responses endpoint.
    let models = d.json(&["model", "list"], None);
    let protocols = |name: &str| {
        models
            .as_array()
            .expect("models")
            .iter()
            .find(|m| m["name"] == name)
            .unwrap_or_else(|| panic!("{name} missing: {models}"))["protocols"]
            .clone()
    };
    assert_eq!(protocols(GPT), json!(["responses"]));
    assert_eq!(protocols(KIMI), json!(["chat"]));

    // Probes pass the session check, each with its own id.
    let results = d.json(&["bench", KIMI, GPT], None);
    let results = results.as_array().expect("results");
    assert_eq!(results.len(), 2, "{results:?}");
    assert!(results.iter().all(|r| r["ok"] == true), "{results:?}");
    let probes: Vec<(String, String)> = seen.lock().expect("lock").drain(..).collect();
    assert_eq!(probes.len(), 2);
    assert_ne!(probes[0].1, probes[1].1);

    // Client requests: the same conversation keeps its id, another one gets a new id.
    let key = d.json(&["client", "create", "e2e"], None)["key"]
        .as_str()
        .expect("key")
        .to_owned();
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let http = reqwest::Client::new();
    let post = |path: &str, body: Value| {
        rt.block_on(async {
            let resp = http
                .post(format!("http://{}{path}", d.gateway))
                .bearer_auth(&key)
                .json(&body)
                .send()
                .await
                .expect("send");
            let status = resp.status().as_u16();
            let text = resp.text().await.expect("body");
            assert_eq!(status, 200, "{path}: {text}");
        });
    };
    post(
        "/v1/chat/completions",
        json!({"model": KIMI, "stream": true, "messages": [
            {"role": "user", "content": "first"}
        ]}),
    );
    post(
        "/v1/chat/completions",
        json!({"model": KIMI, "stream": true, "messages": [
            {"role": "user", "content": "first"},
            {"role": "assistant", "content": "OK"},
            {"role": "user", "content": "second"}
        ]}),
    );
    post(
        "/v1/chat/completions",
        json!({"model": KIMI, "stream": true, "messages": [
            {"role": "user", "content": "unrelated"}
        ]}),
    );
    post("/v1/responses", json!({"model": GPT, "input": "hi"}));

    let calls = seen.lock().expect("lock").clone();
    assert_eq!(calls.len(), 4, "{calls:?}");
    assert_eq!(calls[0].1, calls[1].1, "same conversation");
    assert_ne!(calls[0].1, calls[2].1, "different conversation");
    assert!(calls[3].0.ends_with("/responses"));
    assert!(
        calls.iter().all(|(_, s)| s.len() == 36),
        "uuid-form ids: {calls:?}"
    );
}
