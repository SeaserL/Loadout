// SPDX-License-Identifier: Apache-2.0

//! Daemon lifecycle (ExecPlan 001 §5.5, ExecPlan 002):
//! single-instance lock → database → settings → control plane + route table → telemetry
//! writer → gateway bind → IPC bind → serve → shutdown (gateway drains, telemetry flushes).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use loadout_gateway::{ClientKeyIndex, DataPlane, Gateway, GatewayConfig, UpstreamLimits};
use loadout_ipc::{IpcServer, pipe_name_for};
use loadout_platform::{InstanceLock, current_user_sid};
use loadout_provider_sdk::{HttpClient, HttpConfig};
use loadout_providers::Registry;
use loadout_router::RouteTableHandle;
use loadout_secrets::CredentialManagerStore;
use loadout_storage::telemetry::{DEFAULT_CAPACITY, TelemetryWriter};
use loadout_storage::{Database, now_ms};
use tokio_util::sync::CancellationToken;

use crate::control::ControlPlane;
use crate::dispatcher::{DaemonState, Dispatcher};
use crate::settings::Resolved;
use crate::sinks::{LatencyTap, ResponseRoutes, StorageTelemetry};
use crate::supervisor::{Backoff, spawn_critical, spawn_restartable};
use crate::{DaemonError, DataPaths, VERSION};

/// Win32 ERROR_ACCESS_DENIED: returned when the pipe name is already owned.
const ERROR_ACCESS_DENIED: i32 = 5;
const DEFAULT_SECRET_PREFIX: &str = "loadout:";

/// Retention (ADR-008 §4).
const REQUEST_RETENTION: Duration = Duration::from_secs(400 * 24 * 3600);
const RESPONSE_ROUTE_RETENTION: Duration = Duration::from_secs(30 * 24 * 3600);
const RETENTION_INTERVAL: Duration = Duration::from_secs(6 * 3600);
const AUTO_BACKUP_INTERVAL: Duration = Duration::from_secs(24 * 3600);

#[derive(Clone, Debug)]
pub struct DaemonOptions {
    pub data_dir: PathBuf,
    /// Overrides `\\.\pipe\loadout-<sid>` (tests).
    pub pipe_name: Option<String>,
    /// Overrides the single-instance mutex suffix (tests).
    pub instance_id: Option<String>,
    /// Overrides the `gateway.port` setting; `0` picks a free port (tests).
    pub port: Option<u16>,
    /// Credential Manager target-name prefix (tests use their own, ADR-004).
    pub secret_prefix: Option<String>,
    /// Home directory for official-CLI discovery and usage (default: the user profile).
    pub home_dir: Option<PathBuf>,
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
    let (db, recovered_from) =
        tokio::task::spawn_blocking(move || Database::open_or_recover(&db_path))
            .await
            .map_err(|_| DaemonError::ServiceFailed("database open"))??;
    let db = Arc::new(db);
    let recovered_from = recovered_from.map(|p| p.display().to_string());
    if let Some(moved) = &recovered_from {
        tracing::error!(
            moved_to = %moved,
            "the database was damaged; it was moved aside and a new one created. Restore the \
            configuration with `loadout backup import` from the backups folder"
        );
    }
    let settings = {
        let db = db.clone();
        tokio::task::spawn_blocking(move || Resolved::load(&db))
            .await
            .map_err(|_| DaemonError::ServiceFailed("settings load"))??
    };

    // Control plane and the first route table.
    let http =
        HttpClient::new(&HttpConfig::default()).map_err(|e| DaemonError::Config(e.to_string()))?;
    let routes = Arc::new(RouteTableHandle::default());
    let control = Arc::new(ControlPlane::new(
        db.clone(),
        Arc::new(CredentialManagerStore::with_prefix(
            options
                .secret_prefix
                .clone()
                .unwrap_or_else(|| DEFAULT_SECRET_PREFIX.to_owned()),
        )),
        Registry::builtin(),
        routes.clone(),
        http.clone(),
        options
            .home_dir
            .clone()
            .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(".")),
    ));
    control
        .rebuild_routes()
        .await
        .map_err(|e| DaemonError::Config(format!("building routes: {}", e.message)))?;
    match control.seed_latency().await {
        Ok(n) => tracing::info!(samples = n, "latency history loaded"),
        Err(e) => tracing::warn!(error = %e.message, "could not load latency history"),
    }

    // Telemetry writer (ADR-008 §5).
    let (telemetry_writer, telemetry) = TelemetryWriter::spawn(&paths.database, DEFAULT_CAPACITY)?;
    let plane = Arc::new(DataPlane {
        routes: routes.clone(),
        http,
        telemetry: Arc::new(LatencyTap {
            latency: control.latency.clone(),
            policies: control.policies.clone(),
            inner: StorageTelemetry {
                sender: telemetry.clone(),
            },
        }),
        response_routes: Arc::new(ResponseRoutes::new(db.clone(), telemetry.clone())),
        limits: UpstreamLimits {
            first_token_timeout: Duration::from_secs(settings.first_token_timeout_secs),
            max_attempts: settings.max_attempts,
            ..UpstreamLimits::default()
        },
        sessions: Arc::default(),
        policies: control.policies.clone(),
        account_slots: Arc::default(),
    });
    drop(telemetry);

    let port = options.port.unwrap_or(settings.gateway_port);
    let keys = Arc::new(ClientKeyIndex::default());
    let gateway = Gateway::bind(
        GatewayConfig {
            bind: SocketAddr::from(([127, 0, 0, 1], port)),
            allowed_origins: settings.allowed_origins.clone(),
            ..GatewayConfig::default()
        },
        keys.clone(),
        plane.clone(),
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
    {
        let (control, token) = (control.clone(), shutdown.clone());
        spawn_restartable("usage", shutdown.clone(), Backoff::default(), move || {
            crate::usage::poll(control.clone(), token.clone())
        });
    }
    {
        let (control, token) = (control.clone(), shutdown.clone());
        spawn_restartable("latency", shutdown.clone(), Backoff::default(), move || {
            crate::latency::run(control.clone(), token.clone())
        });
    }
    {
        let (control, token) = (control.clone(), shutdown.clone());
        spawn_restartable(
            "oauth-refresh",
            shutdown.clone(),
            Backoff::default(),
            move || crate::oauth::refresh_loop(control.clone(), token.clone()),
        );
    }
    let state = Arc::new(DaemonState {
        db: db.clone(),
        control,
        keys,
        events: ipc.events(),
        shutdown: shutdown.clone(),
        started_at_ms: now_ms(),
        gateway_addr,
        data_dir: paths.root.clone(),
        recovered_from,
    });
    state
        .refresh_keys()
        .await
        .map_err(|e| DaemonError::Config(format!("loading client keys: {}", e.message)))?;
    {
        let (db, policies) = (db.clone(), state.control.policies.clone());
        match tokio::task::spawn_blocking(move || {
            crate::clients::seed_usage(&policies, &db, now_ms())
        })
        .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => tracing::warn!(error = %e, "could not restore client usage counters"),
            Err(_) => tracing::warn!("client usage restore task failed"),
        }
    }

    let gateway_task = spawn_critical("gateway", shutdown.clone(), gateway.serve(shutdown.clone()));
    let ipc_task = spawn_critical(
        "ipc",
        shutdown.clone(),
        ipc.serve(Arc::new(Dispatcher { state }), shutdown.clone()),
    );
    spawn_retention(db.clone(), shutdown.clone());
    spawn_auto_backup(db.clone(), paths.root.clone(), shutdown.clone());
    spawn_signal_listener(shutdown.clone());

    tracing::info!(gateway = %gateway_addr, pipe = %pipe_name, "loadoutd ready");
    shutdown.cancelled().await;
    tracing::info!("loadoutd shutting down");

    let (gateway_result, ipc_result) = tokio::join!(gateway_task, ipc_task);
    drop(plane);
    // Flush queued telemetry (bounded even if stray stream tasks still hold senders).
    let _ = tokio::task::spawn_blocking(move || telemetry_writer.shutdown()).await;
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

/// Periodically deletes expired request metadata and response routes (ADR-008 §4).
fn spawn_retention(db: Arc<Database>, shutdown: CancellationToken) {
    let token = shutdown.clone();
    spawn_restartable("retention", shutdown, Backoff::default(), move || {
        let db = db.clone();
        let token = token.clone();
        async move {
            loop {
                let db2 = db.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let now = now_ms();
                    let requests =
                        db2.prune_requests(now - REQUEST_RETENTION.as_millis() as i64)?;
                    let routes = db2
                        .prune_response_routes(now - RESPONSE_ROUTE_RETENTION.as_millis() as i64)?;
                    db2.prune_probe_samples(now - loadout_router::MAX_SAMPLE_AGE_MS)?;
                    Ok::<_, loadout_storage::StorageError>((requests, routes))
                })
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
                if result != (0, 0) {
                    tracing::info!(
                        requests = result.0,
                        response_routes = result.1,
                        "pruned expired records"
                    );
                }
                tokio::select! {
                    () = tokio::time::sleep(RETENTION_INTERVAL) => {}
                    () = token.cancelled() => return Ok::<(), String>(()),
                }
            }
        }
    });
}

/// Daily configuration backup into `<data>\backups\auto` (ExecPlan 007 §2).
fn spawn_auto_backup(db: Arc<Database>, data_dir: PathBuf, shutdown: CancellationToken) {
    let token = shutdown.clone();
    spawn_restartable("auto-backup", shutdown, Backoff::default(), move || {
        let (db, dir, token) = (db.clone(), data_dir.clone(), token.clone());
        async move {
            loop {
                let (db2, dir2) = (db.clone(), dir.clone());
                match tokio::task::spawn_blocking(move || crate::backup::auto_export(&db2, &dir2))
                    .await
                    .map_err(|e| e.to_string())?
                {
                    Ok(Some(path)) => {
                        tracing::info!(%path, "automatic configuration backup written")
                    }
                    Ok(None) => {}
                    Err(e) => tracing::warn!(error = %e, "automatic configuration backup failed"),
                }
                tokio::select! {
                    () = tokio::time::sleep(AUTO_BACKUP_INTERVAL) => {}
                    () = token.cancelled() => return Ok::<(), String>(()),
                }
            }
        }
    });
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
