// SPDX-License-Identifier: Apache-2.0

use clap::Subcommand;
use loadout_ipc::api::{
    self, ClientCreateParams, ClientNameParams, ClientPolicy, ClientPolicyParams,
};

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
    /// Set what a client key may do (replaces its current policy).
    Policy {
        name: String,
        /// Model used when the app sends a model name Loadout does not know (e.g. gpt-4o).
        #[arg(long)]
        default_model: Option<String>,
        /// Allowed model name or prefix ending in `*` (repeatable), e.g. `loadout/*`.
        #[arg(long = "allow", value_name = "MODEL")]
        allowed_models: Vec<String>,
        /// Maximum input + output tokens per day (UTC).
        #[arg(long)]
        daily_tokens: Option<u64>,
        /// Maximum estimated cost per day in USD (UTC).
        #[arg(long)]
        daily_cost: Option<f64>,
        /// Maximum requests per minute.
        #[arg(long)]
        rpm: Option<u32>,
        /// Remove every restriction.
        #[arg(long, conflicts_with_all = ["default_model", "allowed_models", "daily_tokens", "daily_cost", "rpm"])]
        clear: bool,
    },
}

fn describe(p: &ClientPolicy) -> String {
    let mut parts = Vec::new();
    if let Some(m) = &p.default_model {
        parts.push(format!("default {m}"));
    }
    if !p.allowed_models.is_empty() {
        parts.push(format!("allow {}", p.allowed_models.join(",")));
    }
    if let Some(t) = p.daily_token_limit {
        parts.push(format!("{t} tokens/day"));
    }
    if let Some(c) = p.daily_cost_limit_usd {
        parts.push(format!("${c}/day"));
    }
    if let Some(r) = p.requests_per_minute {
        parts.push(format!("{r}/min"));
    }
    if parts.is_empty() {
        "-".into()
    } else {
        parts.join("; ")
    }
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
                        format!("{} tok · ${:.2}", c.tokens_today, c.cost_today_usd),
                        describe(&c.policy),
                    ]
                })
                .collect();
            output::table(
                &["NAME", "KEY", "STATUS", "CREATED", "TODAY", "POLICY"],
                &rows,
            );
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
        ClientCommand::Policy {
            name,
            default_model,
            allowed_models,
            daily_tokens,
            daily_cost,
            rpm,
            clear: _,
        } => {
            let info = ipc
                .call::<api::ClientSetPolicy>(&ClientPolicyParams {
                    name,
                    policy: ClientPolicy {
                        default_model,
                        allowed_models,
                        daily_token_limit: daily_tokens,
                        daily_cost_limit_usd: daily_cost,
                        requests_per_minute: rpm,
                    },
                })
                .await?;
            if ctx.json {
                return output::json(&info);
            }
            println!("{}: {}", info.name, describe(&info.policy));
            Ok(())
        }
    }
}
