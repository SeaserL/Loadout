// SPDX-License-Identifier: Apache-2.0

use clap::Subcommand;
use loadout_ipc::api::{self, EndpointInfo, EndpointRef, EndpointSaveParams};

use crate::{Ctx, Failure, output};

#[derive(Debug, Subcommand)]
pub enum EndpointCommand {
    /// List dedicated endpoints with their URLs, members and models.
    List,
    /// Create or update a dedicated endpoint for one provider (e.g. `enable sensenova`):
    /// clients use its URL and bare model names; requests fail over across its accounts.
    Enable {
        /// Provider slug.
        provider: String,
        /// Endpoint name (default: the provider slug).
        #[arg(long)]
        name: Option<String>,
        /// Member account aliases (default: every account of the provider in the current
        /// pool, including ones added later).
        #[arg(long, value_delimiter = ',')]
        accounts: Option<Vec<String>>,
        /// Model used when a client asks for one the endpoint does not serve.
        #[arg(long)]
        default_model: Option<String>,
        /// Seconds a request may wait for a rate-limited account to recover (0 = never).
        #[arg(long)]
        wait: Option<u32>,
    },
    /// Turn an endpoint off (kept for later).
    Disable { name: String },
    /// Remove an endpoint.
    Remove { name: String },
}

fn print(e: &EndpointInfo) {
    let state = if e.enabled { "on" } else { "off" };
    println!("{} ({}, {state})", e.name, e.provider);
    println!("  OpenAI base URL     {}", e.openai_base_url);
    println!("  Anthropic base URL  {}", e.anthropic_base_url);
    match &e.members {
        None => println!(
            "  Members             every {} account in the current pool",
            e.provider
        ),
        Some(m) => println!("  Members             {}", m.join(", ")),
    }
    println!(
        "  Routable now        {}",
        if e.active_accounts.is_empty() {
            "none".to_owned()
        } else {
            e.active_accounts.join(" -> ")
        }
    );
    if let Some(m) = &e.default_model {
        println!("  Default model       {m}");
    }
    println!("  Wait for recovery   {} s", e.wait_secs);
    println!("  Models              {}", e.models.join(", "));
}

pub async fn run(ctx: &Ctx, cmd: EndpointCommand) -> Result<(), Failure> {
    let mut ipc = ctx.connect().await?;
    match cmd {
        EndpointCommand::List => {
            let list = ipc.call::<api::EndpointList>(&()).await?;
            if ctx.json {
                return output::json(&list);
            }
            if list.is_empty() {
                println!("No endpoints. Create one with `loadout endpoint enable <provider>`.");
            }
            list.iter().for_each(print);
        }
        EndpointCommand::Enable {
            provider,
            name,
            accounts,
            default_model,
            wait,
        } => {
            let e = ipc
                .call::<api::EndpointSave>(&EndpointSaveParams {
                    name,
                    provider,
                    enabled: true,
                    members: accounts,
                    default_model,
                    wait_secs: wait,
                })
                .await?;
            if ctx.json {
                return output::json(&e);
            }
            print(&e);
        }
        EndpointCommand::Disable { name } => {
            let current = ipc
                .call::<api::EndpointList>(&())
                .await?
                .into_iter()
                .find(|e| e.name == name)
                .ok_or_else(|| Failure::new(format!("endpoint {name} not found")))?;
            let e = ipc
                .call::<api::EndpointSave>(&EndpointSaveParams {
                    name: Some(current.name),
                    provider: current.provider,
                    enabled: false,
                    members: current.members,
                    default_model: current.default_model,
                    wait_secs: Some(current.wait_secs),
                })
                .await?;
            if ctx.json {
                return output::json(&e);
            }
            print(&e);
        }
        EndpointCommand::Remove { name } => {
            let r = ipc
                .call::<api::EndpointRemove>(&EndpointRef { name })
                .await?;
            if ctx.json {
                return output::json(&r);
            }
            println!(
                "{}",
                if r.revoked {
                    "Removed."
                } else {
                    "No such endpoint."
                }
            );
        }
    }
    Ok(())
}
