// SPDX-License-Identifier: Apache-2.0

use clap::Subcommand;
use loadout_ipc::api::{self, RecentParams};

use crate::{Ctx, Failure, output};

#[derive(Debug, Subcommand)]
pub enum RouteCommand {
    /// Health of every route target (account + model + protocol).
    Status,
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
                        t.unavailable_until_ms
                            .map(output::timestamp)
                            .unwrap_or_default(),
                        format!("{}/{}", t.successes, t.failures),
                        t.last_error.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            output::table(
                &[
                    "ROUTE",
                    "PROTOCOL",
                    "STATE",
                    "UNTIL",
                    "OK/FAIL",
                    "LAST ERROR",
                ],
                &rows,
            );
            Ok(())
        }
    }
}

pub async fn logs(ctx: &Ctx, limit: usize) -> Result<(), Failure> {
    let mut ipc = ctx.connect().await?;
    let list = ipc
        .call::<api::RequestsRecent>(&RecentParams { limit })
        .await?;
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
