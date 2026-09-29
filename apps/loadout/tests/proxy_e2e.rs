// SPDX-License-Identifier: Apache-2.0

//! The whole data plane through a real daemon: configure a provider/account with the CLI,
//! sync models from a mock upstream, call the gateway over HTTP in two protocols, and check
//! telemetry, route status and credential cleanup.

use std::io::Write;
use std::net::SocketAddr;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::{Value, json};

const CLI: &str = env!("CARGO_BIN_EXE_loadout");
const UPSTREAM_KEY: &str = "sk-e2e-upstream-key-0123456789";

/// Mock OpenAI-compatible upstream on its own thread and runtime.
fn start_upstream() -> String {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime");
        rt.block_on(async move {
            let authorized = |h: &HeaderMap| {
                h.get("authorization").and_then(|v| v.to_str().ok()) == Some(&format!("Bearer {UPSTREAM_KEY}"))
            };
            let app = Router::new()
                .route(
                    "/v1/models",
                    get(move |h: HeaderMap| async move {
                        if !authorized(&h) {
                            return StatusCode::UNAUTHORIZED.into_response();
                        }
                        Json(json!({"object": "list", "data": [{"id": "mock-model", "context_length": 32000}]}))
                            .into_response()
                    }),
                )
                .route(
                    "/v1/chat/completions",
                    post(move |h: HeaderMap, Json(body): Json<Value>| async move {
                        if !authorized(&h) {
                            return (StatusCode::UNAUTHORIZED, r#"{"error":{"message":"bad key"}}"#).into_response();
                        }
                        let text = format!("echo from {}", body["model"].as_str().unwrap_or("?"));
                        if body["stream"] == true {
                            let events = [
                                json!({"id": "c1", "model": "mock-model", "choices": [{"index": 0, "delta": {"role": "assistant", "content": ""}}]}),
                                json!({"choices": [{"index": 0, "delta": {"content": text}}]}),
                                json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]}),
                                json!({"choices": [], "usage": {"prompt_tokens": 6, "completion_tokens": 4}}),
                            ];
                            let mut sse: String = events.iter().map(|e| format!("data: {e}\n\n")).collect();
                            sse.push_str("data: [DONE]\n\n");
                            return Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(axum::body::Body::from(sse))
                                .expect("response");
                        }
                        Json(json!({
                            "id": "c1", "object": "chat.completion", "model": "mock-model",
                            "choices": [{"index": 0, "message": {"role": "assistant", "content": text}, "finish_reason": "stop"}],
                            "usage": {"prompt_tokens": 6, "completion_tokens": 4}
                        }))
                        .into_response()
                    }),
                );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
            tx.send(listener.local_addr().expect("addr")).expect("send addr");
            let _ = axum::serve(listener, app).await;
        });
    });
    let addr: SocketAddr = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("upstream started");
    format!("http://{addr}/v1")
}

struct Daemon {
    id: String,
    pipe: String,
    data: tempfile::TempDir,
}

impl Daemon {
    fn prefix(&self) -> String {
        format!("loadout-e2e-{}:", self.id)
    }

    fn cli(&self, args: &[&str], stdin: Option<&str>) -> Output {
        let mut child = Command::new(CLI)
            .arg("--pipe-name")
            .arg(&self.pipe)
            .args(args)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn");
        if let Some(input) = stdin {
            child
                .stdin
                .take()
                .expect("stdin")
                .write_all(input.as_bytes())
                .expect("write");
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(child.wait_with_output());
        });
        rx.recv_timeout(Duration::from_secs(60))
            .expect("cli finished")
            .expect("output")
    }

    fn json(&self, args: &[&str], stdin: Option<&str>) -> Value {
        let mut full = vec!["--json"];
        full.extend_from_slice(args);
        let out = self.cli(&full, stdin);
        assert!(
            out.status.success(),
            "loadout {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).expect("json")
    }

    fn secrets(&self) -> Vec<String> {
        let out = Command::new("cmdkey")
            .arg("/list")
            .output()
            .expect("cmdkey");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.split("target=").nth(1))
            .filter(|t| t.starts_with(&self.prefix()))
            .map(|t| t.trim().to_owned())
            .collect()
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.cli(&["daemon", "stop"], None);
        for t in self.secrets() {
            let _ = Command::new("cmdkey").arg(format!("/delete:{t}")).output();
        }
    }
}

#[test]
fn configure_with_cli_and_proxy_through_the_daemon() {
    let base = start_upstream();
    let id = uuid::Uuid::new_v4().to_string();
    let d = Daemon {
        pipe: format!(r"\\.\pipe\loadout-e2e-{id}"),
        data: tempfile::tempdir().expect("tempdir"),
        id,
    };
    let data_dir = d.data.path().to_str().expect("utf8").to_owned();
    let prefix = d.prefix();
    let status = d.json(
        &[
            "daemon",
            "start",
            "--data-dir",
            &data_dir,
            "--port",
            "0",
            "--instance-id",
            &d.id,
            "--secret-prefix",
            &prefix,
        ],
        None,
    );
    let gateway = status["gateway"]["addr"].as_str().expect("addr").to_owned();

    // Configure through the CLI.
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
    assert_eq!(added["sync"]["total"], 1, "{added}");
    assert_eq!(d.secrets().len(), 1, "key stored under the test prefix");
    let models = d.json(&["model", "list"], None);
    assert_eq!(models[0]["name"], "mock/main/mock-model");
    assert_eq!(models[0]["context_window"], 32000);
    let client_key = d.json(&["client", "create", "e2e"], None)["key"]
        .as_str()
        .expect("key")
        .to_owned();

    // The database never contains the upstream key.
    let db_bytes = std::fs::read(d.data.path().join("data").join("loadout.db")).expect("db");
    assert!(!String::from_utf8_lossy(&db_bytes).contains(UPSTREAM_KEY));

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let http = reqwest::Client::new();
    let (chat, anthropic) = rt.block_on(async {
        let chat: Value = http
            .post(format!("http://{gateway}/v1/chat/completions"))
            .bearer_auth(&client_key)
            .json(&json!({"model": "mock/main/mock-model", "messages": [{"role": "user", "content": "hi"}]}))
            .send()
            .await
            .expect("chat")
            .json()
            .await
            .expect("chat json");
        let anthropic: Value = http
            .post(format!("http://{gateway}/v1/messages"))
            .header("x-api-key", &client_key)
            .header("anthropic-version", "2023-06-01")
            .json(&json!({"model": "mock/mock-model", "max_tokens": 50, "messages": [{"role": "user", "content": "hi"}]}))
            .send()
            .await
            .expect("messages")
            .json()
            .await
            .expect("messages json");
        (chat, anthropic)
    });
    assert_eq!(
        chat["choices"][0]["message"]["content"],
        "echo from mock-model"
    );
    assert_eq!(anthropic["type"], "message");
    assert_eq!(anthropic["content"][0]["text"], "echo from mock-model");
    assert_eq!(anthropic["usage"]["output_tokens"], 4);

    // Telemetry reaches the database asynchronously.
    let mut logs = Value::Null;
    for _ in 0..50 {
        logs = d.json(&["logs", "--limit", "10"], None);
        if logs.as_array().map_or(0, Vec::len) >= 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let logs = logs.as_array().expect("logs");
    assert_eq!(logs.len(), 2);
    assert!(logs.iter().all(|l| l["status"] == "ok"));
    assert!(
        logs.iter()
            .any(|l| l["mode"] == "converted" && l["protocol_in"] == "messages")
    );
    assert!(logs.iter().any(|l| l["mode"] == "passthrough"));

    let routes = d.json(&["route", "status"], None);
    assert_eq!(routes[0]["name"], "mock/main/mock-model");
    assert_eq!(routes[0]["state"], "HEALTHY");

    // A wrong key trips the breaker for that route; fixing it clears it.
    d.json(
        &["account", "set-key", "mock", "main", "--api-key-stdin"],
        Some("sk-wrong-key-000000000000"),
    );
    let failed = rt.block_on(async {
        http.post(format!("http://{gateway}/v1/chat/completions"))
            .bearer_auth(&client_key)
            .json(&json!({"model": "mock/main/mock-model", "messages": []}))
            .send()
            .await
            .expect("send")
    });
    assert_eq!(failed.status(), 502);
    assert_eq!(
        d.json(&["route", "status"], None)[0]["state"],
        "AUTH_FAILED"
    );
    d.json(
        &["account", "set-key", "mock", "main", "--api-key-stdin"],
        Some(UPSTREAM_KEY),
    );
    assert_eq!(d.json(&["route", "status"], None)[0]["state"], "UNKNOWN");
    assert_eq!(d.secrets().len(), 1, "old key removed when replaced");

    // Removing the provider removes its credentials from Credential Manager.
    d.json(&["provider", "remove", "mock"], None);
    assert!(d.secrets().is_empty());
    assert_eq!(d.json(&["route", "status"], None), json!([]));
}
