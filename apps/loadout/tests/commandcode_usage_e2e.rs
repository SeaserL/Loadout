// SPDX-License-Identifier: Apache-2.0

//! Command Code usage through a real daemon: the billing routes sit on the API origin, outside
//! the `/provider/v1` base URL, and `loadout usage` shows the 5-hour / weekly / monthly windows.

use std::net::SocketAddr;
use std::sync::mpsc;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::routing::get;
use serde_json::json;

mod common;

use common::Daemon;

const UPSTREAM_KEY: &str = "cc-e2e-commandcode-0123456789";

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
                    "/provider/v1/models",
                    get(|| async {
                        Json(json!({"data": [
                            {"id": "kimi-k3", "supported_endpoints": ["/chat/completions"]}
                        ]}))
                    }),
                )
                .route(
                    "/alpha/billing/credits",
                    get(|| async {
                        Json(json!({
                            "credits": {"monthlyCredits": 52.5, "purchasedCredits": 0},
                            "windowLimits": {
                                "fiveHour": {"cap": 20, "used": 5, "resetAt": 4_102_444_800_i64},
                                "weekly": {"cap": 100, "used": 40, "resetAt": "2100-01-01T00:00:00Z"}
                            }
                        }))
                    }),
                )
                .route(
                    "/alpha/billing/subscriptions",
                    get(|| async {
                        Json(json!({"success": true, "data": {
                            "planId": "individual-goat", "status": "active",
                            "currentPeriodEnd": "2100-01-01T00:00:00Z"
                        }}))
                    }),
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
    format!("http://{addr}/provider/v1")
}

#[test]
fn commandcode_usage_shows_credit_windows() {
    let base = start_upstream();
    let d = Daemon::start(None);
    d.json(&["config", "set", "benchmark.auto_probe", "false"], None);
    d.json(
        &["provider", "add", "commandcode", "--base-url", &base],
        None,
    );
    d.json(
        &["account", "add", "commandcode", "main", "--api-key-stdin"],
        Some(UPSTREAM_KEY),
    );

    let list = d.json(&["usage", "commandcode", "--refresh"], None);
    let u = &list.as_array().expect("usage list")[0];
    assert_eq!(u["supported"], true, "{u}");
    assert!(u["error"].is_null(), "{u}");
    assert_eq!(u["source"], "PRIVATE_ENDPOINT", "{u}");
    assert_eq!(u["plan"]["name"], "Command Code GOAT", "{u}");
    let windows = u["windows"].as_array().expect("windows");
    let scopes: Vec<_> = windows.iter().map(|w| w["scope"].as_str()).collect();
    assert_eq!(scopes, [Some("5h"), Some("weekly"), Some("monthly")], "{u}");
    assert_eq!(windows[0]["remaining_percent"], 75.0, "{u}");
    assert_eq!(windows[2]["used"], 17.5, "{u}");
    assert_eq!(windows[2]["limit"], 70.0, "{u}");
}
