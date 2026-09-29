// SPDX-License-Identifier: Apache-2.0

use loadout_ipc::api::{self, AccountUsage, UsageListParams};

use crate::{Ctx, Failure, output};

fn bar(used_percent: f64) -> String {
    let filled = ((used_percent / 10.0).round() as usize).min(10);
    format!("{}{}", "█".repeat(filled), "░".repeat(10 - filled))
}

fn money(v: f64, currency: &str) -> String {
    match currency {
        "USD" => format!("${v:.2}"),
        "CNY" => format!("¥{v:.2}"),
        other => format!("{v:.2} {other}"),
    }
}

fn print_account(u: &AccountUsage, now_ms: i64) {
    let updated = match (u.fetched_at_ms, &u.confidence) {
        (Some(at), Some(c)) => format!("updated {} ago ({c})", output::duration(now_ms - at)),
        (Some(at), None) => format!("updated {} ago", output::duration(now_ms - at)),
        (None, _) if u.supported => "not fetched yet".to_owned(),
        (None, _) => "provider does not report usage".to_owned(),
    };
    println!(
        "{}/{}  {}  {}",
        u.provider, u.alias, u.account_type, updated
    );
    if let Some(label) = &u.account_label {
        println!("  Account   {label}");
    }
    if let Some(err) = &u.error {
        println!("  Error     {err}");
    }
    if let Some(p) = &u.plan {
        let mut parts = Vec::new();
        if let Some(n) = &p.name {
            parts.push(n.clone());
        }
        if let Some(price) = p.price_usd_monthly {
            parts.push(format!("${price:.2}/mo"));
        }
        if let Some(r) = p.renews_at_ms {
            parts.push(format!("renews {}", output::date(r)));
        }
        if !parts.is_empty() {
            println!("  Plan      {}", parts.join(" · "));
        }
    }
    for w in &u.windows {
        let used = w.remaining_percent.map(|r| 100.0 - r);
        let amount = match (w.unit.as_str(), w.used, w.limit) {
            ("percent", _, _) => used.map_or_else(|| "-".into(), |p| format!("{p:>3.0}% used")),
            ("usd", Some(u), Some(l)) => format!("${u:.2} / ${l:.2}"),
            (_, Some(u), Some(l)) => format!("{u} / {l} {}", w.unit),
            (_, Some(u), None) => format!("{u} {} used", w.unit),
            _ => "-".into(),
        };
        let reset = w
            .resets_at_ms
            .map(|r| {
                if r > now_ms {
                    format!("resets in {}", output::duration(r - now_ms))
                } else {
                    "reset due".to_owned()
                }
            })
            .unwrap_or_default();
        println!(
            "  {:<9} {}  {:<16} {}",
            w.label,
            used.map_or_else(|| " ".repeat(10), bar),
            amount,
            reset
        );
    }
    for b in &u.balances {
        let text = match b.granted {
            Some(g) => format!(
                "{} / {}",
                money(b.amount, &b.currency),
                money(g, &b.currency)
            ),
            None => money(b.amount, &b.currency),
        };
        println!("  Balance   {text}");
    }
    println!(
        "  Loadout   today {} req · ${:.2}   month {} req · ${:.2}",
        u.spend_today.requests,
        u.spend_today.cost_usd,
        u.spend_month.requests,
        u.spend_month.cost_usd
    );
}

pub async fn run(ctx: &Ctx, provider: Option<String>, refresh: bool) -> Result<(), Failure> {
    let mut ipc = ctx.connect().await?;
    let list = ipc
        .call::<api::UsageList>(&UsageListParams { provider, refresh })
        .await?;
    if ctx.json {
        return output::json(&list);
    }
    if list.is_empty() {
        println!("No accounts yet.");
        return Ok(());
    }
    let now = output::now_ms();
    for (i, u) in list.iter().enumerate() {
        if i > 0 {
            println!();
        }
        print_account(u, now);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars() {
        assert_eq!(bar(0.0), "░░░░░░░░░░");
        assert_eq!(bar(78.0), "████████░░");
        assert_eq!(bar(150.0), "██████████");
    }
}
