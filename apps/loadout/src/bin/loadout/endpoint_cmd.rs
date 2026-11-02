// SPDX-License-Identifier: Apache-2.0

use clap::{Subcommand, ValueEnum};
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
    /// Round-robin: each request starts at the next account instead of the first healthy
    /// one, spreading traffic over every member (ADR-015 §5).
    Rotate { name: String, state: OnOff },
    /// Remove an endpoint.
    Remove { name: String },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum OnOff {
    On,
    Off,
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
    println!(
        "  Rotation            {}",
        if e.rotate {
            "on (each request starts at the next account)"
        } else {
            "off (first healthy account in order)"
        }
    );
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
                    rotate: None,
                })
                .await?;
            if ctx.json {
                return output::json(&e);
            }
            print(&e);
        }
        EndpointCommand::Disable { name } => {
            let current = find(&mut ipc, &name).await?;
            let e = ipc
                .call::<api::EndpointSave>(&EndpointSaveParams {
                    enabled: false,
                    ..unchanged(current)
                })
                .await?;
            if ctx.json {
                return output::json(&e);
            }
            print(&e);
        }
        EndpointCommand::Rotate { name, state } => {
            let current = find(&mut ipc, &name).await?;
            let e = ipc
                .call::<api::EndpointSave>(&EndpointSaveParams {
                    rotate: Some(matches!(state, OnOff::On)),
                    ..unchanged(current)
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

async fn find(ipc: &mut loadout_ipc::IpcClient, name: &str) -> Result<EndpointInfo, Failure> {
    ipc.call::<api::EndpointList>(&())
        .await?
        .into_iter()
        .find(|e| e.name == name)
        .ok_or_else(|| Failure::new(format!("endpoint {name} not found")))
}

/// Save parameters that keep every setting of `e`.
fn unchanged(e: EndpointInfo) -> EndpointSaveParams {
    EndpointSaveParams {
        name: Some(e.name),
        provider: e.provider,
        enabled: e.enabled,
        members: e.members,
        default_model: e.default_model,
        wait_secs: Some(e.wait_secs),
        rotate: Some(e.rotate),
    }
}
