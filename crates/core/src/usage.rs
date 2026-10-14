// SPDX-License-Identifier: Apache-2.0

//! Usage tracking (ExecPlan 003 §4.1–4.2): background refresh through each module's Usage
//! driver, persistence of the latest snapshot, and quota-driven route availability
//! (ADR-003 §9).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use loadout_ipc::ErrorBody;
use loadout_ipc::api::{
    AccountInfo, AccountUsage, SpendInfo, UsageBalance, UsagePlan, UsageWindow,
};
use loadout_provider_sdk::usage::{
    Balance, Confidence, QuotaUnit, QuotaWindow, UsageReport, UsageSource,
};
use loadout_provider_sdk::{DriverContext, ProviderError};
use loadout_router::{HealthState, now_ms};
use loadout_secrets::Secret;
use loadout_storage::{Account, BalanceRow, Provider, Spend, UsageRecord, WindowRow};
use serde_json::Value;
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

fn unit_from_name(s: &str) -> Option<QuotaUnit> {
    Some(match s {
        "percent" => QuotaUnit::Percent,
        "requests" => QuotaUnit::Requests,
        "tokens" => QuotaUnit::Tokens,
        "usd" => QuotaUnit::Usd,
        "credits" => QuotaUnit::Credits,
        _ => return None,
    })
}

/// Quota reserve thresholds (percent remaining below which an account leaves the candidate
/// chain, ADR-003 §9). Per window scope: the account's value for that scope (or for its base
/// scope, `weekly` for `weekly:opus`), else the account's `default`, else the global
/// `routing.quota_reserve_percent`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ReservePolicy {
    global: f64,
    default: Option<f64>,
    windows: HashMap<String, f64>,
}

impl ReservePolicy {
    /// `account_json` is the stored per-account override; invalid JSON is ignored.
    pub(crate) fn new(global: f64, account_json: Option<&str>) -> Self {
        let mut policy = Self {
            global,
            ..Self::default()
        };
        let parsed: Option<serde_json::Map<String, Value>> =
            account_json.and_then(|j| serde_json::from_str(j).ok());
        for (k, v) in parsed.into_iter().flatten() {
            let Some(n) = v.as_f64().filter(|n| n.is_finite()) else {
                continue;
            };
            if k == "default" {
                policy.default = Some(n);
            } else {
                policy.windows.insert(k, n);
            }
        }
        policy
    }

    pub(crate) fn for_scope(&self, scope: &str) -> f64 {
        let base = scope.split(':').next().unwrap_or(scope);
        self.windows
            .get(scope)
            .or_else(|| self.windows.get(base))
            .copied()
            .or(self.default)
            .unwrap_or(self.global)
    }
}

/// Validates a reserve object from IPC / backups and returns its canonical JSON; `None`
/// for an empty object (= use the global setting).
pub(crate) fn normalize_reserve(v: &Value) -> Result<Option<String>, String> {
    let obj = v
        .as_object()
        .ok_or("reserve must be an object such as {\"default\":10,\"weekly\":20}")?;
    let mut out = serde_json::Map::new();
    for (k, n) in obj {
        let key = k.trim();
        if key.is_empty() {
            return Err("reserve window names must not be empty".into());
        }
        let n = n
            .as_f64()
            .filter(|n| n.is_finite() && (0.0..100.0).contains(n))
            .ok_or_else(|| format!("reserve for {key:?} must be a percentage from 0 to 99"))?;
        out.insert(key.to_owned(), Value::from(n));
    }
    Ok((!out.is_empty()).then(|| Value::Object(out).to_string()))
}

/// The last stored usage as a report, for re-applying quota state after a setting change.
fn report_from_record(r: &UsageRecord) -> UsageReport {
    let mut report = UsageReport::new(UsageSource::Estimated, Confidence::Unknown);
    report.windows = r
        .windows
        .iter()
        .filter_map(|w| {
            Some(QuotaWindow {
                scope: w.scope.clone(),
                label: w.label.clone(),
                unit: unit_from_name(&w.unit)?,
                used: w.used,
                limit: w.limit,
                window_seconds: w.window_seconds.and_then(|s| u64::try_from(s).ok()),
                resets_at_ms: w.resets_at,
            })
        })
        .collect();
    report.balances = r
        .balances
        .iter()
        .map(|b| Balance {
            currency: b.currency.clone(),
            amount: b.amount,
            included: b.included,
            topup: b.topup,
            granted: b.granted,
        })
        .collect();
    report
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
                if !module.descriptor().drivers.usage || provider_config(&p).usage_source() == "off"
                {
                    continue;
                }
                for a in db.accounts().list(Some(&p.id)).map_err(storage)? {
                    if !a.enabled {
                        continue;
                    }
                    let key =
                        crate::control::key_ref(db.accounts().credentials(&a.id).map_err(storage)?)
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
                let reserve = ReservePolicy::new(
                    self.quota_reserve_percent().await,
                    t.account.quota_reserve_json.as_deref(),
                );
                self.apply_quota(&account_id, &report, &reserve, now);
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

    /// Sets (or clears, with `None` / an empty object) an account's quota reserve and
    /// re-applies its last stored usage so the change takes effect without waiting for the
    /// next poll. Runs on the control plane only (ADR-003 §9: routing reads the result as
    /// health state).
    pub async fn set_account_reserve(
        &self,
        provider: String,
        alias: String,
        reserve: Option<serde_json::Value>,
    ) -> Result<AccountInfo, ErrorBody> {
        let json = match &reserve {
            None => None,
            Some(v) => normalize_reserve(v).map_err(ErrorBody::invalid_params)?,
        };
        let db = self.db.clone();
        let (p2, a2, j2) = (provider.clone(), alias.clone(), json.clone());
        let (account_id, record) = blocking(move || {
            let p = Self::load_provider(&db, &p2)?;
            let a = Self::load_account(&db, &p, &a2)?;
            db.accounts()
                .set_quota_reserve(&a.id, j2.as_deref())
                .map_err(storage)?;
            let record = db.usage(&a.id).map_err(storage)?;
            Ok((a.id, record))
        })
        .await?;
        if let Some(record) = record {
            let policy = ReservePolicy::new(self.quota_reserve_percent().await, json.as_deref());
            self.apply_quota(&account_id, &report_from_record(&record), &policy, now_ms());
        }
        self.account(&provider, &alias).await
    }

    /// Blocks the account's routes while its quota is exhausted (or below the reserve), and
    /// unblocks routes a previous report blocked once quota is available again.
    pub(crate) fn apply_quota(
        &self,
        account_id: &str,
        report: &UsageReport,
        reserve: &ReservePolicy,
        now: i64,
    ) {
        let below_reserve = report.windows.iter().filter(|w| {
            let percent = reserve.for_scope(&w.scope);
            percent > 0.0 && w.remaining_fraction().is_some_and(|r| r * 100.0 < percent)
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

/// Passive usage from the gateway (ExecPlan 013): `(account id, report)`. Bounded; the
/// gateway drops reports when it is full, never waits.
pub(crate) type PassiveUsageSender = tokio::sync::mpsc::Sender<(String, UsageReport)>;
pub(crate) const PASSIVE_QUEUE: usize = 256;
/// At most one database write per account in this interval; quota state is applied on
/// every report.
const PASSIVE_SAVE_INTERVAL_MS: i64 = 30_000;

impl ControlPlane {
    /// Stores a passive report (keeping plan and label from the last full refresh) and
    /// returns the account's reserve policy.
    async fn save_passive_usage(
        &self,
        account_id: &str,
        report: &UsageReport,
        now: i64,
    ) -> Result<ReservePolicy, ErrorBody> {
        let global = self.quota_reserve_percent().await;
        let db = self.db.clone();
        let mut record = to_record(account_id, now, report);
        let id = account_id.to_owned();
        blocking(move || {
            let account = db.accounts().get_by_id(&id).map_err(storage)?;
            let Some(account) = account else {
                return Ok(ReservePolicy::new(global, None));
            };
            if let Some(old) = db.usage(&id).map_err(storage)? {
                record.plan_json = record.plan_json.or(old.plan_json);
                record.account_label = record.account_label.or(old.account_label);
            }
            db.save_usage(&record).map_err(storage)?;
            Ok(ReservePolicy::new(
                global,
                account.quota_reserve_json.as_deref(),
            ))
        })
        .await
    }
}

/// Consumes passive usage reports until shutdown.
pub(crate) async fn passive(
    control: Arc<ControlPlane>,
    mut rx: tokio::sync::mpsc::Receiver<(String, UsageReport)>,
    shutdown: CancellationToken,
) {
    let mut policies: HashMap<String, (i64, ReservePolicy)> = HashMap::new();
    loop {
        let (account_id, report) = tokio::select! {
            m = rx.recv() => match m {
                Some(m) => m,
                None => return,
            },
            () = shutdown.cancelled() => return,
        };
        let now = now_ms();
        let due = policies
            .get(&account_id)
            .is_none_or(|(at, _)| now - at >= PASSIVE_SAVE_INTERVAL_MS);
        if due {
            match control.save_passive_usage(&account_id, &report, now).await {
                Ok(policy) => {
                    policies.insert(account_id.clone(), (now, policy));
                }
                Err(e) => tracing::debug!(error = %e.message, "storing passive usage failed"),
            }
        }
        if let Some((_, policy)) = policies.get(&account_id) {
            control.apply_quota(&account_id, &report, policy, now);
        }
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
        let target = install_target(&control, account);
        (control, target)
    }

    /// Replaces the route table with one target for `account`.
    fn install_target(control: &ControlPlane, account: &str) -> Arc<RouteTarget> {
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
            opted_in: false,
            context_window: None,
            max_output_tokens: None,
            capabilities: Capabilities::DEFAULT_MODEL,
            prices: Prices::default(),
            endpoint: Endpoint::new("https://x.test".into()),
            web: None,
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
        target
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

    fn global(percent: f64) -> ReservePolicy {
        ReservePolicy::new(percent, None)
    }

    /// A report with one percent window per `(scope, used)`.
    fn windows(list: &[(&str, f64)], resets_at: i64) -> UsageReport {
        let mut r = UsageReport::new(UsageSource::OfficialApi, Confidence::Exact);
        for (scope, used) in list {
            r.windows.push(QuotaWindow {
                scope: (*scope).into(),
                label: (*scope).into(),
                unit: QuotaUnit::Percent,
                used: Some(*used),
                limit: None,
                window_seconds: None,
                resets_at_ms: Some(resets_at),
            });
        }
        r
    }

    #[test]
    fn exhausted_quota_blocks_routes_until_reset_and_recovers() {
        let (control, target) = control_with_target("acc-1");
        control.apply_quota("acc-1", &report(100.0, Some(5_000)), &global(0.0), 1_000);
        assert_eq!(target.health.state(), HealthState::QuotaExhausted);
        assert!(!target.health.is_available(4_999));
        assert!(target.health.is_available(5_000));

        control.apply_quota("acc-1", &report(20.0, Some(5_000)), &global(0.0), 6_000);
        assert_eq!(
            target.health.state(),
            HealthState::Unknown,
            "unblocked when quota is back"
        );

        // Other accounts are untouched.
        control.apply_quota("acc-2", &report(100.0, None), &global(0.0), 1_000);
        assert_eq!(target.health.state(), HealthState::Unknown);
    }

    #[test]
    fn reserve_threshold_blocks_early() {
        let (control, target) = control_with_target("acc-1");
        control.apply_quota("acc-1", &report(92.0, Some(9_000)), &global(10.0), 1_000);
        assert_eq!(target.health.state(), HealthState::QuotaExhausted);
        assert!(!target.health.is_available(8_999));

        let (control, target) = control_with_target("acc-1");
        control.apply_quota("acc-1", &report(85.0, Some(9_000)), &global(10.0), 1_000);
        assert_eq!(target.health.state(), HealthState::Unknown);
    }

    #[test]
    fn reserve_policy_resolves_window_then_default_then_global() {
        let p = ReservePolicy::new(5.0, Some(r#"{"default":10,"5h":12,"weekly":20}"#));
        assert_eq!(p.for_scope("5h"), 12.0);
        assert_eq!(p.for_scope("weekly"), 20.0);
        assert_eq!(p.for_scope("weekly:opus"), 20.0, "base scope matches");
        assert_eq!(p.for_scope("monthly"), 10.0, "account default");
        let p = ReservePolicy::new(5.0, Some(r#"{"weekly":20}"#));
        assert_eq!(p.for_scope("5h"), 5.0, "global fallback");
        assert_eq!(
            ReservePolicy::new(5.0, Some("not json")).for_scope("5h"),
            5.0
        );
        assert_eq!(global(7.0).for_scope("anything"), 7.0);
        // An explicit 0 turns the reserve off for that window even with a global value.
        assert_eq!(
            ReservePolicy::new(5.0, Some(r#"{"5h":0}"#)).for_scope("5h"),
            0.0
        );
    }

    #[test]
    fn per_window_reserve_blocks_only_the_window_below_its_threshold() {
        // 5h has 15% left, weekly 18% left.
        let r = windows(&[("5h", 85.0), ("weekly", 82.0)], 9_000);

        // Weekly reserve 20% → blocked by the weekly window.
        let (control, target) = control_with_target("acc-1");
        let p = ReservePolicy::new(0.0, Some(r#"{"5h":10,"weekly":20}"#));
        control.apply_quota("acc-1", &r, &p, 1_000);
        assert_eq!(target.health.state(), HealthState::QuotaExhausted);

        // Weekly reserve 15% → both windows above their reserve → available.
        let (control, target) = control_with_target("acc-1");
        let p = ReservePolicy::new(0.0, Some(r#"{"5h":10,"weekly":15}"#));
        control.apply_quota("acc-1", &r, &p, 1_000);
        assert_eq!(target.health.state(), HealthState::Unknown);

        // Account default overrides a stricter global value.
        let (control, target) = control_with_target("acc-1");
        control.apply_quota("acc-1", &r, &global(30.0), 1_000);
        assert_eq!(target.health.state(), HealthState::QuotaExhausted);
        let p = ReservePolicy::new(30.0, Some(r#"{"default":10}"#));
        control.apply_quota("acc-1", &r, &p, 2_000);
        assert_eq!(target.health.state(), HealthState::Unknown, "unblocked");
    }

    #[test]
    fn normalizes_reserve_input() {
        assert_eq!(
            normalize_reserve(&serde_json::json!({"weekly": 20, "default": 10.5})).expect("ok"),
            Some(r#"{"weekly":20.0,"default":10.5}"#.to_owned())
        );
        assert_eq!(normalize_reserve(&serde_json::json!({})).expect("ok"), None);
        assert!(normalize_reserve(&serde_json::json!({"5h": 100})).is_err());
        assert!(normalize_reserve(&serde_json::json!({"5h": -1})).is_err());
        assert!(normalize_reserve(&serde_json::json!({"5h": "x"})).is_err());
        assert!(normalize_reserve(&serde_json::json!({" ": 1})).is_err());
        assert!(normalize_reserve(&serde_json::json!([1])).is_err());
    }

    #[tokio::test]
    async fn passive_reports_are_stored_and_applied_off_the_request_path() {
        use loadout_storage::{NewAccount, NewProvider};
        let (control, _) = control_with_target("unused");
        let db = control.db.clone();
        let p = db
            .providers()
            .create(&NewProvider {
                slug: "p",
                kind: "openai-compatible",
                display_name: "P",
                base_url: Some("https://x.test"),
                config_json: "{}",
            })
            .expect("provider");
        let a = db
            .accounts()
            .create(&NewAccount {
                provider_id: &p.id,
                alias: "main",
                display_name: None,
                account_type: "subscription",
            })
            .expect("account");
        db.accounts()
            .set_quota_reserve(&a.id, Some(r#"{"5h":10}"#))
            .expect("reserve");
        let target = install_target(&control, &a.id);
        let control = Arc::new(control);
        let (tx, rx) = tokio::sync::mpsc::channel(PASSIVE_QUEUE);
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(passive(control.clone(), rx, shutdown.clone()));

        let far = now_ms() + 3_600_000;
        tx.send((a.id.clone(), windows(&[("5h", 95.0)], far)))
            .await
            .expect("send");
        let wait_for = |state: HealthState| {
            let target = target.clone();
            async move {
                for _ in 0..200 {
                    if target.health.state() == state {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                panic!("expected {state:?}, got {:?}", target.health.state());
            }
        };
        wait_for(HealthState::QuotaExhausted).await;
        let stored = db.usage(&a.id).expect("usage").expect("record");
        assert_eq!(stored.windows[0].used, Some(95.0));

        // A newer report within the save interval still updates routing (not the database).
        tx.send((a.id.clone(), windows(&[("5h", 20.0)], far)))
            .await
            .expect("send");
        wait_for(HealthState::Unknown).await;
        assert_eq!(
            db.usage(&a.id).expect("usage").expect("record").windows[0].used,
            Some(95.0)
        );

        shutdown.cancel();
        task.await.expect("task ends");
    }

    #[tokio::test]
    async fn setting_a_reserve_reapplies_stored_usage() {
        use loadout_storage::{NewAccount, NewProvider};
        let (control, _) = control_with_target("unused");
        let db = control.db.clone();
        let p = db
            .providers()
            .create(&NewProvider {
                slug: "p",
                kind: "openai-compatible",
                display_name: "P",
                base_url: Some("https://x.test"),
                config_json: "{}",
            })
            .expect("provider");
        let a = db
            .accounts()
            .create(&NewAccount {
                provider_id: &p.id,
                alias: "main",
                display_name: None,
                account_type: "api_key",
            })
            .expect("account");
        let far = now_ms() + 3_600_000;
        db.save_usage(&to_record(
            &a.id,
            now_ms(),
            &windows(&[("5h", 50.0), ("weekly", 85.0)], far),
        ))
        .expect("usage");
        let target = install_target(&control, &a.id);

        let info = control
            .set_account_reserve(
                "p".into(),
                "main".into(),
                Some(serde_json::json!({"weekly": 20})),
            )
            .await
            .expect("set");
        assert_eq!(
            info.quota_reserve,
            Some(serde_json::json!({"weekly": 20.0}))
        );
        assert_eq!(target.health.state(), HealthState::QuotaExhausted);

        let info = control
            .set_account_reserve("p".into(), "main".into(), None)
            .await
            .expect("clear");
        assert_eq!(info.quota_reserve, None);
        assert_eq!(target.health.state(), HealthState::Unknown);

        assert!(
            control
                .set_account_reserve(
                    "p".into(),
                    "main".into(),
                    Some(serde_json::json!({"weekly": 200}))
                )
                .await
                .is_err()
        );
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
