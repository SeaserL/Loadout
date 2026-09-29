// SPDX-License-Identifier: Apache-2.0

//! `loadout` — command-line client for `loadoutd` over IPC (ADR-002).

mod autostart_cmd;
mod client_cmd;
mod config_cmd;
mod daemon_cmd;
mod output;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use loadout_ipc::{IpcClient, IpcError};

/// Exit code when the daemon is not running.
pub const EXIT_NOT_RUNNING: u8 = 3;

#[derive(Debug, Parser)]
#[command(
    name = "loadout",
    version,
    about = "Loadout — All your AI, one endpoint."
)]
struct Cli {
    /// Print machine-readable JSON.
    #[arg(long, global = true)]
    json: bool,

    #[arg(long, global = true, hide = true)]
    pipe_name: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start, stop or inspect the background daemon.
    #[command(subcommand)]
    Daemon(daemon_cmd::DaemonCommand),
    /// Start the daemon automatically when you sign in to Windows.
    #[command(subcommand)]
    Autostart(autostart_cmd::AutostartCommand),
    /// Manage local client API keys for the gateway.
    #[command(subcommand)]
    Client(client_cmd::ClientCommand),
    /// View and change settings.
    #[command(subcommand)]
    Config(config_cmd::ConfigCommand),
}

/// Global options shared by subcommands.
#[derive(Clone, Debug)]
pub struct Ctx {
    pub json: bool,
    pub pipe_name: String,
    pub pipe_overridden: bool,
}

impl Ctx {
    pub async fn connect(&self) -> Result<IpcClient, IpcError> {
        IpcClient::connect(&self.pipe_name, "loadout-cli").await
    }
}

/// Command failure with its exit code.
#[derive(Debug)]
pub struct Failure {
    pub code: u8,
    pub message: String,
}

impl Failure {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            code: 1,
            message: message.into(),
        }
    }

    pub fn not_running() -> Self {
        Self {
            code: EXIT_NOT_RUNNING,
            message: "the Loadout daemon is not running; start it with `loadout daemon start`"
                .into(),
        }
    }
}

impl From<IpcError> for Failure {
    fn from(e: IpcError) -> Self {
        match e {
            IpcError::NotRunning => Self::not_running(),
            IpcError::Remote(body) => Self::new(body.message),
            other => Self::new(other.to_string()),
        }
    }
}

impl From<anyhow::Error> for Failure {
    fn from(e: anyhow::Error) -> Self {
        Self::new(format!("{e:#}"))
    }
}

/// Directory holding this executable; `loadoutd.exe` is installed next to it.
pub fn install_dir() -> Result<PathBuf, Failure> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
        .ok_or_else(|| Failure::new("cannot determine the location of loadout.exe"))
}

pub fn daemon_exe() -> Result<PathBuf, Failure> {
    let path = install_dir()?.join("loadoutd.exe");
    if path.is_file() {
        Ok(path)
    } else {
        Err(Failure::new(format!(
            "loadoutd.exe not found next to loadout.exe (expected {})",
            path.display()
        )))
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let (pipe_name, pipe_overridden) = match cli.pipe_name {
        Some(p) => (p, true),
        None => match loadout_platform::current_user_sid() {
            Ok(sid) => (loadout_ipc::pipe_name_for(&sid), false),
            Err(e) => {
                eprintln!("loadout: {e}");
                return ExitCode::from(1);
            }
        },
    };
    let ctx = Ctx {
        json: cli.json,
        pipe_name,
        pipe_overridden,
    };

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("loadout: {e}");
            return ExitCode::from(1);
        }
    };
    let result = runtime.block_on(async {
        match cli.command {
            Command::Daemon(cmd) => daemon_cmd::run(&ctx, cmd).await,
            Command::Autostart(cmd) => autostart_cmd::run(&ctx, cmd),
            Command::Client(cmd) => client_cmd::run(&ctx, cmd).await,
            Command::Config(cmd) => config_cmd::run(&ctx, cmd).await,
        }
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(f) => {
            if !f.message.is_empty() {
                eprintln!("loadout: {}", f.message);
            }
            ExitCode::from(f.code)
        }
    }
}
