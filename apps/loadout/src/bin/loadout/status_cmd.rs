// SPDX-License-Identifier: Apache-2.0

use clap::{Subcommand, ValueEnum};
use loadout_ipc::api::{self, BenchParams, RecentParams, StatsParams};

use crate::{Ctx, Failure, output};

#[derive(Debug, Subcommand)]
pub enum RouteCommand {
    /// Health, latency tier and speed of every route (account + model + protocol).
    Status,
    /// Virtual models (loadout/realtime, loadout/fast, loadout/default) and pools, with the
    /// order in which their routes are tried.
    List,
}

fn ms(v: Option<u32>) -> String {
    v.map_or_else(|| "-".into(), |v| v.to_string())
}

pub async fn route(ctx: &Ctx, cmd: RouteCommand) -> Result<(), Failure> {
    let mut ipc = ctx.connect().await?;
    match cmd {
        RouteCommand::Status => {
            let list = ipc.call::<api::RouteStatus>(&()).await?;
            if ctx.json {
                return output::json(&list);
            }
            if list.is_empty() {
                println!("No routes. Add a provider, an account and models first.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = list
                .iter()
                .map(|t| {
                    vec![
                        t.name.clone(),
                        t.protocol.clone(),
                        t.state.clone(),
                        if t.tier_pinned {
                            format!("{} (pinned)", t.tier)
                        } else {
                            t.tier.clone()
                        },
                        format!("{}/{}", ms(t.ttft_p50_ms), ms(t.ttft_p90_ms)),
                        t.tps.map_or_else(|| "-".into(), |v| format!("{v:.0}")),
                        t.samples.to_string(),
                        format!("{}/{}", t.successes, t.failures),
                        t.unavailable_until_ms
                            .map(output::timestamp)
                            .unwrap_or_default(),
                        t.last_error.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            output::table(
                &[
                    "ROUTE",
                    "PROTOCOL",
                    "STATE",
                    "TIER",
                    "TTFT P50/P90 MS",
                    "TOK/S",
                    "SAMPLES",
                    "OK/FAIL",
                    "UNTIL",
                    "LAST ERROR",
                ],
                &rows,
            );
            Ok(())
        }
        RouteCommand::List => {
            let list = ipc.call::<api::RouteVirtuals>(&()).await?;
            if ctx.json {
                return output::json(&list);
            }
            for v in &list {
                let kind = match &v.ordering {
                    Some(o) => format!("{}, {o} order", v.kind),
                    None => v.kind.clone(),
                };
                println!("{} ({kind})", v.name);
                if v.routes.is_empty() {
                    println!("  (no routes yet)");
                }
                for (i, r) in v.routes.iter().enumerate() {
                    println!("  {}. {r}", i + 1);
                }
            }
            Ok(())
        }
    }
}

pub async fn bench(ctx: &Ctx, names: Vec<String>) -> Result<(), Failure> {
    let mut ipc = ctx.connect().await?;
    if !ctx.json {
        if names.is_empty() {
            println!("Measuring every route without latency data...");
        } else {
            println!("Measuring {}...", names.join(", "));
        }
    }
    let results = ipc.call::<api::BenchRun>(&BenchParams { names }).await?;
    if ctx.json {
        return output::json(&results);
    }
    if results.is_empty() {
        println!("Nothing to measure: every route already has latency data.");
        return Ok(());
    }
    let rows: Vec<Vec<String>> = results
        .iter()
        .map(|r| {
            vec![
                r.name.clone(),
                r.protocol.clone(),
                if r.ok { "ok".into() } else { "failed".into() },
                r.ttft_ms.map_or_else(|| "-".into(), |t| format!("{t} ms")),
                format!("{} ms", r.duration_ms),
                r.tier.clone(),
                r.error.clone().unwrap_or_default(),
            ]
        })
        .collect();
    output::table(
        &[
            "ROUTE", "PROTOCOL", "RESULT", "TTFT", "TOTAL", "TIER", "ERROR",
        ],
        &rows,
    );
    println!("A tier needs at least 3 measurements; real requests count too.");
    Ok(())
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum GroupBy {
    Model,
    Route,
    Provider,
    Client,
    /// UTC calendar day.
    Day,
}

impl GroupBy {
    fn as_str(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Route => "route",
            Self::Provider => "provider",
            Self::Client => "client",
            Self::Day => "day",
        }
    }
}

pub async fn stats(ctx: &Ctx, by: GroupBy, days: u32) -> Result<(), Failure> {
    let mut ipc = ctx.connect().await?;
    let rows = ipc
        .call::<api::StatsSummary>(&StatsParams {
            since_ms: output::now_ms() - i64::from(days.max(1)) * 86_400_000,
            group_by: by.as_str().to_owned(),
        })
        .await?;
    if ctx.json {
        return output::json(&rows);
    }
    if rows.is_empty() {
        println!("No requests in the last {days} days.");
        return Ok(());
    }
    let table: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            vec![
                r.key.clone(),
                r.requests.to_string(),
                r.errors.to_string(),
                format!("{}/{}", r.input_tokens, r.output_tokens),
                format!("${:.4}", r.cost_usd),
                r.avg_ttft_ms
                    .map_or_else(|| "-".into(), |v| format!("{v:.0} ms")),
                r.avg_tps.map_or_else(|| "-".into(), |v| format!("{v:.0}")),
            ]
        })
        .collect();
    output::table(
        &[
            by.as_str(),
            "REQUESTS",
            "ERRORS",
            "TOKENS IN/OUT",
            "COST",
            "AVG TTFT",
            "AVG TOK/S",
        ],
        &table,
    );
    Ok(())
}

pub async fn logs(ctx: &Ctx, filter: RecentParams) -> Result<(), Failure> {
    let mut ipc = ctx.connect().await?;
    let list = ipc.call::<api::RequestsRecent>(&filter).await?;
    if ctx.json {
        return output::json(&list);
    }
    if list.is_empty() {
        println!("No requests yet.");
        return Ok(());
    }
    let rows: Vec<Vec<String>> = list
        .iter()
        .map(|r| {
            let status = match &r.error_category {
                Some(c) => format!("{} ({c})", r.status),
                None => r.status.clone(),
            };
            vec![
                output::timestamp(r.ts_ms),
                r.client.clone().unwrap_or_else(|| "-".into()),
                r.requested_model.clone(),
                r.route.clone().unwrap_or_else(|| "-".into()),
                format!(
                    "{}→{}",
                    r.protocol_in,
                    r.protocol_out.as_deref().unwrap_or("-")
                ),
                status,
                format!(
                    "{}/{}",
                    r.input_tokens.map_or_else(|| "-".into(), |t| t.to_string()),
                    r.output_tokens
                        .map_or_else(|| "-".into(), |t| t.to_string())
                ),
                r.ttft_ms.map_or_else(|| "-".into(), |t| format!("{t} ms")),
                format!("{} ms", r.duration_ms),
            ]
        })
        .collect();
    output::table(
        &[
            "TIME",
            "CLIENT",
            "MODEL",
            "ROUTE",
            "PROTOCOL",
            "STATUS",
            "TOKENS IN/OUT",
            "TTFT",
            "TOTAL",
        ],
        &rows,
    );
    Ok(())
}
