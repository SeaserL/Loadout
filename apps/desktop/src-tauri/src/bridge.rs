// SPDX-License-Identifier: Apache-2.0

//! The only way the web UI reaches the daemon (ADR-002, Harness §22): allow-listed IPC
//! methods forwarded over the per-user named pipe.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::time::Duration;

use loadout_ipc::api::{self, Method};
use loadout_ipc::{IpcClient, IpcError};
use serde::Serialize;
use serde_json::Value;

const CLIENT_NAME: &str = "loadout-app";
const START_TIMEOUT: Duration = Duration::from_secs(15);
const STOP_TIMEOUT: Duration = Duration::from_secs(40);
const POLL: Duration = Duration::from_millis(150);

/// Methods the UI may call. Anything else is rejected before it reaches the pipe.
pub const ALLOWED_METHODS: &[&str] = &[
    api::Status::NAME,
    api::ClientCreate::NAME,
    api::ClientList::NAME,
    api::ClientRevoke::NAME,
    api::ClientSetPolicy::NAME,
    api::StatsSummary::NAME,
    api::BackupExport::NAME,
    api::BackupImport::NAME,
    api::BackupDatabase::NAME,
    api::SettingsList::NAME,
    api::SettingsSet::NAME,
    api::SettingsReset::NAME,
    api::ProviderKinds::NAME,
    api::ProviderAdd::NAME,
    api::ProviderList::NAME,
    api::ProviderRemove::NAME,
    api::ProviderSetSettings::NAME,
    api::AccountAdd::NAME,
    api::AccountList::NAME,
    api::AccountSetEnabled::NAME,
    api::AccountSetKey::NAME,
    api::AccountRemove::NAME,
    api::AccountSetPlan::NAME,
    api::ModelSync::NAME,
    api::ModelAdd::NAME,
    api::ModelList::NAME,
    api::ModelSetEnabled::NAME,
    api::ModelSetPrice::NAME,
    api::ModelSetTier::NAME,
    api::ModelRemove::NAME,
    api::RouteStatus::NAME,
    api::RouteVirtuals::NAME,
    api::BenchRun::NAME,
    api::PoolSave::NAME,
    api::PoolList::NAME,
    api::PoolRemove::NAME,
    api::RequestsRecent::NAME,
    api::UsageList::NAME,
    api::DiscoveryScan::NAME,
    api::DiscoveryImport::NAME,
];

/// Error returned to the web UI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UiError {
    /// `not_running`, `forbidden`, a daemon error code (`invalid_params`, `not_found`, …)
    /// or `ipc`.
    pub code: String,
    pub message: String,
}

impl UiError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
        }
    }
}

impl From<IpcError> for UiError {
    fn from(e: IpcError) -> Self {
        match e {
            IpcError::NotRunning => Self::new("not_running", "The Loadout gateway is not running."),
            IpcError::Remote(body) => {
                let code = serde_json::to_value(body.code)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "unknown".to_owned());
                Self {
                    code,
                    message: body.message,
                }
            }
            other => Self::new("ipc", other.to_string()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Bridge {
    pub pipe_name: String,
}

impl Bridge {
    /// The current user's daemon pipe; `LOADOUT_PIPE_NAME` overrides it for development.
    pub fn for_current_user() -> Result<Self, String> {
        if let Ok(pipe) = std::env::var("LOADOUT_PIPE_NAME")
            && !pipe.is_empty()
        {
            return Ok(Self { pipe_name: pipe });
        }
        let sid = loadout_platform::current_user_sid().map_err(|e| e.to_string())?;
        Ok(Self {
            pipe_name: loadout_ipc::pipe_name_for(&sid),
        })
    }

    async fn connect(&self) -> Result<IpcClient, IpcError> {
        IpcClient::connect(&self.pipe_name, CLIENT_NAME).await
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value, UiError> {
        if !ALLOWED_METHODS.contains(&method) {
            return Err(UiError::new(
                "forbidden",
                format!("method {method} is not available to the desktop app"),
            ));
        }
        Ok(self.connect().await?.call_raw(method, params).await?)
    }

    pub async fn is_running(&self) -> bool {
        match self.connect().await {
            Ok(mut c) => c.call::<api::Status>(&()).await.is_ok(),
            Err(_) => false,
        }
    }

    /// Starts `loadoutd.exe` from this app's directory unless it is already running.
    pub async fn start_daemon(&self) -> Result<Value, UiError> {
        if let Ok(mut c) = self.connect().await
            && let Ok(status) = c.call::<api::Status>(&()).await
        {
            return serde_json::to_value(status).map_err(|e| UiError::new("ipc", e.to_string()));
        }
        let exe = daemon_exe().map_err(|m| UiError::new("not_installed", m))?;
        let mut args: Vec<&OsStr> = vec![OsStr::new("--background")];
        if std::env::var("LOADOUT_PIPE_NAME").is_ok_and(|p| !p.is_empty()) {
            args.extend([OsStr::new("--pipe-name"), OsStr::new(&self.pipe_name)]);
        }
        let child = loadout_platform::spawn_detached(&exe, &args).map_err(|e| {
            UiError::new(
                "start_failed",
                format!("cannot start {}: {e}", exe.display()),
            )
        })?;
        let deadline = tokio::time::Instant::now() + START_TIMEOUT;
        loop {
            if let Some(code) = child.try_exit_code().ok().flatten() {
                return Err(UiError::new("start_failed", explain_exit(code)));
            }
            if let Ok(mut c) = self.connect().await
                && let Ok(status) = c.call::<api::Status>(&()).await
            {
                return serde_json::to_value(status)
                    .map_err(|e| UiError::new("ipc", e.to_string()));
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(UiError::new(
                    "start_failed",
                    "the gateway did not become ready in time; see the log in %LOCALAPPDATA%\\Loadout\\logs",
                ));
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// Asks the daemon to stop and waits until it is gone.
    pub async fn stop_daemon(&self) -> Result<(), UiError> {
        let mut c = self.connect().await?;
        c.call::<api::Shutdown>(&()).await?;
        drop(c);
        let deadline = tokio::time::Instant::now() + STOP_TIMEOUT;
        while tokio::time::Instant::now() < deadline {
            if matches!(self.connect().await, Err(IpcError::NotRunning)) {
                return Ok(());
            }
            tokio::time::sleep(POLL).await;
        }
        Err(UiError::new("ipc", "the gateway did not stop in time"))
    }
}

fn explain_exit(code: u32) -> String {
    match code {
        2 => "another Loadout gateway is already running for this user".into(),
        3 => "the gateway port is already in use; change `gateway.port` in Settings".into(),
        4 => "invalid configuration; see the log in %LOCALAPPDATA%\\Loadout\\logs".into(),
        code => format!(
            "loadoutd exited with code {code}; see the log in %LOCALAPPDATA%\\Loadout\\logs"
        ),
    }
}

/// `loadoutd.exe` next to this executable.
pub fn daemon_exe() -> Result<PathBuf, String> {
    let dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
        .ok_or_else(|| "cannot determine the app's location".to_owned())?;
    let exe = dir.join("loadoutd.exe");
    if exe.is_file() {
        Ok(exe)
    } else {
        Err(format!(
            "loadoutd.exe not found next to the app (expected {})",
            exe.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use loadout_ipc::{ErrorBody, ErrorCode};

    use super::*;

    #[tokio::test]
    async fn rejects_methods_outside_the_allow_list() {
        let bridge = Bridge {
            pipe_name: r"\\.\pipe\loadout-desktop-test-none".into(),
        };
        let err = bridge
            .call(api::Shutdown::NAME, Value::Null)
            .await
            .expect_err("forbidden");
        assert_eq!(err.code, "forbidden");
        let err = bridge
            .call(api::Status::NAME, Value::Null)
            .await
            .expect_err("no daemon");
        assert_eq!(err.code, "not_running");
    }

    #[test]
    fn maps_daemon_errors() {
        let e = UiError::from(IpcError::Remote(ErrorBody::new(
            ErrorCode::InvalidParams,
            "bad",
        )));
        assert_eq!(
            e,
            UiError {
                code: "invalid_params".into(),
                message: "bad".into()
            }
        );
        let mut names = ALLOWED_METHODS.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ALLOWED_METHODS.len(), "no duplicates");
    }
}
