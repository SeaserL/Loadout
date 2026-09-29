// SPDX-License-Identifier: Apache-2.0

//! Local agent discovery and subscription usage through a real daemon, against a fake home
//! directory. Expired logins keep the test offline: Loadout reports them without calling
//! any upstream.

mod common;

use common::Daemon;

fn fake_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().expect("tempdir");
    let claude = home.path().join(".claude");
    std::fs::create_dir_all(&claude).expect("mkdir");
    std::fs::write(
        claude.join(".credentials.json"),
        r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-e2e-secret","expiresAt":1000,"subscriptionType":"max"}}"#,
    )
    .expect("write");
    let gemini = home.path().join(".gemini");
    std::fs::create_dir_all(&gemini).expect("mkdir");
    std::fs::write(
        gemini.join("oauth_creds.json"),
        r#"{"access_token":"ya29.e2e-secret","expiry_date":1000}"#,
    )
    .expect("write");
    home
}

#[test]
fn scan_import_and_report_subscription_usage() {
    // CLAUDE_CONFIG_DIR / CODEX_HOME on the test machine would point discovery elsewhere.
    if std::env::var_os("CLAUDE_CONFIG_DIR").is_some() {
        return;
    }
    let home = fake_home();
    let d = Daemon::start(Some(home.path()));

    let scan = d.json(&["scan"], None);
    let text = scan.to_string();
    assert!(
        !text.contains("e2e-secret"),
        "scan output never contains tokens"
    );
    let claude = scan
        .as_array()
        .expect("list")
        .iter()
        .find(|t| t["kind"] == "claude-code")
        .expect("claude found");
    assert_eq!(claude["logged_in"], true);
    assert_eq!(claude["expired"], true);
    assert_eq!(claude["detail"], "max");
    assert_eq!(
        claude["inference"], false,
        "subscription: usage only (ADR-009)"
    );

    let imported = d.json(&["scan", "import", "claude-code"], None);
    assert_eq!(imported["provider"], "claude-code");
    assert_eq!(imported["alias"], "main");
    d.json(&["scan", "import", "gemini-cli"], None);

    // Importing a tool that is not logged in fails clearly.
    let out = d.cli(&["scan", "import", "codex"], None);
    assert!(!out.status.success());

    let usage = d.json(&["usage", "--refresh"], None);
    let rows = usage.as_array().expect("list");
    let claude = rows
        .iter()
        .find(|u| u["provider"] == "claude-code")
        .expect("claude usage");
    assert!(
        claude["error"]
            .as_str()
            .expect("error")
            .contains("run `claude`"),
        "{claude}"
    );
    let gemini = rows
        .iter()
        .find(|u| u["provider"] == "gemini-cli")
        .expect("gemini usage");
    assert!(
        gemini["error"]
            .as_str()
            .expect("error")
            .contains("run `gemini`")
    );
    assert!(!usage.to_string().contains("e2e-secret"));

    // Subscriptions never become routes.
    assert_eq!(d.json(&["route", "status"], None), serde_json::json!([]));
    // No credential was copied into Credential Manager.
    assert!(d.secrets().is_empty());

    // The user can record plan and renewal (separate from quota resets).
    d.json(
        &[
            "account",
            "plan",
            "claude-code",
            "main",
            "--name",
            "Max",
            "--price",
            "100",
            "--renews",
            "2026-10-14",
        ],
        None,
    );
    let usage = d.json(&["usage", "claude-code"], None);
    assert_eq!(usage[0]["plan"]["price_usd_monthly"], 100.0);
    assert_eq!(usage[0]["plan"]["renews_at_ms"], 1_791_936_000_000_i64);
}
