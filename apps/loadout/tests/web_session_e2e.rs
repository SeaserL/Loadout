// SPDX-License-Identifier: Apache-2.0

//! A browser-session account through a real daemon and gateway (ExecPlan 017 Stage 5): a
//! headless Edge drives a local mock of the ChatGPT page. All three API entry points answer
//! with exactly what the page streamed, consecutive requests continue one conversation, and
//! removing the account deletes its browser profile. Skipped without Edge.

use std::time::{Duration, Instant};

use serde_json::{Value, json};

mod common;

#[path = "../../../crates/browser/tests/support/mock_chatgpt.rs"]
mod mock;

use common::Daemon;

const MODEL: &str = "web-chatgpt/main/auto";

fn edge_installed() -> bool {
    let candidates = [
        std::env::var_os("ProgramFiles(x86)"),
        std::env::var_os("ProgramFiles"),
    ];
    candidates.into_iter().flatten().any(|base| {
        std::path::Path::new(&base)
            .join(r"Microsoft\Edge\Application\msedge.exe")
            .is_file()
    })
}

fn wait_for(d: &Daemon, what: &str, mut check: impl FnMut(&Daemon) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !check(d) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(300));
    }
}

#[test]
fn web_session_answers_through_the_gateway() {
    if !edge_installed() {
        eprintln!("skipping: Microsoft Edge is not installed");
        return;
    }
    let (origin, page) = mock::start();
    let d = Daemon::start(None);

    // Off by default: adding an account is refused until the user turns the feature on.
    let refused = d.cli(
        &[
            "account",
            "web",
            "add",
            "web-chatgpt",
            "main",
            "--i-understand-the-risk",
            "--confirm",
            "main",
        ],
        None,
    );
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("web_session.enabled"));

    d.json(&["config", "set", "web_session.enabled", "true"], None);
    d.json(&["config", "set", "web_session.headless", "true"], None);
    d.json(
        &[
            "provider",
            "add",
            "web-chatgpt",
            "--kind",
            "web-chatgpt",
            "--base-url",
            &origin,
        ],
        None,
    );
    // The second confirmation must match the alias.
    let wrong = d.cli(
        &[
            "account",
            "web",
            "add",
            "web-chatgpt",
            "main",
            "--i-understand-the-risk",
            "--confirm",
            "other",
        ],
        None,
    );
    assert!(!wrong.status.success());
    // No sign-in window: the mock page needs no login.
    let added = d.json(
        &[
            "account",
            "web",
            "add",
            "web-chatgpt",
            "main",
            "--i-understand-the-risk",
            "--confirm",
            "main",
            "--no-sign-in",
        ],
        None,
    );
    assert_eq!(added["transport_opt_in"], true, "{added}");
    d.json(
        &[
            "account",
            "web",
            "config",
            "web-chatgpt",
            "main",
            "--min-interval",
            "0",
        ],
        None,
    );

    wait_for(&d, "the session to be ready", |d| {
        d.json(&["account", "web", "status", "web-chatgpt", "main"], None)["state"] == "ready"
    });
    // The page's own model list was read while it loaded.
    wait_for(&d, "the page's models", |d| {
        d.json(&["model", "list"], None)
            .as_array()
            .is_some_and(|m| m.iter().any(|m| m["name"] == "web-chatgpt/main/gpt-5"))
    });
    // Never probed with test requests, never in the built-in virtual models.
    let bench = d.cli(&["bench", MODEL], None);
    assert!(!bench.status.success());
    let virtuals = d.json(&["route", "list"], None).to_string();
    assert!(!virtuals.contains("web-chatgpt"), "{virtuals}");

    let key = d.json(&["client", "create", "e2e"], None)["key"]
        .as_str()
        .expect("key")
        .to_owned();
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let http = reqwest::Client::new();
    let post = |path: &str, body: Value| -> (u16, String) {
        rt.block_on(async {
            let resp = http
                .post(format!("http://{}{path}", d.gateway))
                .bearer_auth(&key)
                .json(&body)
                .timeout(Duration::from_secs(120))
                .send()
                .await
                .expect("send");
            let status = resp.status().as_u16();
            (status, resp.text().await.expect("body"))
        })
    };

    // Chat, non-streaming: a new default conversation.
    let (status, body) = post(
        "/v1/chat/completions",
        json!({"model": MODEL, "messages": [{"role": "user", "content": "hello page"}]}),
    );
    assert_eq!(status, 200, "{body}");
    let v: Value = serde_json::from_str(&body).expect("json");
    let got = page.received();
    assert_eq!(got.len(), 1);
    let conv = got[0].conversation.clone();
    assert_eq!(
        v["choices"][0]["message"]["content"],
        format!("turn 1 of {conv}: hello page")
    );

    // Messages (max_tokens is accepted): the same conversation, only the newest message.
    let (status, body) = post(
        "/v1/messages",
        json!({"model": MODEL, "max_tokens": 100, "messages": [
            {"role": "user", "content": "hello page"},
            {"role": "assistant", "content": "hi"},
            {"role": "user", "content": "second round"}
        ]}),
    );
    assert_eq!(status, 200, "{body}");
    let v: Value = serde_json::from_str(&body).expect("json");
    assert_eq!(
        v["content"][0]["text"],
        format!("turn 2 of {conv}: second round")
    );
    assert_eq!(page.received()[1].text, "second round");

    // Responses, streaming.
    let (status, body) = post(
        "/v1/responses",
        json!({"model": MODEL, "stream": true, "input": "third"}),
    );
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("response.completed"), "{body}");
    let streamed: String = body
        .lines()
        .filter_map(|l| l.strip_prefix("data: "))
        .filter_map(|d| serde_json::from_str::<Value>(d).ok())
        .filter(|e| e["type"] == "response.output_text.delta")
        .filter_map(|e| e["delta"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(streamed, format!("turn 3 of {conv}: third"));

    // Sampling parameters cannot be honoured by a web page: refused, nothing typed.
    let (status, body) = post(
        "/v1/chat/completions",
        json!({"model": MODEL, "temperature": 0.2,
            "messages": [{"role": "user", "content": "x"}]}),
    );
    assert_ne!(status, 200);
    assert!(body.contains("temperature"), "{body}");
    assert_eq!(page.received().len(), 3);

    let status = d.json(&["account", "web", "status", "web-chatgpt", "main"], None);
    assert_eq!(status["default_conversation"], format!("{origin}/c/{conv}"));
    assert_eq!(status["bound_by"], "auto");
    // The refused request never reached the page.
    assert_eq!(status["today_count"], 3);

    // Removing the account closes its browser and deletes the profile.
    let profiles = d.data.path().join("browser-profiles");
    assert_eq!(
        std::fs::read_dir(&profiles).expect("profiles").count(),
        1,
        "one profile directory"
    );
    d.json(&["account", "remove", "web-chatgpt", "main"], None);
    assert_eq!(
        std::fs::read_dir(&profiles).expect("profiles").count(),
        0,
        "profile deleted"
    );
}
