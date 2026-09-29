// SPDX-License-Identifier: Apache-2.0

//! Usage tracking (ExecPlan 003 §4.1–4.2): background refresh through each module's Usage
//! driver, persistence of the latest snapshot, and quota-driven route availability
//! (ADR-003 §9).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use loadout_ipc::ErrorBody;
use loadout_ipc::api::{AccountUsage, SpendInfo, UsageBalance, UsagePlan, UsageWindow};
use loadout_provider_sdk::usage::{QuotaUnit, UsageReport};
use loadout_provider_sdk::{DriverContext, ProviderError};
use loadout_router::{HealthState, now_ms};
use loadout_secrets::Secret;
use loadout_storage::{Account, BalanceRow, Provider, Spend, UsageRecord, WindowRow};
use tokio_util::sync::CancellationToken;

use crate::control::{ControlPlane, blocking, storage};
use crate::route_build::{provider_config, read_secret};
use crate::settings;

const FETCH_TIMEOUT: Duration = Duration::from_secs(45);
const POLL_TICK: Duration = Duration::from_secs(60);
const MAX_BACKOFF: Duration = Duration::from_secs(3600);
/// How long an exhausted account stays blocked when the upstream gives no reset time.
const DEFAULT_BLOCK: i64 = 3_600_000;

fn unit_name(u: QuotaUnit) -> &'static str {
    match u {
        QuotaUnit::Percent => "percent",
        QuotaUnit::Requests => "requests",
        QuotaUnit::Tokens => "tokens",
        QuotaUnit::Usd => "usd",
        QuotaUnit::Credits => "credits",
    }
}

fn json_name<T: serde::Serialize>(v: &T) -> Option<String> {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
}

/// One account whose usage can be refreshed.
pub(crate) struct UsageTarget {
    pub provider: Provider,
    pub account: Account,
    pub key: Option<Secret>,
}

impl ControlPlane {
    /// Accounts whose provider implements a Usage driver (optionally one provider only).
    async fn usage_targets(&self, provider: Option<String>) -> Result<Vec<UsageTarget>, ErrorBody> {
        let (db, secrets, registry) =
            (self.db.clone(), self.secrets.clone(), self.registry.clone());
        blocking(move || {
            let mut out = Vec::new();
            for p in db.providers().list().map_err(storage)? {
                if provider.as_ref().is_some_and(|x| *x != p.slug) {
                    continue;
                }
                let Some(module) = registry.get(&p.kind) else {
                    continue;
                };
                if !module.descriptor().drivers.usage {
                    continue;
                }
                for a in db.accounts().list(Some(&p.id)).map_err(storage)? {
                    if !a.enabled {
                        continue;
                    }
                    let key = db
                        .accounts()
                        .credentials(&a.id)
                        .map_err(storage)?
                        .into_iter()
                        .find_map(|c| c.secret_ref)
                        .and_then(|r| read_secret(secrets.as_ref(), &r));
                    out.push(UsageTarget {
                        provider: p.clone(),
                        account: a,
                        key,
                    });
                }
            }
            Ok(out)
        })
        .await
    }

    /// Refreshes one account; stores the result and applies quota state to its routes.
    pub(crate) async fn refresh_account_usage(&self, t: &UsageTarget) -> Result<(), String> {
        let module = self
            .registry
            .get(&t.provider.kind)
            .ok_or("unknown provider kind")?
            .clone();
        let config = provider_config(&t.provider);
        let fetched = tokio::time::timeout(
            FETCH_TIMEOUT,
            module.fetch_usage(DriverContext {
                http: &self.http,
                config: &config,
                key: t.key.as_ref(),
                home: Some(&self.home),
            }),
        )
        .await
        .unwrap_or_else(|_| {
            Err(ProviderError::InvalidResponse(
                "usage request timed out".into(),
            ))
        });
        let now = now_ms();
        let db = self.db.clone();
        let account_id = t.account.id.clone();
        match fetched {
            Ok(report) => {
                let record = to_record(&account_id, now, &report);
                blocking(move || db.save_usage(&record).map_err(storage))
                    .await
                    .map_err(|e| e.message)?;
                let reserve = self.quota_reserve_percent().await;
                self.apply_quota(&account_id, &report, reserve, now);
                Ok(())
            }
            Err(e) => {
                let message = e.to_string();
                let m = message.clone();
                let _ =
                    blocking(move || db.save_usage_error(&account_id, now, &m).map_err(storage))
                        .await;
                Err(message)
            }
        }
    }

    async fn quota_reserve_percent(&self) -> f64 {
        let db = self.db.clone();
        blocking(move || {
            Ok(db
                .settings()
                .get(settings::QUOTA_RESERVE)
                .ok()
                .flatten()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0))
        })
        .await
        .unwrap_or(0.0)
    }

    /// Blocks the account's routes while its quota is exhausted (or below the reserve), and
    /// unblocks routes a previous report blocked once quota is available again.
    pub(crate) fn apply_quota(
        &self,
        account_id: &str,
        report: &UsageReport,
        reserve_percent: f64,
        now: i64,
    ) {
        let below_reserve = report.windows.iter().filter(|w| {
            reserve_percent > 0.0
                && w.remaining_fraction()
                    .is_some_and(|r| r * 100.0 < reserve_percent)
        });
        let blocked_until = if report.is_exhausted() {
            Some(report.exhausted_until_ms().unwrap_or(now + DEFAULT_BLOCK))
        } else {
            below_reserve
                .map(|w| w.resets_at_ms.unwrap_or(now + DEFAULT_BLOCK))
                .min()
        };
        for t in self.routes.load().targets() {
            if *t.account_id != *account_id {
                continue;
            }
            match blocked_until {
                Some(until) if until > now => t.health.mark_quota_exhausted(until),
                Some(_) => {}
                None if t.health.state() == HealthState::QuotaExhausted => t.health.reset(),
                None => {}
            }
        }
    }

    /// Usage for display. With `refresh`, fetches from upstreams first.
    pub async fn usage_list(
        &self,
        provider: Option<String>,
        refresh: bool,
    ) -> Result<Vec<AccountUsage>, ErrorBody> {
        if refresh {
            for t in self.usage_targets(provider.clone()).await? {
                if let Err(e) = self.refresh_account_usage(&t).await {
                    tracing::info!(account = %format!("{}/{}", t.provider.slug, t.account.alias), error = %e, "usage refresh failed");
                }
            }
        }
        let (db, registry) = (self.db.clone(), self.registry.clone());
        blocking(move || {
            let now = now_ms();
            let (day_start, month_start) = local_period_starts(now);
            let mut out = Vec::new();
            for p in db.providers().list().map_err(storage)? {
                if provider.as_ref().is_some_and(|x| *x != p.slug) {
                    continue;
                }
                let supported = registry
                    .get(&p.kind)
                    .is_some_and(|m| m.descriptor().drivers.usage);
                for a in db.accounts().list(Some(&p.id)).map_err(storage)? {
                    let record = db.usage(&a.id).map_err(storage)?;
                    let spend = |since| {
                        db.account_spend(&p.slug, &a.alias, since)
                            .map(spend_info)
                            .map_err(storage)
                    };
                    out.push(account_usage(
                        &p,
                        &a,
                        supported,
                        record,
                        spend(day_start)?,
                        spend(month_start)?,
                    ));
                }
            }
            Ok(out)
        })
        .await
    }
}

fn spend_info(s: Spend) -> SpendInfo {
    SpendInfo {
        requests: s.requests,
        input_tokens: s.input_tokens,
        output_tokens: s.output_tokens,
        cost_usd: s.cost_usd,
    }
}

/// Start of today and of this month in UTC (the daemon has no reliable local timezone
/// database; UTC periods are consistent and documented).
fn local_period_starts(now_ms: i64) -> (i64, i64) {
    const DAY: i64 = 86_400_000;
    let day_start = now_ms - now_ms.rem_euclid(DAY);
    let days = day_start / DAY;
    // Civil date from days since epoch (Howard Hinnant).
    let z = days + 719_468;
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5; // 0-based day of month
    (day_start, day_start - d * DAY)
}

fn to_record(account_id: &str, now: i64, r: &UsageReport) -> UsageRecord {
    UsageRecord {
        account_id: account_id.to_owned(),
        fetched_at: now,
        source: json_name(&r.source),
        confidence: json_name(&r.confidence),
        plan_json: r.plan.as_ref().and_then(|p| serde_json::to_string(p).ok()),
        account_label: r.account_label.clone(),
        error: None,
        windows: r
            .windows
            .iter()
            .map(|w| WindowRow {
                scope: w.scope.clone(),
                label: w.label.clone(),
                unit: unit_name(w.unit).to_owned(),
                used: w.used,
                limit: w.limit,
                window_seconds: w.window_seconds.and_then(|s| i64::try_from(s).ok()),
                resets_at: w.resets_at_ms,
            })
            .collect(),
        balances: r
            .balances
            .iter()
            .map(|b| BalanceRow {
                currency: b.currency.clone(),
                amount: b.amount,
                included: b.included,
                topup: b.topup,
                granted: b.granted,
            })
            .collect(),
    }
}

fn account_usage(
    p: &Provider,
    a: &Account,
    supported: bool,
    record: Option<UsageRecord>,
    spend_today: SpendInfo,
    spend_month: SpendInfo,
) -> AccountUsage {
    let reported: Option<loadout_provider_sdk::usage::Plan> = record
        .as_ref()
        .and_then(|r| r.plan_json.as_deref())
        .and_then(|j| serde_json::from_str(j).ok());
    // User-entered plan data wins over reported values (the user knows their price).
    let plan = if a.subscription_plan.is_some()
        || a.price_monthly_usd.is_some()
        || a.renew_at.is_some()
        || reported.is_some()
    {
        Some(UsagePlan {
            name: a
                .subscription_plan
                .clone()
                .or_else(|| reported.as_ref().map(|p| p.name.clone())),
            price_usd_monthly: a
                .price_monthly_usd
                .or(reported.as_ref().and_then(|p| p.price_usd_monthly)),
            renews_at_ms: a
                .renew_at
                .or(reported.as_ref().and_then(|p| p.renews_at_ms)),
            auto_renew: reported.as_ref().and_then(|p| p.auto_renew),
        })
    } else {
        None
    };
    let (windows, balances) = match &record {
        Some(r) => (
            r.windows
                .iter()
                .map(|w| {
                    let remaining = match (w.unit.as_str(), w.used, w.limit) {
                        ("percent", Some(u), _) => Some((100.0 - u).clamp(0.0, 100.0)),
                        (_, Some(u), Some(l)) if l > 0.0 => {
                            Some(((l - u) / l * 100.0).clamp(0.0, 100.0))
                        }
                        _ => None,
                    };
                    UsageWindow {
                        scope: w.scope.clone(),
                        label: w.label.clone(),
                        unit: w.unit.clone(),
                        used: w.used,
                        limit: w.limit,
                        remaining_percent: remaining,
                        window_seconds: w.window_seconds,
                        resets_at_ms: w.resets_at,
                    }
                })
                .collect(),
            r.balances
                .iter()
                .map(|b| UsageBalance {
                    currency: b.currency.clone(),
                    amount: b.amount,
                    included: b.included,
                    topup: b.topup,
                    granted: b.granted,
                })
                .collect(),
        ),
        None => (Vec::new(), Vec::new()),
    };
    AccountUsage {
        provider: p.slug.clone(),
        alias: a.alias.clone(),
        account_type: a.account_type.clone(),
        supported,
        fetched_at_ms: record.as_ref().map(|r| r.fetched_at),
        source: record.as_ref().and_then(|r| r.source.clone()),
        confidence: record.as_ref().and_then(|r| r.confidence.clone()),
        error: record.as_ref().and_then(|r| r.error.clone()),
        account_label: record.as_ref().and_then(|r| r.account_label.clone()),
        plan,
        windows,
        balances,
        spend_today,
        spend_month,
    }
}

/// Background usage refresh (restartable). Each account is refreshed at its module's
/// interval; failures back off exponentially up to an hour.
pub(crate) async fn poll(
    control: Arc<ControlPlane>,
    shutdown: CancellationToken,
) -> Result<(), String> {
    let mut schedule: HashMap<String, (Instant, u32)> = HashMap::new();
    loop {
        let targets = control.usage_targets(None).await.map_err(|e| e.message)?;
        let now = Instant::now();
        for t in targets {
            let Some(module) = control.registry.get(&t.provider.kind) else {
                continue;
            };
            let due = schedule.get(&t.account.id).is_none_or(|(at, _)| *at <= now);
            if !due {
                continue;
            }
            let interval = module.usage_interval();
            let failures = schedule.get(&t.account.id).map_or(0, |(_, f)| *f);
            let next = match control.refresh_account_usage(&t).await {
                Ok(()) => (Instant::now() + interval, 0),
                Err(e) => {
                    tracing::info!(
                        account = %format!("{}/{}", t.provider.slug, t.account.alias),
                        error = %e,
                        "usage refresh failed"
                    );
                    let backoff = interval
                        .saturating_mul(1 << failures.min(6))
                        .min(MAX_BACKOFF);
                    (Instant::now() + backoff, failures + 1)
                }
            };
            schedule.insert(t.account.id.clone(), next);
        }
        tokio::select! {
            () = tokio::time::sleep(POLL_TICK) => {}
            () = shutdown.cancelled() => return Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use loadout_protocol::Protocol;
    use loadout_provider_sdk::usage::{Confidence, QuotaWindow, UsageSource};
    use loadout_provider_sdk::{Capabilities, Endpoint, HttpClient, ProviderConfig, TrustLevel};
    use loadout_providers::{OpenAiCompatible, Registry};
    use loadout_router::{Prices, RouteTable, RouteTarget, TargetId};
    use loadout_storage::Database;

    use super::*;

    fn control_with_target(account: &str) -> (ControlPlane, Arc<RouteTarget>) {
        let control = ControlPlane::new(
            Arc::new(Database::open_in_memory().expect("db")),
            Arc::new(loadout_secrets::InMemorySecretStore::new()),
            Registry::builtin(),
            Arc::default(),
            HttpClient::new(&Default::default()).expect("http"),
            std::path::PathBuf::from("."),
        );
        let id: TargetId = Arc::from("m1:chat");
        let target = Arc::new(RouteTarget {
            health: control.health.get_or_create(&id),
            id,
            provider_slug: Arc::from("p"),
            account_id: Arc::from(account),
            account_alias: Arc::from("main"),
            account_position: 0,
            upstream_model: Arc::from("m"),
            protocol: Protocol::Chat,
            trust: TrustLevel::OfficialApi,
            context_window: None,
            max_output_tokens: None,
            capabilities: Capabilities::DEFAULT_MODEL,
            prices: Prices::default(),
            endpoint: Endpoint::new("https://x.test".into()),
            module: Arc::new(OpenAiCompatible),
            config: Arc::new(ProviderConfig {
                slug: "p".into(),
                kind: "openai-compatible".into(),
                base_url: None,
                settings: serde_json::Value::Null,
            }),
        });
        control
            .routes
            .store(RouteTable::new(1, vec![target.clone()]));
        (control, target)
    }

    fn report(used_percent: f64, resets_at: Option<i64>) -> UsageReport {
        let mut r = UsageReport::new(UsageSource::OfficialApi, Confidence::Exact);
        r.windows.push(QuotaWindow {
            scope: "5h".into(),
            label: "5h".into(),
            unit: QuotaUnit::Percent,
            used: Some(used_percent),
            limit: None,
            window_seconds: Some(18_000),
            resets_at_ms: resets_at,
        });
        r
    }

    #[test]
    fn exhausted_quota_blocks_routes_until_reset_and_recovers() {
        let (control, target) = control_with_target("acc-1");
        control.apply_quota("acc-1", &report(100.0, Some(5_000)), 0.0, 1_000);
        assert_eq!(target.health.state(), HealthState::QuotaExhausted);
        assert!(!target.health.is_available(4_999));
        assert!(target.health.is_available(5_000));

        control.apply_quota("acc-1", &report(20.0, Some(5_000)), 0.0, 6_000);
        assert_eq!(
            target.health.state(),
            HealthState::Unknown,
            "unblocked when quota is back"
        );

        // Other accounts are untouched.
        control.apply_quota("acc-2", &report(100.0, None), 0.0, 1_000);
        assert_eq!(target.health.state(), HealthState::Unknown);
    }

    #[test]
    fn reserve_threshold_blocks_early() {
        let (control, target) = control_with_target("acc-1");
        control.apply_quota("acc-1", &report(92.0, Some(9_000)), 10.0, 1_000);
        assert_eq!(target.health.state(), HealthState::QuotaExhausted);
        assert!(!target.health.is_available(8_999));

        let (control, target) = control_with_target("acc-1");
        control.apply_quota("acc-1", &report(85.0, Some(9_000)), 10.0, 1_000);
        assert_eq!(target.health.state(), HealthState::Unknown);
    }

    #[test]
    fn period_starts() {
        // 2026-09-29T15:30:00Z
        let now = 1_790_695_800_000;
        let (day, month) = local_period_starts(now);
        assert_eq!(day, 1_790_640_000_000); // 2026-09-29T00:00:00Z
        assert_eq!(month, 1_788_220_800_000); // 2026-09-01T00:00:00Z
    }
}
