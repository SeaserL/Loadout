// SPDX-License-Identifier: Apache-2.0

use clap::Subcommand;
use loadout_ipc::api::{
    self, AccountRef, ModelAddParams, ModelEnableParams, ModelInfo, ModelListParams,
    ModelPriceParams, ModelRef, ModelTierParams,
};

use crate::account_cmd::print_sync;
use crate::{Ctx, Failure, output};

#[derive(Debug, Subcommand)]
pub enum ModelCommand {
    /// Fetch the account's model list from the provider.
    Sync { provider: String, alias: String },
    /// Add a model manually (for providers that cannot list models).
    Add {
        provider: String,
        alias: String,
        /// Upstream model id, e.g. `deepseek-chat`.
        model: String,
        /// Protocols the upstream serves this model with: chat, responses, messages.
        #[arg(long = "protocol")]
        protocols: Vec<String>,
        /// Context window in tokens.
        #[arg(long)]
        context: Option<i64>,
    },
    /// List models and the names to use them by.
    List {
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        account: Option<String>,
        /// Include disabled models.
        #[arg(long)]
        all: bool,
    },
    /// Route to this model again (name: provider/account/model).
    Enable { name: String },
    /// Stop routing to this model.
    Disable { name: String },
    /// Set prices in USD per million tokens (for cost statistics).
    Price {
        name: String,
        #[arg(long)]
        input: Option<f64>,
        #[arg(long)]
        output: Option<f64>,
        #[arg(long)]
        cache_read: Option<f64>,
        #[arg(long)]
        cache_write: Option<f64>,
    },
    /// Remove a model.
    Remove { name: String },
    /// Pin the latency tier of a model (ultra-fast, fast, normal) or return to the measured
    /// tier (auto).
    Tier { name: String, tier: String },
}

fn price(p: Option<f64>) -> String {
    p.map_or_else(|| "-".into(), |v| format!("{v}"))
}

fn print_models(models: &[ModelInfo]) {
    if models.is_empty() {
        println!("No models.");
        return;
    }
    let rows: Vec<Vec<String>> = models
        .iter()
        .map(|m| {
            vec![
                m.name.clone(),
                m.protocols.join(","),
                if m.enabled { "enabled" } else { "disabled" }.to_owned(),
                m.context_window
                    .map_or_else(|| "-".into(), |c| c.to_string()),
                format!("{}/{}", price(m.input_price), price(m.output_price)),
                m.source.clone(),
            ]
        })
        .collect();
    output::table(
        &[
            "NAME",
            "PROTOCOLS",
            "STATUS",
            "CONTEXT",
            "PRICE IN/OUT",
            "SOURCE",
        ],
        &rows,
    );
}

pub async fn run(ctx: &Ctx, cmd: ModelCommand) -> Result<(), Failure> {
    let mut ipc = ctx.connect().await?;
    match cmd {
        ModelCommand::Sync { provider, alias } => {
            let r = ipc
                .call::<api::ModelSync>(&AccountRef { provider, alias })
                .await?;
            if ctx.json {
                return output::json(&r);
            }
            print_sync(&r);
            Ok(())
        }
        ModelCommand::Add {
            provider,
            alias,
            model,
            protocols,
            context,
        } => {
            let m = ipc
                .call::<api::ModelAdd>(&ModelAddParams {
                    provider,
                    account: alias,
                    model,
                    protocols,
                    context_window: context,
                })
                .await?;
            if ctx.json {
                return output::json(&m);
            }
            println!("Added {} ({}).", m.name, m.protocols.join(", "));
            Ok(())
        }
        ModelCommand::List {
            provider,
            account,
            all,
        } => {
            let mut models = ipc
                .call::<api::ModelList>(&ModelListParams { provider, account })
                .await?;
            if !all {
                models.retain(|m| m.enabled);
            }
            if ctx.json {
                return output::json(&models);
            }
            print_models(&models);
            Ok(())
        }
        ModelCommand::Enable { name } => set_enabled(ctx, &mut ipc, name, true).await,
        ModelCommand::Disable { name } => set_enabled(ctx, &mut ipc, name, false).await,
        ModelCommand::Price {
            name,
            input,
            output: out,
            cache_read,
            cache_write,
        } => {
            let m = ipc
                .call::<api::ModelSetPrice>(&ModelPriceParams {
                    name,
                    input,
                    output: out,
                    cache_read,
                    cache_write,
                })
                .await?;
            if ctx.json {
                return output::json(&m);
            }
            println!(
                "{}: input {} / output {} USD per 1M tokens.",
                m.name,
                price(m.input_price),
                price(m.output_price)
            );
            Ok(())
        }
        ModelCommand::Tier { name, tier } => {
            let tier = (tier != "auto").then_some(tier);
            let m = ipc
                .call::<api::ModelSetTier>(&ModelTierParams { name, tier })
                .await?;
            if ctx.json {
                return output::json(&m);
            }
            match &m.tier_override {
                Some(t) => println!("{} is pinned to {t}.", m.name),
                None => println!("{} uses its measured tier.", m.name),
            }
            Ok(())
        }
        ModelCommand::Remove { name } => {
            let r = ipc
                .call::<api::ModelRemove>(&ModelRef { name: name.clone() })
                .await?;
            if ctx.json {
                return output::json(&r);
            }
            if r.revoked {
                println!("Removed {name}.");
                Ok(())
            } else {
                Err(Failure::new(format!("no model {name}")))
            }
        }
    }
}

async fn set_enabled(
    ctx: &Ctx,
    ipc: &mut loadout_ipc::IpcClient,
    name: String,
    enabled: bool,
) -> Result<(), Failure> {
    let m = ipc
        .call::<api::ModelSetEnabled>(&ModelEnableParams { name, enabled })
        .await?;
    if ctx.json {
        return output::json(&m);
    }
    println!(
        "{} is now {}.",
        m.name,
        if m.enabled { "enabled" } else { "disabled" }
    );
    Ok(())
}
