// SPDX-License-Identifier: Apache-2.0

//! Shared harness: a daemon isolated by data dir, pipe name, instance id, Credential
//! Manager prefix, free port and (optionally) a fake home directory.

#![allow(dead_code)]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::Value;

pub const CLI: &str = env!("CARGO_BIN_EXE_loadout");

pub struct Daemon {
    pub id: String,
    pub pipe: String,
    pub data: tempfile::TempDir,
    pub gateway: String,
}

impl Daemon {
    /// Starts an isolated daemon; `home` replaces the user profile for CLI discovery.
    pub fn start(home: Option<&Path>) -> Self {
        Self::start_in(tempfile::tempdir().expect("tempdir"), home)
    }

    /// Starts on a prepared data directory.
    pub fn start_in(data: tempfile::TempDir, home: Option<&Path>) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let mut d = Self {
            pipe: format!(r"\\.\pipe\loadout-e2e-{id}"),
            data,
            gateway: String::new(),
            id,
        };
        let data_dir = d.data.path().to_str().expect("utf8").to_owned();
        let prefix = d.prefix();
        let mut args = vec![
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
        ];
        let home_str = home.map(|h| h.to_str().expect("utf8").to_owned());
        if let Some(h) = &home_str {
            args.extend(["--home-dir", h]);
        }
        let status = d.json(&args, None);
        d.gateway = status["gateway"]["addr"].as_str().expect("addr").to_owned();
        d
    }

    pub fn prefix(&self) -> String {
        format!("loadout-e2e-{}:", self.id)
    }

    pub fn cli(&self, args: &[&str], stdin: Option<&str>) -> Output {
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

    pub fn json(&self, args: &[&str], stdin: Option<&str>) -> Value {
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

    /// Credential Manager entries under this daemon's prefix.
    pub fn secrets(&self) -> Vec<String> {
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
