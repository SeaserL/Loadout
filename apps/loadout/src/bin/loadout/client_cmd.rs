// SPDX-License-Identifier: Apache-2.0

use clap::Subcommand;
use loadout_ipc::api::{self, ClientCreateParams, ClientNameParams};

use crate::{Ctx, Failure, output};

#[derive(Debug, Subcommand)]
pub enum ClientCommand {
    /// Create a client and print its API key (shown only once).
    Create {
        /// A name that identifies the program using the key, e.g. "OpenCode".
        name: String,
    },
    /// List clients.
    List,
    /// Revoke a client's key immediately.
    Revoke { name: String },
}

pub async fn run(ctx: &Ctx, cmd: ClientCommand) -> Result<(), Failure> {
    let mut ipc = ctx.connect().await?;
    match cmd {
        ClientCommand::Create { name } => {
            let created = ipc
                .call::<api::ClientCreate>(&ClientCreateParams { name })
                .await?;
            if ctx.json {
                return output::json(&created);
            }
            let status = ipc.call::<api::Status>(&()).await?;
            println!(
                "Created client \"{}\" ({})",
                created.client.name, created.client.key_prefix
            );
            println!();
            println!("  API key   {}", created.key);
            println!("  Base URL  http://{}/v1", status.gateway.addr);
            println!();
            println!("The key is shown only once. Store it in the program that will use it.");
            Ok(())
        }
        ClientCommand::List => {
            let clients = ipc.call::<api::ClientList>(&()).await?;
            if ctx.json {
                return output::json(&clients);
            }
            if clients.is_empty() {
                println!("No clients. Create one with `loadout client create <name>`.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = clients
                .iter()
                .map(|c| {
                    vec![
                        c.name.clone(),
                        c.key_prefix.clone(),
                        if c.enabled { "active" } else { "revoked" }.to_owned(),
                        output::timestamp(c.created_at_ms),
                    ]
                })
                .collect();
            output::table(&["NAME", "KEY", "STATUS", "CREATED"], &rows);
            Ok(())
        }
        ClientCommand::Revoke { name } => {
            let result = ipc
                .call::<api::ClientRevoke>(&ClientNameParams { name: name.clone() })
                .await?;
            if ctx.json {
                return output::json(&result);
            }
            if result.revoked {
                println!("Revoked client \"{name}\". Its key stopped working immediately.");
                Ok(())
            } else {
                Err(Failure::new(format!("no active client named \"{name}\"")))
            }
        }
    }
}
