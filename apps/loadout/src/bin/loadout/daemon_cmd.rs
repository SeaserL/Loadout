// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Subcommand};
use loadout_ipc::IpcError;
use loadout_ipc::api::{self, DaemonStatus};

use crate::{Ctx, Failure, daemon_exe, output};

const START_TIMEOUT: Duration = Duration::from_secs(15);
/// Gateway shutdown grace (30 s) plus margin.
const STOP_TIMEOUT: Duration = Duration::from_secs(40);
const POLL: Duration = Duration::from_millis(100);

/// Hidden options that let tests run a daemon fully isolated from the real installation.
#[derive(Debug, Args)]
pub struct TestIsolation {
    #[arg(long, hide = true)]
    instance_id: Option<String>,
    #[arg(long, hide = true)]
    secret_prefix: Option<String>,
    #[arg(long, hide = true)]
    home_dir: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum DaemonCommand {
    /// Start loadoutd in the background.
    Start {
        /// Data directory (default: %LOCALAPPDATA%\Loadout).
        #[arg(long)]
        data_dir: Option<PathBuf>,
        /// Gateway port for this run (overrides the `gateway.port` setting).
        #[arg(long)]
        port: Option<u16>,
        #[command(flatten)]
        isolation: TestIsolation,
    },
    /// Stop the daemon gracefully.
    Stop,
    /// Show whether the daemon is running and where the gateway listens.
    Status,
}

pub async fn run(ctx: &Ctx, cmd: DaemonCommand) -> Result<(), Failure> {
    match cmd {
        DaemonCommand::Start {
            data_dir,
            port,
            isolation,
        } => start(ctx, data_dir, port, isolation).await,
        DaemonCommand::Stop => stop(ctx).await,
        DaemonCommand::Status => status(ctx).await,
    }
}

async fn fetch_status(ctx: &Ctx) -> Result<DaemonStatus, IpcError> {
    ctx.connect().await?.call::<api::Status>(&()).await
}

fn print_status(ctx: &Ctx, s: &DaemonStatus) -> Result<(), Failure> {
    if ctx.json {
        return output::json(s);
    }
    println!("Loadout daemon {} is running (pid {})", s.version, s.pid);
    println!("  Gateway   http://{}/v1", s.gateway.addr);
    println!("  Started   {}", output::timestamp(s.started_at_ms));
    println!("  Data      {}", s.data_dir);
    Ok(())
}

async fn start(
    ctx: &Ctx,
    data_dir: Option<PathBuf>,
    port: Option<u16>,
    isolation: TestIsolation,
) -> Result<(), Failure> {
    if let Ok(s) = fetch_status(ctx).await {
        if !ctx.json {
            println!("Already running.");
        }
        return print_status(ctx, &s);
    }

    let exe = daemon_exe()?;
    let mut args: Vec<OsString> = vec!["--background".into()];
    if let Some(dir) = data_dir {
        args.extend(["--data-dir".into(), dir.into_os_string()]);
    }
    if let Some(port) = port {
        args.extend(["--port".into(), port.to_string().into()]);
    }
    if ctx.pipe_overridden {
        args.extend(["--pipe-name".into(), ctx.pipe_name.clone().into()]);
    }
    if let Some(id) = isolation.instance_id {
        args.extend(["--instance-id".into(), id.into()]);
    }
    if let Some(prefix) = isolation.secret_prefix {
        args.extend(["--secret-prefix".into(), prefix.into()]);
    }
    if let Some(home) = isolation.home_dir {
        args.extend(["--home-dir".into(), home.into_os_string()]);
    }
    let arg_refs: Vec<&OsStr> = args.iter().map(OsString::as_os_str).collect();
    let child = loadout_platform::spawn_detached(&exe, &arg_refs)
        .map_err(|e| Failure::new(format!("cannot start {}: {e}", exe.display())))?;

    let deadline = tokio::time::Instant::now() + START_TIMEOUT;
    loop {
        if let Some(code) = child.try_exit_code().ok().flatten() {
            return Err(Failure::new(explain_exit(code)));
        }
        match fetch_status(ctx).await {
            Ok(s) => {
                if !ctx.json {
                    println!("Started.");
                }
                return print_status(ctx, &s);
            }
            Err(IpcError::NotRunning) => {}
            Err(e) => return Err(e.into()),
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(Failure::new(
                "the daemon did not become ready in time; see the log in <data dir>\\logs",
            ));
        }
        tokio::time::sleep(POLL).await;
    }
}

fn explain_exit(code: u32) -> String {
    match code {
        2 => "another Loadout daemon is already running for this user".into(),
        3 => "the gateway port is already in use; choose another with \
              `loadout config set gateway.port <port>` or `--port`"
            .into(),
        4 => "invalid configuration; see the log in <data dir>\\logs".into(),
        code => format!("loadoutd exited with code {code}; see the log in <data dir>\\logs"),
    }
}

async fn stop(ctx: &Ctx) -> Result<(), Failure> {
    let mut client = ctx.connect().await?;
    client.call::<api::Shutdown>(&()).await?;
    drop(client);

    let deadline = tokio::time::Instant::now() + STOP_TIMEOUT;
    loop {
        match ctx.connect().await {
            Err(IpcError::NotRunning) => break,
            _ if tokio::time::Instant::now() >= deadline => {
                return Err(Failure::new("the daemon did not stop in time"));
            }
            _ => tokio::time::sleep(POLL).await,
        }
    }
    if ctx.json {
        output::json(&serde_json::json!({ "stopped": true }))
    } else {
        println!("Stopped.");
        Ok(())
    }
}

async fn status(ctx: &Ctx) -> Result<(), Failure> {
    match fetch_status(ctx).await {
        Ok(s) => print_status(ctx, &s),
        Err(IpcError::NotRunning) => {
            if ctx.json {
                output::json(&serde_json::json!({ "running": false }))?;
            } else {
                println!("Loadout daemon is not running.");
            }
            Err(Failure {
                code: crate::EXIT_NOT_RUNNING,
                message: String::new(),
            })
        }
        Err(e) => Err(e.into()),
    }
}
