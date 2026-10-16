// SPDX-License-Identifier: Apache-2.0

use clap::Subcommand;
use loadout_ipc::api::{
    self, SubpoolActivateParams, SubpoolInfo, SubpoolRef, SubpoolSaveParams, SubpoolState,
};

use crate::{Ctx, Failure, output};

#[derive(Debug, Subcommand)]
pub enum SubpoolCommand {
    /// List sub-pools and show which one is active.
    List,
    /// Create a sub-pool, or replace the accounts of an existing one.
    Save {
        /// Sub-pool name.
        name: String,
        /// Accounts as provider/alias.
        #[arg(required = true)]
        accounts: Vec<String>,
        /// Rename this sub-pool to NAME.
        #[arg(long)]
        rename_from: Option<String>,
    },
    /// Remove a sub-pool (its accounts are kept). Removing the active one returns to the
    /// main pool.
    Remove { name: String },
    /// Switch routing to a sub-pool, or back to every account with `main`.
    Use { name: String },
}

fn print_state(s: &SubpoolState) {
    match &s.active {
        Some(name) => println!("Active: sub-pool {name} (only its accounts are routable)"),
        None => println!("Active: main pool (every account)"),
    }
    println!(
        "Keep after restart: {} (loadout config set subpool.remember_active true|false)",
        if s.remember { "yes" } else { "no" }
    );
    if s.subpools.is_empty() {
        println!(
            "No sub-pools. Create one with `loadout subpool save <name> <provider/alias...>`."
        );
    }
    for p in &s.subpools {
        print_one(p);
    }
}

fn print_one(p: &SubpoolInfo) {
    let mark = if p.active { "*" } else { " " };
    println!("{mark} {} ({} accounts)", p.name, p.accounts.len());
    for a in &p.accounts {
        println!("    {a}");
    }
}

pub async fn run(ctx: &Ctx, cmd: SubpoolCommand) -> Result<(), Failure> {
    let mut ipc = ctx.connect().await?;
    match cmd {
        SubpoolCommand::List => {
            let s = ipc.call::<api::SubpoolList>(&()).await?;
            if ctx.json {
                return output::json(&s);
            }
            print_state(&s);
        }
        SubpoolCommand::Save {
            name,
            accounts,
            rename_from,
        } => {
            let p = ipc
                .call::<api::SubpoolSave>(&SubpoolSaveParams {
                    name,
                    rename_from,
                    accounts,
                })
                .await?;
            if ctx.json {
                return output::json(&p);
            }
            print_one(&p);
        }
        SubpoolCommand::Remove { name } => {
            let r = ipc.call::<api::SubpoolRemove>(&SubpoolRef { name }).await?;
            if ctx.json {
                return output::json(&r);
            }
            println!(
                "{}",
                if r.revoked {
                    "Removed."
                } else {
                    "No such sub-pool."
                }
            );
        }
        SubpoolCommand::Use { name } => {
            let name = (!name.eq_ignore_ascii_case("main") && name != "主号池").then_some(name);
            let s = ipc
                .call::<api::SubpoolActivate>(&SubpoolActivateParams { name })
                .await?;
            if ctx.json {
                return output::json(&s);
            }
            print_state(&s);
        }
    }
    Ok(())
}
