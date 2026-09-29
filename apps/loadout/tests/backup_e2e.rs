// SPDX-License-Identifier: Apache-2.0

//! Configuration export from one daemon and import into a fresh one (ExecPlan 006):
//! everything but secrets moves, client keys keep working, API keys never leave
//! Credential Manager.

use serde_json::Value;

mod common;

use common::Daemon;

const UPSTREAM_KEY: &str = "sk-e2e-backup-secret-0123456789";

fn names(v: &Value, field: &str) -> Vec<String> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|x| x[field].as_str().expect("str").to_owned())
        .collect()
}

#[test]
fn export_then_import_into_a_new_daemon() {
    let a = Daemon::start(None);
    a.json(
        &[
            "provider",
            "add",
            "mock",
            "--kind",
            "openai-compatible",
            "--base-url",
            "http://127.0.0.1:9/v1",
        ],
        None,
    );
    a.json(
        &["account", "add", "mock", "main", "--api-key-stdin"],
        Some(UPSTREAM_KEY),
    );
    a.json(
        &[
            "model",
            "add",
            "mock",
            "main",
            "m1",
            "--protocol",
            "chat",
            "--context",
            "64000",
        ],
        None,
    );
    a.json(&["model", "add", "mock", "main", "m2"], None);
    a.json(
        &[
            "model",
            "price",
            "mock/main/m1",
            "--input",
            "1.5",
            "--output",
            "6",
        ],
        None,
    );
    a.json(&["model", "tier", "mock/main/m1", "fast"], None);
    a.json(&["model", "disable", "mock/main/m2"], None);
    a.json(&["pool", "set", "mine", "mock/main/m1"], None);
    a.json(&["config", "set", "latency.ultra_fast_ms", "500"], None);
    let key = a.json(&["client", "create", "app"], None)["key"]
        .as_str()
        .expect("key")
        .to_owned();
    a.json(
        &[
            "client",
            "policy",
            "app",
            "--allow",
            "loadout/*",
            "--rpm",
            "30",
        ],
        None,
    );

    let out = a.data.path().join("export.json");
    let exported = a.json(
        &["backup", "export", "--out", out.to_str().expect("utf8")],
        None,
    );
    assert_eq!(exported["models"], 2);
    assert_eq!(exported["clients"], 1);
    let text = std::fs::read_to_string(&out).expect("export file");
    assert!(!text.contains(UPSTREAM_KEY), "API keys are never exported");
    assert!(
        !text.contains(&key),
        "client keys are exported only as hashes"
    );
    let db_copy = a.json(&["backup", "database"], None);
    assert!(std::path::Path::new(db_copy["path"].as_str().expect("path")).is_file());

    let b = Daemon::start(None);
    let report = b.json(&["backup", "import", out.to_str().expect("utf8")], None);
    assert_eq!(report["providers"], 1, "{report}");
    assert_eq!(report["accounts"], 1);
    assert_eq!(report["models"], 2);
    assert_eq!(report["pools"], 1);
    assert_eq!(report["clients"], 1);
    assert_eq!(report["settings"], 1);
    assert_eq!(report["accounts_needing_keys"][0], "mock/main");

    let models = b.json(&["model", "list", "--all"], None);
    let m1 = models
        .as_array()
        .expect("models")
        .iter()
        .find(|m| m["name"] == "mock/main/m1")
        .expect("m1");
    assert_eq!(m1["input_price"], 1.5);
    assert_eq!(m1["context_window"], 64000);
    assert_eq!(m1["tier_override"], "FAST");
    let m2 = models
        .as_array()
        .expect("models")
        .iter()
        .find(|m| m["name"] == "mock/main/m2")
        .expect("m2");
    assert_eq!(m2["enabled"], false);
    assert_eq!(
        names(&b.json(&["pool", "list"], None), "name"),
        vec!["mine"]
    );
    assert_eq!(
        b.json(&["config", "get", "latency.ultra_fast_ms"], None)["value"],
        "500"
    );
    let accounts = b.json(&["account", "list"], None);
    assert_eq!(accounts[0]["has_key"], false, "keys must be entered again");
    assert!(b.secrets().is_empty());
    let clients = b.json(&["client", "list"], None);
    assert_eq!(clients[0]["policy"]["requests_per_minute"], 30);

    // The client key from the first machine works on the second.
    let status = tokio::runtime::Runtime::new()
        .expect("runtime")
        .block_on(async {
            reqwest::Client::new()
                .get(format!("http://{}/v1/models", b.gateway))
                .bearer_auth(&key)
                .send()
                .await
                .expect("send")
                .status()
        });
    assert_eq!(status, 200);

    // Importing again changes nothing.
    let again = b.json(&["backup", "import", out.to_str().expect("utf8")], None);
    assert_eq!(again["accounts"], 0);
    assert_eq!(again["clients"], 0);
    assert!(!again["skipped"].as_array().expect("skipped").is_empty());
}
