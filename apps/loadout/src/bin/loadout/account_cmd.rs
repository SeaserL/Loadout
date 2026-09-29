// SPDX-License-Identifier: Apache-2.0

use std::io::Read;

use clap::{Args, Subcommand};
use loadout_ipc::api::{
    self, AccountAddParams, AccountEnableParams, AccountInfo, AccountKeyParams, AccountListParams,
    AccountRef, ApiKey, SyncResult,
};

use crate::{Ctx, Failure, output};

#[derive(Debug, Args)]
pub struct KeyInput {
    /// Read the API key from standard input instead of prompting.
    #[arg(long)]
    api_key_stdin: bool,
}

#[derive(Debug, Subcommand)]
pub enum AccountCommand {
    /// Add an account (you are prompted for its API key; it is stored in Windows Credential
    /// Manager, never in the database).
    Add {
        provider: String,
        /// Short name used in model names, e.g. `main` in `deepseek/main/deepseek-chat`.
        alias: String,
        #[command(flatten)]
        key: KeyInput,
        /// The account has no API key (e.g. a local server).
        #[arg(long, conflicts_with = "api_key_stdin")]
        no_key: bool,
        /// Do not fetch the model list now.
        #[arg(long)]
        no_sync: bool,
        #[arg(long)]
        name: Option<String>,
    },
    /// List accounts.
    List { provider: Option<String> },
    /// Route requests to this account again.
    Enable { provider: String, alias: String },
    /// Stop routing requests to this account.
    Disable { provider: String, alias: String },
    /// Replace the account's API key.
    SetKey {
        provider: String,
        alias: String,
        #[command(flatten)]
        key: KeyInput,
    },
    /// Remove the account, its key and its models.
    Remove { provider: String, alias: String },
    /// Record the subscription plan, monthly price and next renewal date (for the usage
    /// view and subscription value statistics).
    Plan {
        provider: String,
        alias: String,
        /// Plan name, e.g. "Pro".
        #[arg(long)]
        name: Option<String>,
        /// Monthly price in USD.
        #[arg(long)]
        price: Option<f64>,
        /// Next renewal date, YYYY-MM-DD.
        #[arg(long)]
        renews: Option<String>,
    },
}

fn read_key(input: &KeyInput, what: &str) -> Result<Option<String>, Failure> {
    let raw = if input.api_key_stdin {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| Failure::new(format!("reading stdin: {e}")))?;
        s
    } else {
        loadout_platform::read_hidden_line(&format!("API key for {what} (input hidden): "))
            .map_err(|e| Failure::new(format!("reading the key: {e}")))?
    };
    let key = raw.trim().to_owned();
    Ok((!key.is_empty()).then_some(key))
}

pub fn print_sync(sync: &SyncResult) {
    println!(
        "Models: {} total ({} new, {} updated, {} no longer offered).",
        sync.total, sync.added, sync.updated, sync.disabled
    );
    if !sync.ambiguous.is_empty() {
        println!(
            "Warning: these model ids start with an account alias and may be ambiguous: {}",
            sync.ambiguous.join(", ")
        );
    }
}

fn print_accounts(list: &[AccountInfo]) {
    if list.is_empty() {
        println!("No accounts. Add one with `loadout account add <provider> <alias>`.");
        return;
    }
    let rows: Vec<Vec<String>> = list
        .iter()
        .map(|a| {
            vec![
                format!("{}/{}", a.provider, a.alias),
                a.account_type.clone(),
                if a.enabled { "enabled" } else { "disabled" }.to_owned(),
                if a.has_key { "yes" } else { "no" }.to_owned(),
                format!("{}/{}", a.enabled_models, a.models),
            ]
        })
        .collect();
    output::table(&["ACCOUNT", "TYPE", "STATUS", "KEY", "MODELS"], &rows);
}

pub async fn run(ctx: &Ctx, cmd: AccountCommand) -> Result<(), Failure> {
    // Read secrets before connecting so a slow prompt does not hold a pipe instance.
    match cmd {
        AccountCommand::Add {
            provider,
            alias,
            key,
            no_key,
            no_sync,
            name,
        } => {
            let api_key = if no_key {
                None
            } else {
                read_key(&key, &format!("{provider}/{alias}"))?
            };
            let mut ipc = ctx.connect().await?;
            let added = ipc
                .call::<api::AccountAdd>(&AccountAddParams {
                    provider,
                    alias,
                    api_key: api_key.map(ApiKey),
                    display_name: name,
                    sync_models: !no_sync,
                })
                .await?;
            if ctx.json {
                return output::json(&added);
            }
            println!(
                "Added account {}/{}.",
                added.account.provider, added.account.alias
            );
            if let Some(sync) = &added.sync {
                print_sync(sync);
            }
            if let Some(err) = &added.sync_error {
                println!("Could not fetch models: {err}");
                println!(
                    "Add models manually with `loadout model add {} {} <model>`.",
                    added.account.provider, added.account.alias
                );
            }
            Ok(())
        }
        AccountCommand::SetKey {
            provider,
            alias,
            key,
        } => {
            let api_key = read_key(&key, &format!("{provider}/{alias}"))?
                .ok_or_else(|| Failure::new("no API key given"))?;
            let mut ipc = ctx.connect().await?;
            let info = ipc
                .call::<api::AccountSetKey>(&AccountKeyParams {
                    provider,
                    alias,
                    api_key: ApiKey(api_key),
                })
                .await?;
            if ctx.json {
                return output::json(&info);
            }
            println!("Updated the API key of {}/{}.", info.provider, info.alias);
            Ok(())
        }
        AccountCommand::List { provider } => {
            let mut ipc = ctx.connect().await?;
            let list = ipc
                .call::<api::AccountList>(&AccountListParams { provider })
                .await?;
            if ctx.json {
                return output::json(&list);
            }
            print_accounts(&list);
            Ok(())
        }
        AccountCommand::Enable { provider, alias } => set_enabled(ctx, provider, alias, true).await,
        AccountCommand::Disable { provider, alias } => {
            set_enabled(ctx, provider, alias, false).await
        }
        AccountCommand::Plan {
            provider,
            alias,
            name,
            price,
            renews,
        } => {
            let renews_at_ms =
                match renews {
                    Some(d) => Some(output::parse_date(&d).ok_or_else(|| {
                        Failure::new(format!("invalid date {d:?}; use YYYY-MM-DD"))
                    })?),
                    None => None,
                };
            let mut ipc = ctx.connect().await?;
            let info = ipc
                .call::<api::AccountSetPlan>(&api::AccountPlanParams {
                    provider,
                    alias,
                    name,
                    price_usd_monthly: price,
                    renews_at_ms,
                })
                .await?;
            if ctx.json {
                return output::json(&info);
            }
            println!("Saved the plan of {}/{}.", info.provider, info.alias);
            Ok(())
        }
        AccountCommand::Remove { provider, alias } => {
            let mut ipc = ctx.connect().await?;
            let r = ipc
                .call::<api::AccountRemove>(&AccountRef {
                    provider: provider.clone(),
                    alias: alias.clone(),
                })
                .await?;
            if ctx.json {
                return output::json(&r);
            }
            if r.revoked {
                println!("Removed {provider}/{alias} with its key and models.");
                Ok(())
            } else {
                Err(Failure::new(format!("no account {provider}/{alias}")))
            }
        }
    }
}

async fn set_enabled(
    ctx: &Ctx,
    provider: String,
    alias: String,
    enabled: bool,
) -> Result<(), Failure> {
    let mut ipc = ctx.connect().await?;
    let info = ipc
        .call::<api::AccountSetEnabled>(&AccountEnableParams {
            provider,
            alias,
            enabled,
        })
        .await?;
    if ctx.json {
        return output::json(&info);
    }
    println!(
        "{}/{} is now {}.",
        info.provider,
        info.alias,
        if info.enabled { "enabled" } else { "disabled" }
    );
    Ok(())
}
