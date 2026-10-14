// SPDX-License-Identifier: Apache-2.0

//! Starting Edge with a dedicated profile and a CDP pipe (ExecPlan 017 §5.1).
//!
//! The command line is fixed here. It never contains anything that changes how the browser
//! presents itself to sites (ADR-014 §2): see [`FORBIDDEN_ARG_PREFIXES`].

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use loadout_platform::{OwnedJob, OwnedProcess, spawn_owned};
use serde_json::{Value, json};

use crate::BrowserError;
use crate::cdp::Cdp;
use crate::page::Page;

/// Arguments Loadout must never pass: they hide automation, change the user agent, route
/// traffic elsewhere or load code into the browser.
pub const FORBIDDEN_ARG_PREFIXES: &[&str] = &[
    "--disable-blink-features",
    "--user-agent",
    "--proxy",
    "--load-extension",
    "--disable-extensions-except",
    "--remote-debugging-port",
    "--remote-debugging-address",
    "--host-resolver-rules",
    "--enable-automation",
];

#[derive(Clone, Debug)]
pub struct LaunchOptions {
    pub edge: PathBuf,
    /// Dedicated `--user-data-dir` of one account.
    pub profile_dir: PathBuf,
    pub headless: bool,
    pub start_url: String,
}

/// The full argument list (without the program).
pub fn launch_args(opts: &LaunchOptions, pipe_read: &str, pipe_write: &str) -> Vec<OsString> {
    let mut profile = OsString::from("--user-data-dir=");
    profile.push(&opts.profile_dir);
    let mut args = vec![
        profile,
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--remote-debugging-pipe".into(),
        format!("--remote-debugging-io-pipes={pipe_read},{pipe_write}").into(),
        // Keep streaming while minimized or covered (ADR-014 §3).
        "--disable-background-timer-throttling".into(),
        "--disable-backgrounding-occluded-windows".into(),
        "--disable-renderer-backgrounding".into(),
    ];
    if opts.headless {
        args.push("--headless=new".into());
    }
    args.push(opts.start_url.clone().into());
    args
}

/// A running browser owned by the daemon.
#[derive(Debug)]
pub struct Browser {
    process: OwnedProcess,
    cdp: Cdp,
}

impl Browser {
    /// Starts Edge inside `job` and waits until it answers on the pipe.
    pub async fn launch(opts: &LaunchOptions, job: &OwnedJob) -> Result<Self, BrowserError> {
        std::fs::create_dir_all(&opts.profile_dir).map_err(|e| BrowserError::Io(e.to_string()))?;
        let (process, pipes) = spawn_owned(&opts.edge, |r, w| launch_args(opts, r, w), job)
            .map_err(|e| BrowserError::Launch(e.to_string()))?;
        let cdp = Cdp::new(pipes.to_child, pipes.from_child);
        let browser = Self { process, cdp };
        match browser
            .cdp
            .call_with_timeout(
                "Browser.getVersion",
                json!({}),
                None,
                Duration::from_secs(30),
            )
            .await
        {
            Ok(v) => {
                let product = v.get("product").and_then(Value::as_str).unwrap_or("?");
                tracing::info!(product, "browser started");
                Ok(browser)
            }
            Err(e) => {
                // An Edge already running on this profile takes the launch over and exits.
                let exited = browser.process.try_exit_code().ok().flatten().is_some();
                browser.process.kill();
                Err(if exited {
                    BrowserError::ProfileInUse
                } else {
                    BrowserError::Launch(format!("no answer on the debugging pipe: {e}"))
                })
            }
        }
    }

    pub fn cdp(&self) -> &Cdp {
        &self.cdp
    }

    pub fn pid(&self) -> u32 {
        self.process.pid()
    }

    pub fn is_alive(&self) -> bool {
        !self.cdp.is_closed() && self.process.try_exit_code().ok().flatten().is_none()
    }

    /// Attaches to the first tab (the one opened with the start URL).
    pub async fn first_page(&self, origin: &str) -> Result<Page, BrowserError> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            let targets = self.cdp.call("Target.getTargets", json!({}), None).await?;
            let pages: Vec<&Value> = targets
                .get("targetInfos")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter(|t| t.get("type").and_then(Value::as_str) == Some("page"))
                        .collect()
                })
                .unwrap_or_default();
            let chosen = pages
                .iter()
                .find(|t| {
                    t.get("url")
                        .and_then(Value::as_str)
                        .is_some_and(|u| u.starts_with(origin))
                })
                .or_else(|| pages.first());
            if let Some(t) = chosen
                && let Some(id) = t.get("targetId").and_then(Value::as_str)
            {
                return Page::attach(self.cdp.clone(), id).await;
            }
            if tokio::time::Instant::now() > deadline {
                return Err(BrowserError::Timeout("waiting for the first tab"));
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    /// Closes the browser so it flushes its profile, then ends it after `grace`.
    pub async fn shutdown(self, grace: Duration) {
        let _ = self
            .cdp
            .call_with_timeout("Browser.close", json!({}), None, grace)
            .await;
        let process = self.process;
        let exited = tokio::task::spawn_blocking(move || {
            let done = process.wait(grace);
            if !done {
                process.kill();
            }
            done
        })
        .await
        .unwrap_or(false);
        if !exited {
            tracing::warn!("browser did not close in time; terminated");
        }
        self.cdp.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_arguments_never_change_how_the_browser_presents_itself() {
        for headless in [false, true] {
            let opts = LaunchOptions {
                edge: PathBuf::from(r"C:\Edge\msedge.exe"),
                profile_dir: PathBuf::from(r"C:\Data\browser-profiles\a b"),
                headless,
                start_url: "https://chatgpt.com/".into(),
            };
            let args = launch_args(&opts, "12", "34");
            let text: Vec<String> = args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            for a in &text {
                assert!(
                    !FORBIDDEN_ARG_PREFIXES.iter().any(|f| a.starts_with(f)),
                    "forbidden argument {a}"
                );
            }
            assert!(text.contains(&"--remote-debugging-io-pipes=12,34".to_owned()));
            assert!(text.contains(&r"--user-data-dir=C:\Data\browser-profiles\a b".to_owned()));
            assert_eq!(text.contains(&"--headless=new".to_owned()), headless);
            assert_eq!(
                text.last().map(String::as_str),
                Some("https://chatgpt.com/")
            );
        }
    }
}
