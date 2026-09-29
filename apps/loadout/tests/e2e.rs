// SPDX-License-Identifier: Apache-2.0

//! End-to-end: real `loadout.exe` driving a real `loadoutd.exe` over a Named Pipe, plus HTTP
//! against the gateway. Every test uses its own data directory, pipe name, instance id and a
//! free port, so tests run in parallel and never touch the user's real installation.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::Value;

const CLI: &str = env!("CARGO_BIN_EXE_loadout");
const DAEMON: &str = env!("CARGO_BIN_EXE_loadoutd");
const CLI_TIMEOUT: Duration = Duration::from_secs(60);

struct Sandbox {
    id: String,
    pipe: String,
    data: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        Self {
            pipe: format!(r"\\.\pipe\loadout-e2e-{id}"),
            data: tempfile::tempdir().expect("tempdir"),
            id,
        }
    }

    fn data_dir(&self) -> &Path {
        self.data.path()
    }

    /// Runs the CLI with captured stdout/stderr. Waiting for EOF on those pipes doubles as a
    /// regression test: if the daemon inherited them, this would never return.
    fn cli(&self, args: &[&str]) -> Output {
        self.cli_env(args, &[])
    }

    fn cli_env(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new(CLI);
        cmd.arg("--pipe-name").arg(&self.pipe).args(args);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn loadout");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(child.wait_with_output());
        });
        rx.recv_timeout(CLI_TIMEOUT)
            .unwrap_or_else(|_| {
                panic!(
                    "`loadout {}` did not finish (inherited pipe?)",
                    args.join(" ")
                )
            })
            .expect("wait loadout")
    }

    fn cli_json(&self, args: &[&str]) -> Value {
        let mut full = vec!["--json"];
        full.extend_from_slice(args);
        let out = self.cli(&full);
        assert!(
            out.status.success(),
            "loadout {args:?} failed: {}",
            text(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).expect("json output")
    }

    fn start(&self) -> SocketAddr {
        self.start_env(&[])
    }

    fn start_env(&self, env: &[(&str, &str)]) -> SocketAddr {
        let data = self.data_dir().to_str().expect("utf8 path").to_owned();
        let out = self.cli_env(
            &[
                "--json",
                "daemon",
                "start",
                "--data-dir",
                &data,
                "--port",
                "0",
                "--instance-id",
                &self.id,
            ],
            env,
        );
        assert!(out.status.success(), "start failed: {}", text(&out.stderr));
        let status: Value = serde_json::from_slice(&out.stdout).expect("status json");
        status["gateway"]["addr"]
            .as_str()
            .expect("addr")
            .parse()
            .expect("socket addr")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = Command::new(CLI)
            .args(["--pipe-name", &self.pipe, "daemon", "stop"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Minimal HTTP/1.1 GET; returns (status code, body).
fn http_get(addr: SocketAddr, path: &str, key: Option<&str>) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("timeout");
    let auth = key
        .map(|k| format!("Authorization: Bearer {k}\r\n"))
        .unwrap_or_default();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\n{auth}Connection: close\r\n\r\n"
    )
    .expect("write");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read");
    let status = response
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("status line");
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_owned())
        .unwrap_or_default();
    (status, body)
}

#[test]
fn full_lifecycle_with_client_keys_and_settings() {
    let sb = Sandbox::new();

    let before = sb.cli(&["daemon", "status"]);
    assert_eq!(before.status.code(), Some(3), "status before start");

    let addr = sb.start();

    // Starting again is a no-op that reports the running daemon.
    let again = sb.cli(&["daemon", "start", "--port", "0", "--instance-id", &sb.id]);
    assert!(again.status.success());
    assert!(text(&again.stdout).contains("Already running"));

    let created = sb.cli_json(&["client", "create", "E2E Client"]);
    let key = created["key"].as_str().expect("key").to_owned();
    assert!(key.starts_with("lo-"));

    let (status, body) = http_get(addr, "/v1/models", Some(&key));
    assert_eq!(status, 200, "{body}");
    assert!(body.contains(r#""object":"list""#));
    assert_eq!(http_get(addr, "/v1/models", None).0, 401);

    let listed = sb.cli_json(&["client", "list"]);
    assert_eq!(listed.as_array().map(Vec::len), Some(1));

    let revoke = sb.cli(&["client", "revoke", "E2E Client"]);
    assert!(revoke.status.success());
    assert_eq!(
        http_get(addr, "/v1/models", Some(&key)).0,
        401,
        "revocation must apply without restart"
    );

    let bad = sb.cli(&["config", "set", "gateway.port", "70000"]);
    assert!(!bad.status.success());
    let set = sb.cli_json(&["config", "set", "gateway.port", "9555"]);
    assert_eq!(set["value"], "9555");
    assert_eq!(set["restart_required"], true);

    let stop = sb.cli(&["daemon", "stop"]);
    assert!(stop.status.success(), "{}", text(&stop.stderr));
    assert_eq!(sb.cli(&["daemon", "status"]).status.code(), Some(3));
}

#[test]
fn second_daemon_for_same_instance_is_rejected() {
    let sb = Sandbox::new();
    sb.start();

    let status = Command::new(DAEMON)
        .args(["--data-dir"])
        .arg(sb.data_dir())
        .args([
            "--port",
            "0",
            "--pipe-name",
            &sb.pipe,
            "--instance-id",
            &sb.id,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("run loadoutd");
    assert_eq!(status.code(), Some(2));
}

#[test]
fn port_conflict_is_explained() {
    let sb = Sandbox::new();
    let blocker = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = blocker.local_addr().expect("addr").port().to_string();
    let data = sb.data_dir().to_str().expect("utf8").to_owned();

    let out = sb.cli(&[
        "daemon",
        "start",
        "--data-dir",
        &data,
        "--port",
        &port,
        "--instance-id",
        &sb.id,
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("already in use"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn logs_never_contain_client_keys() {
    let sb = Sandbox::new();
    let addr = sb.start_env(&[("LOADOUT_LOG", "trace")]);

    let created = sb.cli_json(&["client", "create", "log-probe"]);
    let key = created["key"].as_str().expect("key").to_owned();
    assert_eq!(http_get(addr, "/v1/models", Some(&key)).0, 200);
    // A key in the URL is still redacted by the log writer.
    assert_eq!(
        http_get(addr, &format!("/v1/models?leak={key}"), Some(&key)).0,
        200
    );
    assert_eq!(
        http_get(addr, "/v1/models", Some("lo-0123456789abcdef0123")).0,
        401
    );

    assert!(sb.cli(&["daemon", "stop"]).status.success());

    let logs = sb.data_dir().join("logs");
    let mut combined = String::new();
    for entry in std::fs::read_dir(&logs).expect("log dir") {
        combined.push_str(&std::fs::read_to_string(entry.expect("entry").path()).expect("read"));
    }
    assert!(combined.contains("loadoutd ready"), "logs were written");
    assert!(!combined.contains(&key), "client key leaked into logs");
    assert!(
        !combined.contains(&key[3..]),
        "key material leaked into logs"
    );
    assert!(
        !combined.contains("0123456789abcdef0123"),
        "bearer token leaked into logs"
    );
}

#[test]
fn autostart_commands_use_the_daemon_next_to_the_cli() {
    let sb = Sandbox::new();
    let value = format!("Loadout-E2E-{}", sb.id);

    let enabled = sb.cli_json(&["autostart", "enable", "--value-name", &value]);
    assert_eq!(enabled["enabled"], true);
    let status = sb.cli_json(&["autostart", "status", "--value-name", &value]);
    assert_eq!(status["enabled"], true);
    assert_eq!(status["executable_exists"], true);
    assert!(
        status["command"]
            .as_str()
            .expect("command")
            .contains("loadoutd.exe\" --background")
    );

    let disabled = sb.cli_json(&["autostart", "disable", "--value-name", &value]);
    assert_eq!(disabled["was_enabled"], true);
    let after = sb.cli_json(&["autostart", "status", "--value-name", &value]);
    assert_eq!(after["enabled"], false);
}
