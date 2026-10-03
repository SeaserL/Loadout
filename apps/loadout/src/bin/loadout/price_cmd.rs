// SPDX-License-Identifier: Apache-2.0

use clap::Subcommand;
use loadout_ipc::api::{self, PriceCompareParams, PriceGroup, PriceTokenMix};

use crate::{Ctx, Failure, output};

#[derive(Debug, Subcommand)]
pub enum PriceCommand {
    /// Compare what each account charges for the same model, blended with your own token
    /// mix of the last 30 days. Unknown prices are shown as `?`, never as free.
    Compare {
        /// Model id or part of it, e.g. `claude-sonnet-4-5` (default: every model).
        model: Option<String>,
        /// Include disabled accounts and models, and models served by one account only.
        #[arg(long)]
        all: bool,
    },
}

fn usd(p: Option<f64>) -> String {
    p.map_or_else(
        || "?".into(),
        |v| {
            format!("{v:.4}")
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_owned()
        },
    )
}

fn describe_mix(mix: &PriceTokenMix) -> String {
    let total = (mix.input_tokens + mix.output_tokens).max(1) as f64;
    let input_pct = mix.input_tokens as f64 * 100.0 / total;
    let cached_pct = if mix.input_tokens > 0 {
        (mix.cache_read_tokens + mix.cache_write_tokens) as f64 * 100.0 / mix.input_tokens as f64
    } else {
        0.0
    };
    let source = match mix.source.as_str() {
        "model" => format!("your last 30 days on this model, {} requests", mix.requests),
        "all" => format!("your last 30 days on all models, {} requests", mix.requests),
        _ => "default mix, no history".to_owned(),
    };
    format!(
        "{input_pct:.0}% input / {:.0}% output, {cached_pct:.0}% of input cached ({source})",
        100.0 - input_pct
    )
}

fn print_groups(groups: &[PriceGroup]) {
    if groups.is_empty() {
        println!("No matching models.");
        return;
    }
    for (i, g) in groups.iter().enumerate() {
        if i > 0 {
            println!();
        }
        println!("{}  —  {}", g.key, describe_mix(&g.mix));
        let rows: Vec<Vec<String>> = g
            .offers
            .iter()
            .map(|o| {
                vec![
                    o.name.clone(),
                    if o.enabled { "enabled" } else { "disabled" }.to_owned(),
                    usd(o.input_price),
                    usd(o.output_price),
                    usd(o.cache_read_price),
                    usd(o.cache_write_price),
                    usd(o.blended_per_million),
                    o.price_source.clone().unwrap_or_else(|| "unknown".into()),
                ]
            })
            .collect();
        output::table(
            &[
                "ROUTE",
                "STATUS",
                "IN",
                "OUT",
                "CACHE R",
                "CACHE W",
                "BLENDED $/1M",
                "PRICE SOURCE",
            ],
            &rows,
        );
    }
    println!();
    println!("Prices are USD per 1M tokens; local estimates from configured or catalog prices.");
}

pub async fn run(ctx: &Ctx, cmd: PriceCommand) -> Result<(), Failure> {
    let mut ipc = ctx.connect().await?;
    match cmd {
        PriceCommand::Compare { model, all } => {
            let explicit = model.is_some();
            let mut groups = ipc
                .call::<api::PriceCompare>(&PriceCompareParams { model })
                .await?;
            if !all {
                for g in &mut groups {
                    g.offers.retain(|o| o.enabled);
                }
                // Without a model filter, only show models that can actually be compared.
                groups.retain(|g| !g.offers.is_empty() && (explicit || g.offers.len() > 1));
            }
            if ctx.json {
                return output::json(&groups);
            }
            print_groups(&groups);
            Ok(())
        }
    }
}
