// SPDX-License-Identifier: Apache-2.0

use clap::{Subcommand, ValueEnum};
use loadout_ipc::api::{self, PoolInfo, PoolRef, PoolSaveParams};

use crate::{Ctx, Failure, output};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Ordering {
    /// Try the models in the order given.
    Manual,
    /// Fastest first (tier, then output speed), like loadout/default.
    Latency,
}

impl Ordering {
    fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Latency => "latency",
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum PoolCommand {
    /// Create or replace a pool, usable as `loadout/<name>`.
    Set {
        /// Pool name (lowercase letters, digits and dashes).
        name: String,
        /// Models as provider/account/model.
        #[arg(required = true)]
        models: Vec<String>,
        #[arg(long, value_enum, default_value_t = Ordering::Manual)]
        ordering: Ordering,
    },
    /// List pools.
    List,
    /// Remove a pool.
    Remove { name: String },
}

fn print(p: &PoolInfo) {
    println!("loadout/{} ({} order)", p.name, p.ordering);
    for (i, m) in p.members.iter().enumerate() {
        println!("  {}. {m}", i + 1);
    }
}

pub async fn run(ctx: &Ctx, cmd: PoolCommand) -> Result<(), Failure> {
    let mut ipc = ctx.connect().await?;
    match cmd {
        PoolCommand::Set {
            name,
            models,
            ordering,
        } => {
            let p = ipc
                .call::<api::PoolSave>(&PoolSaveParams {
                    name,
                    ordering: ordering.as_str().to_owned(),
                    members: models,
                })
                .await?;
            if ctx.json {
                return output::json(&p);
            }
            print(&p);
            Ok(())
        }
        PoolCommand::List => {
            let pools = ipc.call::<api::PoolList>(&()).await?;
            if ctx.json {
                return output::json(&pools);
            }
            if pools.is_empty() {
                println!("No pools. Create one with `loadout pool set <name> <models...>`.");
            }
            pools.iter().for_each(print);
            Ok(())
        }
        PoolCommand::Remove { name } => {
            let r = ipc
                .call::<api::PoolRemove>(&PoolRef { name: name.clone() })
                .await?;
            if ctx.json {
                return output::json(&r);
            }
            if r.revoked {
                println!("Removed loadout/{name}.");
                Ok(())
            } else {
                Err(Failure::new(format!("no pool {name}")))
            }
        }
    }
}
