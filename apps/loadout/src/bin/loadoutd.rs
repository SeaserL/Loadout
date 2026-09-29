// SPDX-License-Identifier: Apache-2.0

//! `loadoutd` — Loadout Core daemon (ADR-002).
//!
//! Built for the Windows GUI subsystem so that starting it at login never flashes a console
//! window. When started from a terminal it attaches to that console for its output.

#![windows_subsystem = "windows"]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use loadout_core::{DaemonOptions, DataPaths, logging};

#[derive(Debug, Parser)]
#[command(name = "loadoutd", version, about = "Loadout background daemon")]
struct Args {
    /// Data directory (default: %LOCALAPPDATA%\Loadout).
    #[arg(long)]
    data_dir: Option<PathBuf>,

    /// Gateway port; overrides the `gateway.port` setting for this run.
    #[arg(long)]
    port: Option<u16>,

    /// Also write logs to the terminal that started the daemon.
    #[arg(long)]
    foreground: bool,

    /// Started by login autostart. Accepted for clarity; behaviour is the default.
    #[arg(long)]
    background: bool,

    /// Log filter in RUST_LOG syntax.
    #[arg(long, env = "LOADOUT_LOG", default_value = "info")]
    log: String,

    #[arg(long, hide = true)]
    pipe_name: Option<String>,

    #[arg(long, hide = true)]
    instance_id: Option<String>,

    #[arg(long, hide = true)]
    secret_prefix: Option<String>,

    #[arg(long, hide = true)]
    home_dir: Option<PathBuf>,
}

fn main() -> ExitCode {
    // Without a console, a GUI-subsystem process has nowhere to print; attaching makes
    // `--help`, argument errors and `--foreground` logs visible when run from a terminal.
    loadout_platform::attach_parent_console();
    let args = Args::parse();

    let data_dir = match args.data_dir.clone() {
        Some(dir) => dir,
        None => match loadout_platform::default_data_dir() {
            Ok(dir) => dir,
            Err(e) => {
                eprintln!("loadoutd: {e}");
                return ExitCode::from(1);
            }
        },
    };
    let paths = DataPaths::new(&data_dir);
    let _log_guard = match logging::init(&paths.logs, &args.log, args.foreground) {
        Ok(guard) => guard,
        Err(e) => {
            eprintln!("loadoutd: cannot initialize logging: {e}");
            return ExitCode::from(1);
        }
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("loadoutd")
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!(error = %e, "cannot start async runtime");
            return ExitCode::from(1);
        }
    };

    let options = DaemonOptions {
        data_dir,
        pipe_name: args.pipe_name,
        instance_id: args.instance_id,
        port: args.port,
        secret_prefix: args.secret_prefix,
        home_dir: args.home_dir,
    };
    match runtime.block_on(loadout_core::run(options)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %e, "loadoutd failed");
            eprintln!("loadoutd: {e}");
            ExitCode::from(u8::try_from(e.exit_code()).unwrap_or(1))
        }
    }
}
