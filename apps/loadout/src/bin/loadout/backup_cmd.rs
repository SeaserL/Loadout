// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use clap::Subcommand;
use loadout_ipc::api::{self, BackupPathParams, ImportParams};

use crate::{Ctx, Failure, output};

#[derive(Debug, Subcommand)]
pub enum BackupCommand {
    /// Export providers, accounts, models, pools, client keys (hashes only) and settings to
    /// JSON. API keys are never exported.
    Export {
        /// Output file (default: <data dir>\backups\loadout-config-<time>.json).
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Merge an exported configuration: missing items are created, existing ones kept.
    Import { file: PathBuf },
    /// Copy the whole database, including request history (no secrets).
    Database {
        /// Output file (default: <data dir>\backups\loadout-<time>.db); must not exist.
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

/// The daemon has its own working directory: send absolute paths.
fn absolute(p: PathBuf) -> Result<String, Failure> {
    let p = if p.is_absolute() {
        p
    } else {
        std::env::current_dir()
            .map_err(|e| Failure::new(e.to_string()))?
            .join(p)
    };
    p.to_str()
        .map(str::to_owned)
        .ok_or_else(|| Failure::new("the path is not valid Unicode"))
}

pub async fn run(ctx: &Ctx, cmd: BackupCommand) -> Result<(), Failure> {
    let mut ipc = ctx.connect().await?;
    match cmd {
        BackupCommand::Export { out } => {
            let path = out.map(absolute).transpose()?;
            let r = ipc
                .call::<api::BackupExport>(&BackupPathParams { path })
                .await?;
            if ctx.json {
                return output::json(&r);
            }
            println!("Exported to {}", r.path);
            println!(
                "  {} providers, {} accounts, {} models, {} pools, {} client keys",
                r.providers, r.accounts, r.models, r.pools, r.clients
            );
            println!("API keys are not included; enter them again after importing.");
            Ok(())
        }
        BackupCommand::Import { file } => {
            let path = absolute(file)?;
            let r = ipc
                .call::<api::BackupImport>(&ImportParams { path })
                .await?;
            if ctx.json {
                return output::json(&r);
            }
            println!(
                "Imported {} providers, {} accounts, {} models, {} pools, {} client keys, {} settings.",
                r.providers, r.accounts, r.models, r.pools, r.clients, r.settings
            );
            if !r.skipped.is_empty() {
                println!("Skipped (already present): {}", r.skipped.join(", "));
            }
            for a in &r.accounts_needing_keys {
                println!(
                    "Enter the API key again: loadout account set-key {}",
                    a.replace('/', " ")
                );
            }
            Ok(())
        }
        BackupCommand::Database { out } => {
            let path = out.map(absolute).transpose()?;
            let r = ipc
                .call::<api::BackupDatabase>(&BackupPathParams { path })
                .await?;
            if ctx.json {
                return output::json(&r);
            }
            println!("Database copied to {}", r.path);
            Ok(())
        }
    }
}
