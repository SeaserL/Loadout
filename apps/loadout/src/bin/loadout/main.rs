// SPDX-License-Identifier: Apache-2.0

//! `loadout` — command-line client for `loadoutd` over IPC (ADR-002).

mod account_cmd;
mod autostart_cmd;
mod backup_cmd;
mod client_cmd;
mod config_cmd;
mod daemon_cmd;
mod model_cmd;
mod output;
mod pool_cmd;
mod price_cmd;
mod provider_cmd;
mod scan_cmd;
mod setup_cmd;
mod status_cmd;
mod statusline_cmd;
mod usage_cmd;
mod web_cmd;

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
    /// Manage providers (API endpoints such as OpenRouter or a local server).
    #[command(subcommand)]
    Provider(provider_cmd::ProviderCommand),
    /// Manage accounts (API keys) of a provider.
    #[command(subcommand)]
    Account(account_cmd::AccountCommand),
    /// Manage the models of an account.
    #[command(subcommand)]
    Model(model_cmd::ModelCommand),
    /// Inspect routing.
    #[command(subcommand)]
    Route(status_cmd::RouteCommand),
    /// Export / import the configuration, or copy the database.
    #[command(subcommand)]
    Backup(backup_cmd::BackupCommand),
    /// Manage pools: your own ordered model lists, used as `loadout/<name>`.
    #[command(subcommand)]
    Pool(pool_cmd::PoolCommand),
    /// Compare prices of the same model across your accounts.
    #[command(subcommand)]
    Price(price_cmd::PriceCommand),
    /// Measure latency now with a tiny request (default: every route without data).
    Bench {
        /// provider/account/model, provider/model or loadout/<name>.
        names: Vec<String>,
    },
    /// Find AI tools installed on this machine (Codex, Claude Code, Gemini CLI, Kimi Code…).
    Scan {
        #[command(subcommand)]
        command: Option<scan_cmd::ScanCommand>,
    },
    /// Point Claude Code, Codex or OpenCode at Loadout (backs up their config first).
    #[command(subcommand)]
    Setup(setup_cmd::SetupCommand),
    /// One-line status for Claude Code's status line (reads Claude Code's JSON on stdin).
    Statusline,
    /// Quota, balance and plan of every account, plus what Loadout routed to it.
    Usage {
        /// Only this provider.
        provider: Option<String>,
        /// Fetch fresh usage from the providers first.
        #[arg(long)]
        refresh: bool,
    },
    /// Show recent requests (metadata only; no prompts or responses are stored).
    Logs {
        #[arg(long, default_value_t = 30)]
        limit: usize,
        /// Only this client (name).
        #[arg(long)]
        client: Option<String>,
        /// Only requests whose model or route contains this text.
        #[arg(long)]
        search: Option<String>,
        /// Only failed requests.
        #[arg(long)]
        errors: bool,
        /// Only the last N hours.
        #[arg(long)]
        hours: Option<u32>,
        /// Also show each request's routing decision (skips, failed attempts, outcome).
        #[arg(long, short)]
        verbose: bool,
    },
    /// Requests, tokens, cost and latency grouped by model, route, provider, client or day.
    Stats {
        #[arg(long, value_enum, default_value_t = status_cmd::GroupBy::Model)]
        by: status_cmd::GroupBy,
        /// Period in days (history is kept for 400 days).
        #[arg(long, default_value_t = 7)]
        days: u32,
    },
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
            Command::Provider(cmd) => provider_cmd::run(&ctx, cmd).await,
            Command::Account(cmd) => account_cmd::run(&ctx, cmd).await,
            Command::Model(cmd) => model_cmd::run(&ctx, cmd).await,
            Command::Route(cmd) => status_cmd::route(&ctx, cmd).await,
            Command::Pool(cmd) => pool_cmd::run(&ctx, cmd).await,
            Command::Price(cmd) => price_cmd::run(&ctx, cmd).await,
            Command::Backup(cmd) => backup_cmd::run(&ctx, cmd).await,
            Command::Bench { names } => status_cmd::bench(&ctx, names).await,
            Command::Logs {
                limit,
                client,
                search,
                errors,
                hours,
                verbose,
            } => {
                let filter = loadout_ipc::api::RecentParams {
                    limit,
                    client,
                    search,
                    status: errors.then(|| "error".to_owned()),
                    since_ms: hours.map(|h| output::now_ms() - i64::from(h) * 3_600_000),
                };
                status_cmd::logs(&ctx, filter, verbose).await
            }
            Command::Stats { by, days } => status_cmd::stats(&ctx, by, days).await,
            Command::Usage { provider, refresh } => usage_cmd::run(&ctx, provider, refresh).await,
            Command::Scan { command } => scan_cmd::run(&ctx, command).await,
            Command::Setup(cmd) => setup_cmd::run(&ctx, cmd).await,
            Command::Statusline => statusline_cmd::run(&ctx).await,
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
