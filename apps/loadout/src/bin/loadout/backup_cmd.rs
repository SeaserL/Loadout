// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use clap::Subcommand;
use loadout_ipc::api::{
    self, BackupPathParams, ImportParams, KEYS_EXPORT_CONFIRM, KeysExportParams, KeysImportParams,
    Passphrase,
};

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
    /// Export every API-key account WITH its key, encrypted with a passphrase, to share the
    /// accounts with another Loadout. Asks for two confirmations.
    ExportKeys {
        /// Output file (default: <data dir>\backups\loadout-keys-<time>.loadout-keys).
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Import a file made by `export-keys`: missing providers and accounts are created and
    /// their models synced.
    ImportKeys {
        file: PathBuf,
        /// Replace the key of accounts that already exist (default: skip them).
        #[arg(long)]
        overwrite: bool,
    },
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
        BackupCommand::ExportKeys { out } => {
            let path = out.map(absolute).transpose()?;
            eprintln!(
                "WARNING: the file will contain EVERY API key of this Loadout. Anyone with the \
                 file and the passphrase can use these accounts and spend their quota or money. \
                 Send the passphrase through a different channel than the file, and revoke the \
                 keys on the provider's site if the file leaks."
            );
            let typed = read_line(&format!(
                "Step 1/2: type {KEYS_EXPORT_CONFIRM} to continue: "
            ))?;
            if typed.trim() != KEYS_EXPORT_CONFIRM {
                return Err(Failure::new("export cancelled"));
            }
            let first = hidden("Step 2/2: passphrase (8+ characters, hidden): ")?;
            let second = hidden("Repeat the passphrase: ")?;
            if first != second {
                return Err(Failure::new("the passphrases do not match"));
            }
            let r = ipc
                .call::<api::KeysExport>(&KeysExportParams {
                    path,
                    passphrase: Passphrase(first),
                    confirm: typed.trim().to_owned(),
                })
                .await?;
            if ctx.json {
                return output::json(&r);
            }
            println!(
                "Exported {} accounts of {} providers (encrypted) to {}",
                r.accounts, r.providers, r.path
            );
            if !r.skipped.is_empty() {
                println!("Key unreadable, left out: {}", r.skipped.join(", "));
            }
            Ok(())
        }
        BackupCommand::ImportKeys { file, overwrite } => {
            let path = absolute(file)?;
            let passphrase = hidden("Passphrase of the export (hidden): ")?;
            let r = ipc
                .call::<api::KeysImport>(&KeysImportParams {
                    path,
                    passphrase: Passphrase(passphrase),
                    overwrite,
                })
                .await?;
            if ctx.json {
                return output::json(&r);
            }
            println!(
                "Imported {} providers and {} accounts; {} existing keys replaced.",
                r.providers, r.accounts, r.keys_updated
            );
            if !r.skipped.is_empty() {
                println!("Skipped: {}", r.skipped.join(", "));
            }
            for e in &r.sync_errors {
                println!("Model sync failed: {e}");
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

fn read_line(prompt: &str) -> Result<String, Failure> {
    use std::io::Write;
    eprint!("{prompt}");
    std::io::stderr().flush().ok();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| Failure::new(e.to_string()))?;
    Ok(line)
}

fn hidden(prompt: &str) -> Result<String, Failure> {
    loadout_platform::read_hidden_line(prompt).map_err(|e| Failure::new(e.to_string()))
}
