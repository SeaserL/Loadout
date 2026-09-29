// SPDX-License-Identifier: Apache-2.0

//! Latency routing through a real daemon (ExecPlan 004): probes classify two mock models
//! into tiers, virtual models and pools route accordingly, tier pins and session affinity
//! change the choice.

use std::net::SocketAddr;
use std::sync::mpsc;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::{Value, json};

mod common;

use common::Daemon;

const UPSTREAM_KEY: &str = "sk-e2e-latency-key-0123456789";
/// The slow model waits this long before answering.
const SLOW_MS: u64 = 400;

fn start_upstream() -> String {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime");
        rt.block_on(async move {
            let app = Router::new()
                .route(
                    "/v1/models",
                    get(|| async {
                        Json(json!({"object": "list", "data": [
                            {"id": "fast-model"}, {"id": "slow-model"}
                        ]}))
                    }),
                )
                .route(
                    "/v1/chat/completions",
                    post(|Json(body): Json<Value>| async move {
                        let model = body["model"].as_str().unwrap_or("?").to_owned();
                        if model == "slow-model" {
                            tokio::time::sleep(Duration::from_millis(SLOW_MS)).await;
                        }
                        let events = [
                            json!({"id": "c1", "model": model, "choices": [{"index": 0, "delta": {"role": "assistant", "content": ""}}]}),
                            json!({"choices": [{"index": 0, "delta": {"content": format!("OK from {model}")}}]}),
                            json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]}),
                            json!({"choices": [], "usage": {"prompt_tokens": 5, "completion_tokens": 3}}),
                        ];
                        let mut sse: String = events.iter().map(|e| format!("data: {e}\n\n")).collect();
                        sse.push_str("data: [DONE]\n\n");
                        Response::builder()
                            .header("content-type", "text/event-stream")
                            .body(axum::body::Body::from(sse))
                            .expect("response")
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

struct Client {
    rt: tokio::runtime::Runtime,
    http: reqwest::Client,
    gateway: String,
    key: String,
}

impl Client {
    /// Sends a streaming chat request; returns (status, route header).
    fn chat(&self, model: &str, session: Option<&str>) -> (u16, Option<String>, String) {
        self.rt.block_on(async {
            let mut req = self
                .http
                .post(format!("http://{}/v1/chat/completions", self.gateway))
                .bearer_auth(&self.key)
                .json(&json!({"model": model, "stream": true,
                               "messages": [{"role": "user", "content": "hi"}]}));
            if let Some(s) = session {
                req = req.header("x-loadout-session-id", s);
            }
            let resp = req.send().await.expect("send");
            let status = resp.status().as_u16();
            let route = resp
                .headers()
                .get("x-loadout-route")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let body = resp.text().await.expect("body");
            (status, route, body)
        })
    }

    fn route(&self, model: &str, session: Option<&str>) -> String {
        let (status, route, body) = self.chat(model, session);
        assert_eq!(status, 200, "{model}: {body}");
        route.expect("route header")
    }
}

fn virtual_routes(d: &Daemon, name: &str) -> Vec<String> {
    let list = d.json(&["route", "list"], None);
    list.as_array()
        .expect("list")
        .iter()
        .find(|v| v["name"] == name)
        .unwrap_or_else(|| panic!("{name} missing: {list}"))["routes"]
        .as_array()
        .expect("routes")
        .iter()
        .map(|r| r.as_str().expect("str").to_owned())
        .collect()
}

fn tier(d: &Daemon, route: &str) -> String {
    let status = d.json(&["route", "status"], None);
    status
        .as_array()
        .expect("status")
        .iter()
        .find(|t| t["name"] == route)
        .unwrap_or_else(|| panic!("{route} missing"))["tier"]
        .as_str()
        .expect("tier")
        .to_owned()
}

const FAST: &str = "mock/main/fast-model";
const SLOW: &str = "mock/main/slow-model";

#[test]
fn tiers_virtual_models_pools_and_sessions() {
    let base = start_upstream();
    let d = Daemon::start(None);
    // Keep the test quick: the slow model (400 ms) is FAST, the other ULTRA_FAST.
    d.json(&["config", "set", "latency.ultra_fast_ms", "200"], None);
    d.json(&["config", "set", "benchmark.auto_probe", "false"], None);
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
    assert_eq!(added["sync"]["total"], 2, "{added}");
    let client = Client {
        rt: tokio::runtime::Runtime::new().expect("runtime"),
        http: reqwest::Client::new(),
        gateway: d.gateway.clone(),
        key: d.json(&["client", "create", "e2e"], None)["key"]
            .as_str()
            .expect("key")
            .to_owned(),
    };

    // Nothing measured: realtime / fast are empty, default follows the model order.
    assert!(virtual_routes(&d, "loadout/realtime").is_empty());
    assert_eq!(virtual_routes(&d, "loadout/default"), vec![FAST, SLOW]);
    let (status, _, body) = client.chat("loadout/realtime", None);
    assert_eq!(status, 503);
    assert!(body.contains("loadout bench"), "{body}");
    assert_eq!(tier(&d, FAST), "UNMEASURED");

    // Three probes each classify both models.
    for round in 0..3 {
        let results = d.json(&["bench", FAST, SLOW], None);
        let results = results.as_array().expect("results");
        assert_eq!(results.len(), 2, "round {round}: {results:?}");
        assert!(results.iter().all(|r| r["ok"] == true), "{results:?}");
    }
    assert_eq!(tier(&d, FAST), "ULTRA_FAST");
    assert_eq!(tier(&d, SLOW), "FAST");
    assert_eq!(virtual_routes(&d, "loadout/realtime"), vec![FAST]);
    assert_eq!(virtual_routes(&d, "loadout/fast"), vec![FAST, SLOW]);
    assert_eq!(client.route("loadout/realtime", None), FAST);
    assert_eq!(
        d.json(&["bench"], None),
        json!([]),
        "nothing left unmeasured"
    );

    // Pinning a tier overrides the measurement.
    d.json(&["model", "tier", FAST, "normal"], None);
    assert!(virtual_routes(&d, "loadout/realtime").is_empty());
    assert_eq!(virtual_routes(&d, "loadout/default"), vec![SLOW, FAST]);
    assert_eq!(client.route("loadout/fast", None), SLOW);
    d.json(&["model", "tier", FAST, "auto"], None);
    assert_eq!(virtual_routes(&d, "loadout/realtime"), vec![FAST]);

    // Pools: manual order, reserved names rejected.
    let pool = d.json(&["pool", "set", "mine", SLOW, FAST], None);
    assert_eq!(pool["members"], json!([SLOW, FAST]));
    assert_eq!(client.route("loadout/mine", None), SLOW);
    let reserved = d.cli(&["pool", "set", "fast", SLOW], None);
    assert!(!reserved.status.success());
    let latency_pool = d.json(
        &["pool", "set", "quick", SLOW, FAST, "--ordering", "latency"],
        None,
    );
    assert_eq!(latency_pool["ordering"], "latency");
    assert_eq!(virtual_routes(&d, "loadout/quick"), vec![FAST, SLOW]);

    // Session affinity: a session keeps its route when the pool order changes.
    assert_eq!(client.route("loadout/mine", Some("s-1")), SLOW);
    d.json(&["pool", "set", "mine", FAST, SLOW], None);
    assert_eq!(client.route("loadout/mine", None), FAST);
    assert_eq!(client.route("loadout/mine", Some("s-1")), SLOW);
    assert_eq!(client.route("loadout/mine", Some("s-2")), FAST);

    let models: Vec<String> = client.rt.block_on(async {
        let v: Value = client
            .http
            .get(format!("http://{}/v1/models", client.gateway))
            .bearer_auth(&client.key)
            .send()
            .await
            .expect("models")
            .json()
            .await
            .expect("json");
        v["data"]
            .as_array()
            .expect("data")
            .iter()
            .map(|m| m["id"].as_str().expect("id").to_owned())
            .collect()
    });
    for name in [
        "loadout/realtime",
        "loadout/fast",
        "loadout/default",
        "loadout/mine",
    ] {
        assert!(models.contains(&name.to_owned()), "{name} in {models:?}");
    }

    assert_eq!(d.json(&["pool", "remove", "mine"], None)["revoked"], true);
    assert_eq!(
        d.json(&["pool", "list"], None).as_array().map(Vec::len),
        Some(1)
    );
}
