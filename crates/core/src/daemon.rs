// SPDX-License-Identifier: Apache-2.0

//! Daemon lifecycle (ExecPlan 001 §5.5):
//! single-instance lock → database → settings → gateway bind → IPC bind → serve → shutdown.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use loadout_gateway::{ClientKeyIndex, Gateway, GatewayConfig};
use loadout_ipc::{IpcServer, pipe_name_for};
use loadout_platform::{InstanceLock, current_user_sid};
use loadout_storage::{Database, now_ms};
use tokio_util::sync::CancellationToken;

use crate::dispatcher::{DaemonState, Dispatcher};
use crate::settings::Resolved;
use crate::supervisor::spawn_critical;
use crate::{DaemonError, DataPaths, VERSION};

/// Win32 ERROR_ACCESS_DENIED: returned when the pipe name is already owned.
const ERROR_ACCESS_DENIED: i32 = 5;

#[derive(Clone, Debug)]
pub struct DaemonOptions {
    pub data_dir: PathBuf,
    /// Overrides `\\.\pipe\loadout-<sid>` (tests).
    pub pipe_name: Option<String>,
    /// Overrides the single-instance mutex suffix (tests).
    pub instance_id: Option<String>,
    /// Overrides the `gateway.port` setting; `0` picks a free port (tests).
    pub port: Option<u16>,
}

/// Runs the daemon until shutdown is requested over IPC, by Ctrl+C, or a critical service
/// fails.
pub async fn run(options: DaemonOptions) -> Result<(), DaemonError> {
    let sid = current_user_sid()?;
    let lock_name = InstanceLock::name_for(options.instance_id.as_deref().unwrap_or(&sid));
    let _instance = InstanceLock::acquire(&lock_name)?.ok_or(DaemonError::AlreadyRunning)?;

    let paths = DataPaths::new(&options.data_dir);
    tracing::info!(version = VERSION, data_dir = %paths.root.display(), "loadoutd starting");

    let db_path = paths.database.clone();
    let db = Arc::new(
        tokio::task::spawn_blocking(move || Database::open(&db_path))
            .await
            .map_err(|_| DaemonError::ServiceFailed("database open"))??,
    );
    let settings = {
        let db = db.clone();
        tokio::task::spawn_blocking(move || Resolved::load(&db))
            .await
            .map_err(|_| DaemonError::ServiceFailed("settings load"))??
    };

    let port = options.port.unwrap_or(settings.gateway_port);
    let keys = Arc::new(ClientKeyIndex::default());
    let gateway = Gateway::bind(
        GatewayConfig {
            bind: SocketAddr::from(([127, 0, 0, 1], port)),
            allowed_origins: settings.allowed_origins.clone(),
            ..GatewayConfig::default()
        },
        keys.clone(),
        Arc::new(loadout_gateway::DataPlane::empty()),
    )
    .await
    .map_err(|e| match e.kind() {
        std::io::ErrorKind::AddrInUse => DaemonError::PortInUse(port),
        _ => DaemonError::Io {
            context: "binding the gateway",
            source: e,
        },
    })?;
    let gateway_addr = gateway.local_addr();

    let pipe_name = options
        .pipe_name
        .clone()
        .unwrap_or_else(|| pipe_name_for(&sid));
    let ipc = IpcServer::bind(&pipe_name, &sid, VERSION).map_err(|e| {
        if e.raw_os_error() == Some(ERROR_ACCESS_DENIED) {
            DaemonError::AlreadyRunning
        } else {
            DaemonError::io("binding the IPC pipe")(e)
        }
    })?;

    let shutdown = CancellationToken::new();
    let state = Arc::new(DaemonState {
        db: db.clone(),
        keys,
        events: ipc.events(),
        shutdown: shutdown.clone(),
        started_at_ms: now_ms(),
        gateway_addr,
        data_dir: paths.root.clone(),
    });
    state
        .refresh_keys()
        .await
        .map_err(|e| DaemonError::Config(format!("loading client keys: {}", e.message)))?;

    let gateway_task = spawn_critical("gateway", shutdown.clone(), gateway.serve(shutdown.clone()));
    let ipc_task = spawn_critical(
        "ipc",
        shutdown.clone(),
        ipc.serve(Arc::new(Dispatcher { state }), shutdown.clone()),
    );
    spawn_signal_listener(shutdown.clone());

    tracing::info!(gateway = %gateway_addr, pipe = %pipe_name, "loadoutd ready");
    shutdown.cancelled().await;
    tracing::info!("loadoutd shutting down");

    let (gateway_result, ipc_result) = tokio::join!(gateway_task, ipc_task);
    drop(db);
    for result in [gateway_result, ipc_result] {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(name)) => return Err(DaemonError::ServiceFailed(name)),
            Err(_) => return Err(DaemonError::ServiceFailed("service task")),
        }
    }
    tracing::info!("loadoutd stopped");
    Ok(())
}

/// Ctrl+C / Ctrl+Break when running attached to a console (`--foreground`). A windowless
/// process receives no logoff notification; it is terminated at logoff and SQLite's WAL
/// keeps the database consistent.
fn spawn_signal_listener(shutdown: CancellationToken) {
    tokio::spawn(async move {
        let ctrl_break = async {
            match tokio::signal::windows::ctrl_break() {
                Ok(mut s) => {
                    s.recv().await;
                }
                Err(_) => std::future::pending().await,
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => tracing::info!("Ctrl+C received"),
            () = ctrl_break => tracing::info!("Ctrl+Break received"),
            () = shutdown.cancelled() => return,
        }
        shutdown.cancel();
    });
}
