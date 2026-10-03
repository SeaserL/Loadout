// SPDX-License-Identifier: Apache-2.0

//! `loadout statusline`: a one-line status for Claude Code's status line
//! (docs/research/2026-10-04-feature-landscape.md §2, demo).
//!
//! Claude Code runs the command with a JSON object on stdin. When Claude Code itself runs on
//! a Claude subscription, that JSON carries the official `rate_limits`; they are shown as is.
//! The Loadout part comes from the daemon over IPC with a short timeout, so a stopped daemon
//! never blocks Claude Code. Configure with:
//! `"statusLine": {"type": "command", "command": "loadout statusline"}` in settings.json.

use std::io::Read;
use std::time::Duration;

use loadout_ipc::api::{self, UsageListParams};
use serde_json::Value;

use crate::{Ctx, Failure};

const TIMEOUT: Duration = Duration::from_millis(800);

fn pct(v: Option<f64>) -> String {
    v.map_or_else(|| "?".into(), |p| format!("{p:.0}%"))
}

/// The official subscription limits Claude Code passes on stdin, if any.
pub(crate) fn claude_limits(input: &Value) -> Option<String> {
    let rl = input.get("rate_limits")?;
    let used = |k: &str| {
        rl.get(k).and_then(|w| {
            w.get("used_percentage")
                .or_else(|| w.get("utilization"))
                .and_then(Value::as_f64)
        })
    };
    let (five, week) = (used("five_hour"), used("seven_day"));
    (five.is_some() || week.is_some())
        .then(|| format!("Claude 5h {} · 7d {}", pct(five), pct(week)))
}

async fn loadout_part(ctx: &Ctx) -> Option<String> {
    let mut ipc = ctx.connect().await.ok()?;
    let status = ipc.call::<api::Status>(&()).await.ok()?;
    let usage = ipc
        .call::<api::UsageList>(&UsageListParams {
            provider: None,
            refresh: false,
        })
        .await
        .ok()?;
    let today: f64 = usage.iter().map(|u| u.spend_today.cost_usd).sum();
    let requests: i64 = usage.iter().map(|u| u.spend_today.requests).sum();
    // The account with the least quota left.
    let tightest = usage
        .iter()
        .flat_map(|u| {
            u.windows.iter().filter_map(move |w| {
                w.remaining_percent.map(|r| {
                    (
                        r,
                        format!("{}/{} {} {:.0}% left", u.provider, u.alias, w.label, r),
                    )
                })
            })
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, s)| s);
    let mut parts = vec![format!(
        "Loadout {} · {} req ${:.2} today",
        if status.gateway.listening {
            "●"
        } else {
            "○"
        },
        requests,
        today
    )];
    parts.extend(tightest);
    Some(parts.join(" · "))
}

pub async fn run(ctx: &Ctx) -> Result<(), Failure> {
    let mut raw = String::new();
    let _ = std::io::stdin().read_to_string(&mut raw);
    let input: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
    let mut out = Vec::new();
    if let Some(model) = input
        .get("model")
        .and_then(|m| m.get("display_name"))
        .and_then(Value::as_str)
    {
        out.push(model.to_owned());
    }
    if let Some(limits) = claude_limits(&input) {
        out.push(limits);
    }
    match tokio::time::timeout(TIMEOUT, loadout_part(ctx)).await {
        Ok(Some(s)) => out.push(s),
        _ => out.push("Loadout ○".into()),
    }
    println!("{}", out.join(" | "));
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn reads_official_limits_from_claude_code_input() {
        let v = json!({"rate_limits": {"five_hour": {"used_percentage": 37.4}, "seven_day": {"used_percentage": 81}}});
        assert_eq!(claude_limits(&v).as_deref(), Some("Claude 5h 37% · 7d 81%"));
        assert!(claude_limits(&json!({"model": {}})).is_none());
    }
}
