// SPDX-License-Identifier: Apache-2.0

//! Account checks and terminal auth states through a real daemon (ExecPlan 014): the catalog
//! verdicts, per-model stream health, and an account the upstream reports as suspended
//! staying `AUTH_FAILED` until a successful check or a re-enable.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::{Value, json};

mod common;

use common::Daemon;

const UPSTREAM_KEY: &str = "sk-e2e-checks-key-0123456789";

const OK: u8 = 0;
const SUSPENDED: u8 = 1;
const NO_CREDIT: u8 = 2;
const RATE_LIMITED: u8 = 3;

#[derive(Clone, Default)]
struct Upstream {
    mode: Arc<AtomicU8>,
    chat_calls: Arc<AtomicUsize>,
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error": {"message": message}}))).into_response()
}

fn blocked(mode: u8) -> Option<Response> {
    match mode {
        SUSPENDED => Some(error(
            StatusCode::FORBIDDEN,
            "Your account has been suspended.",
        )),
        NO_CREDIT => Some(error(StatusCode::PAYMENT_REQUIRED, "Insufficient balance")),
        RATE_LIMITED => Some(error(StatusCode::TOO_MANY_REQUESTS, "Rate limit reached")),
        _ => None,
    }
}

fn sse(body: String) -> Response {
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(axum::body::Body::from(body))
        .expect("response")
}

async fn models(State(u): State<Upstream>) -> Response {
    if let Some(r) = blocked(u.mode.load(Ordering::SeqCst)) {
        return r;
    }
    Json(json!({"object": "list", "data": [
        {"id": "good-model"}, {"id": "broken-model"}, {"id": "empty-model"}, {"id": "cut-model"}
    ]}))
    .into_response()
}

async fn chat(State(u): State<Upstream>, Json(body): Json<Value>) -> Response {
    u.chat_calls.fetch_add(1, Ordering::SeqCst);
    if let Some(r) = blocked(u.mode.load(Ordering::SeqCst)) {
        return r;
    }
    let model = body["model"].as_str().unwrap_or("?").to_owned();
    let start = json!({"id": "c1", "model": model, "choices": [{"index": 0, "delta": {"role": "assistant", "content": ""}}]});
    let text = json!({"choices": [{"index": 0, "delta": {"content": "OK"}}]});
    let finish = json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]});
    match model.as_str() {
        "broken-model" => sse(format!("data: {start}\n\ndata: {{not json\n\n")),
        "empty-model" => sse(String::new()),
        "cut-model" => sse(format!("data: {start}\n\ndata: {text}\n\n")),
        _ => sse(format!(
            "data: {start}\n\ndata: {text}\n\ndata: {finish}\n\ndata: [DONE]\n\n"
        )),
    }
}

fn start_upstream(state: Upstream) -> String {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime");
        rt.block_on(async move {
            let app = Router::new()
                .route("/v1/models", get(models))
                .route("/v1/chat/completions", post(chat))
                .with_state(state);
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
    format!("http://{addr}/v1")
}

fn route_state(d: &Daemon, name: &str) -> Value {
    d.json(&["route", "status"], None)
        .as_array()
        .expect("status")
        .iter()
        .find(|t| t["name"] == name)
        .unwrap_or_else(|| panic!("{name} missing"))
        .clone()
}

#[test]
fn account_checks_and_terminal_auth_states() {
    let upstream = Upstream::default();
    let base = start_upstream(upstream.clone());
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
    let added = d.json(
        &["account", "add", "mock", "main", "--api-key-stdin"],
        Some(UPSTREAM_KEY),
    );
    assert_eq!(added["sync"]["total"], 4, "{added}");

    // Catalog check.
    let r = d.json(&["account", "check", "mock", "main"], None);
    assert_eq!(r["status"], "ok", "{r}");
    assert_eq!(r["models_listed"], 4);
    assert_eq!(r["cleared"], false);
    assert_eq!(r["models"], json!([]));

    // Per-model probes with stream health.
    let r = d.json(&["account", "check", "mock", "main", "--models"], None);
    let by_name = |n: &str| {
        r["models"]
            .as_array()
            .expect("models")
            .iter()
            .find(|m| m["name"] == format!("mock/main/{n}"))
            .unwrap_or_else(|| panic!("{n} missing: {r}"))
            .clone()
    };
    let good = by_name("good-model");
    assert_eq!(
        (good["ok"].clone(), good["stream"].clone()),
        (json!(true), json!("clean"))
    );
    assert!(good["ttft_ms"].is_u64());
    let broken = by_name("broken-model");
    assert_eq!(
        (broken["ok"].clone(), broken["stream"].clone()),
        (json!(false), json!("malformed"))
    );
    let empty = by_name("empty-model");
    assert_eq!(
        (empty["ok"].clone(), empty["stream"].clone()),
        (json!(false), json!("empty"))
    );
    let cut = by_name("cut-model");
    assert_eq!(cut["stream"], "truncated", "{cut}");

    // The upstream suspends the account: the first rejection is terminal.
    let client_key = d.json(&["client", "create", "e2e"], None)["key"]
        .as_str()
        .expect("key")
        .to_owned();
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let http = reqwest::Client::new();
    let send = || {
        rt.block_on(async {
            http.post(format!("http://{}/v1/chat/completions", d.gateway))
                .bearer_auth(&client_key)
                .json(&json!({"model": "mock/main/good-model", "messages": [{"role": "user", "content": "hi"}]}))
                .send()
                .await
                .expect("send")
                .status()
                .as_u16()
        })
    };
    upstream.mode.store(SUSPENDED, Ordering::SeqCst);
    assert_ne!(send(), 200);
    let t = route_state(&d, "mock/main/good-model");
    assert_eq!(t["state"], "AUTH_FAILED", "{t}");
    assert_eq!(
        t["unavailable_until_ms"],
        Value::Null,
        "no automatic recovery"
    );
    let calls = upstream.chat_calls.load(Ordering::SeqCst);
    assert_ne!(send(), 200);
    assert_eq!(
        upstream.chat_calls.load(Ordering::SeqCst),
        calls,
        "a terminal target is not called"
    );

    // Check verdicts while the account is blocked; failed checks do not lift the state.
    let r = d.json(&["account", "check", "mock", "main"], None);
    assert_eq!(
        (r["status"].clone(), r["http_status"].clone()),
        (json!("account_disabled"), json!(403)),
        "{r}"
    );
    upstream.mode.store(NO_CREDIT, Ordering::SeqCst);
    assert_eq!(
        d.json(&["account", "check", "mock", "main"], None)["status"],
        "no_credit"
    );
    upstream.mode.store(RATE_LIMITED, Ordering::SeqCst);
    assert_eq!(
        d.json(&["account", "check", "mock", "main"], None)["status"],
        "rate_limited"
    );
    assert_eq!(
        route_state(&d, "mock/main/good-model")["state"],
        "AUTH_FAILED"
    );
    let text = d.cli(&["account", "check", "mock", "main"], None);
    assert!(!text.status.success(), "a failed check exits non-zero");

    // A successful check lifts it.
    upstream.mode.store(OK, Ordering::SeqCst);
    let r = d.json(&["account", "check", "mock", "main"], None);
    assert_eq!(
        (r["status"].clone(), r["cleared"].clone()),
        (json!("ok"), json!(true)),
        "{r}"
    );
    assert_eq!(route_state(&d, "mock/main/good-model")["state"], "UNKNOWN");
    assert_eq!(send(), 200);

    // So does re-enabling the account.
    upstream.mode.store(SUSPENDED, Ordering::SeqCst);
    assert_ne!(send(), 200);
    assert_eq!(
        route_state(&d, "mock/main/good-model")["state"],
        "AUTH_FAILED"
    );
    upstream.mode.store(OK, Ordering::SeqCst);
    d.json(&["account", "disable", "mock", "main"], None);
    d.json(&["account", "enable", "mock", "main"], None);
    assert_ne!(
        route_state(&d, "mock/main/good-model")["state"],
        "AUTH_FAILED"
    );
    assert_eq!(send(), 200);

    // An unreachable provider.
    d.json(
        &[
            "provider",
            "add",
            "gone",
            "--kind",
            "openai-compatible",
            "--base-url",
            "http://127.0.0.1:9/v1",
        ],
        None,
    );
    d.json(
        &[
            "account",
            "add",
            "gone",
            "main",
            "--api-key-stdin",
            "--no-sync",
        ],
        Some(UPSTREAM_KEY),
    );
    assert_eq!(
        d.json(&["account", "check", "gone", "main"], None)["status"],
        "unreachable"
    );
}
