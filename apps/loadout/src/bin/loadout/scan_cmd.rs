// SPDX-License-Identifier: Apache-2.0

use clap::Subcommand;
use loadout_ipc::api::{self, DiscoveryImportParams};

use crate::{Ctx, Failure, output};

#[derive(Debug, Subcommand)]
pub enum ScanCommand {
    /// Import a detected tool's login as an account (its credentials stay managed by the
    /// official CLI; Loadout only reads usage).
    Import {
        /// Tool kind from `loadout scan`, e.g. `codex`, `claude-code`, `gemini-cli`, `kimi-code`.
        kind: String,
        /// Account alias (default `main`, or `cli` for providers that also take API keys).
        #[arg(long)]
        alias: Option<String>,
    },
}

pub async fn run(ctx: &Ctx, cmd: Option<ScanCommand>) -> Result<(), Failure> {
    let mut ipc = ctx.connect().await?;
    match cmd {
        None => {
            let tools = ipc.call::<api::DiscoveryScan>(&()).await?;
            if ctx.json {
                return output::json(&tools);
            }
            if tools.is_empty() {
                println!("No known AI tools found on this machine.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = tools
                .iter()
                .map(|t| {
                    let state = match (t.installed, t.logged_in, t.expired) {
                        (_, true, true) => "logged in (expired)",
                        (_, true, false) => "logged in",
                        (true, false, _) => "installed",
                        (false, false, _) => "config only",
                    };
                    let action = if t.imported {
                        "imported".to_owned()
                    } else if t.logged_in {
                        format!("loadout scan import {}", t.kind)
                    } else {
                        "log in with the CLI first".to_owned()
                    };
                    vec![
                        t.display_name.clone(),
                        state.to_owned(),
                        t.auth.clone().unwrap_or_else(|| "-".into()),
                        t.detail.clone().unwrap_or_else(|| "-".into()),
                        if t.inference {
                            "routing + usage"
                        } else {
                            "usage only"
                        }
                        .to_owned(),
                        action,
                    ]
                })
                .collect();
            output::table(
                &["TOOL", "STATE", "AUTH", "PLAN", "LOADOUT USE", "ACTION"],
                &rows,
            );
            Ok(())
        }
        Some(ScanCommand::Import { kind, alias }) => {
            let info = ipc
                .call::<api::DiscoveryImport>(&DiscoveryImportParams { kind, alias })
                .await?;
            if ctx.json {
                return output::json(&info);
            }
            println!(
                "Imported {}/{}. Usage will appear in `loadout usage` shortly.",
                info.provider, info.alias
            );
            Ok(())
        }
    }
}
