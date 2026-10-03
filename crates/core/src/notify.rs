// SPDX-License-Identifier: Apache-2.0

//! Desktop notifications (ExecPlan 012, demo). A background loop evaluates conditions the
//! user should hear about from in-memory and database state and emits `notify` IPC events;
//! the tray app turns them into Windows toasts. Nothing here runs on the request path.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use loadout_ipc::Event;
use loadout_ipc::api::{AccountInfo, AccountUsage, NOTIFY_EVENT, Notification, TargetStatus};
use loadout_storage::now_ms;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::control::{ControlPlane, blocking, storage};
use crate::settings;

const TICK: Duration = Duration::from_secs(60);
/// Each `(kind, key)` is emitted at most once per period.
pub(crate) const DEDUP_MS: i64 = 6 * 3_600_000;
/// Logins expiring within this window are reported. The refresher renews logins 24 h ahead,
/// so the first hour is left to it to avoid announcing a login that is about to be renewed.
pub(crate) const LOGIN_WARN_MS: i64 = 23 * 3_600_000;

pub(crate) const QUOTA_LOW: &str = "quota_low";
pub(crate) const BALANCE_LOW: &str = "balance_low";
pub(crate) const AUTH_FAILED: &str = "auth_failed";
pub(crate) const LOGIN_EXPIRING: &str = "login_expiring";
pub(crate) const CLIENT_COST_LIMIT: &str = "client_cost_limit";

/// Effective notification settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NotifySettings {
    pub enabled: bool,
    /// 0 disables.
    pub quota_percent: f64,
    /// 0 disables.
    pub balance_usd: f64,
}

impl Default for NotifySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            quota_percent: 10.0,
            balance_usd: 1.0,
        }
    }
}

impl NotifySettings {
    pub(crate) fn load(
        db: &loadout_storage::Database,
    ) -> Result<Self, loadout_storage::StorageError> {
        let d = Self::default();
        Ok(Self {
            enabled: settings::parse_bool(&settings::value(db, settings::NOTIFY_ENABLED)?)
                .unwrap_or(d.enabled),
            quota_percent: settings::value(db, settings::NOTIFY_QUOTA_PERCENT)?
                .parse()
                .unwrap_or(d.quota_percent),
            balance_usd: settings::value(db, settings::NOTIFY_BALANCE_USD)?
                .parse()
                .unwrap_or(d.balance_usd),
        })
    }
}

/// A client's daily cost limit and today's spend.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ClientSpend {
    pub name: String,
    pub enabled: bool,
    pub daily_cost_limit_usd: Option<f64>,
    pub cost_today_usd: f64,
}

/// Everything the evaluation looks at.
#[derive(Clone, Debug, Default)]
pub(crate) struct Inputs {
    pub usage: Vec<AccountUsage>,
    pub routes: Vec<TargetStatus>,
    pub accounts: Vec<AccountInfo>,
    pub clients: Vec<ClientSpend>,
}

/// Conditions that currently hold, without de-duplication over time (pure).
pub(crate) fn evaluate(inputs: &Inputs, s: &NotifySettings, now: i64) -> Vec<Notification> {
    let mut out = Vec::new();
    if !s.enabled {
        return out;
    }
    let mut seen = HashSet::new();
    let mut push = |n: Notification| {
        if seen.insert((n.kind.clone(), n.key.clone())) {
            out.push(n);
        }
    };

    for u in &inputs.usage {
        let subject = format!("{}/{}", u.provider, u.alias);
        if s.quota_percent > 0.0 {
            for w in &u.windows {
                let Some(left) = w.remaining_percent else {
                    continue;
                };
                // A window that has already reset no longer says anything about now.
                if left >= s.quota_percent || w.resets_at_ms.is_some_and(|r| r <= now) {
                    continue;
                }
                push(Notification {
                    kind: QUOTA_LOW.into(),
                    key: format!("{subject}/{}", w.label),
                    title: "Quota running low".into(),
                    body: format!("{subject}: {} has {left:.0}% left", w.label),
                    subject: subject.clone(),
                    detail: Some(w.label.clone()),
                    value: Some(left),
                });
            }
        }
        if s.balance_usd > 0.0 {
            for b in &u.balances {
                if !b.currency.eq_ignore_ascii_case("USD") || b.amount >= s.balance_usd {
                    continue;
                }
                push(Notification {
                    kind: BALANCE_LOW.into(),
                    key: subject.clone(),
                    title: "Balance running low".into(),
                    body: format!("{subject}: ${:.2} left", b.amount),
                    subject: subject.clone(),
                    detail: None,
                    value: Some(b.amount),
                });
            }
        }
    }

    for r in &inputs.routes {
        if r.state != "AUTH_FAILED" {
            continue;
        }
        // Routes are per model; the credential belongs to the account.
        let subject = r.name.rsplit_once('/').map_or(r.name.as_str(), |(a, _)| a);
        push(Notification {
            kind: AUTH_FAILED.into(),
            key: subject.to_owned(),
            title: "Credentials rejected".into(),
            body: format!(
                "{subject}: the upstream rejected the API key or sign-in (401/403). Update the \
                 key or sign in again."
            ),
            subject: subject.to_owned(),
            detail: None,
            value: None,
        });
    }

    for a in &inputs.accounts {
        let Some(expires) = a.login_expires_at_ms else {
            continue;
        };
        if !a.enabled || expires - now >= LOGIN_WARN_MS {
            continue;
        }
        let subject = format!("{}/{}", a.provider, a.alias);
        let hours = ((expires - now).max(0) as f64 / 3_600_000.0).floor();
        let body = if expires <= now {
            format!("{subject}: the sign-in has expired. Sign in again.")
        } else {
            format!("{subject}: the sign-in expires in {hours:.0} h and could not be renewed.")
        };
        push(Notification {
            kind: LOGIN_EXPIRING.into(),
            key: subject.clone(),
            title: "Sign-in expiring".into(),
            body,
            subject,
            detail: None,
            value: Some(hours),
        });
    }

    for c in &inputs.clients {
        let Some(limit) = c.daily_cost_limit_usd else {
            continue;
        };
        if !c.enabled || c.cost_today_usd < limit {
            continue;
        }
        push(Notification {
            kind: CLIENT_COST_LIMIT.into(),
            key: c.name.clone(),
            title: "Daily cost limit reached".into(),
            body: format!(
                "Client {} reached its daily limit of ${limit:.2}; its requests are rejected \
                 until midnight UTC.",
                c.name
            ),
            subject: c.name.clone(),
            detail: None,
            value: Some(limit),
        });
    }
    out
}

/// Remembers when each `(kind, key)` was last emitted.
#[derive(Debug, Default)]
pub(crate) struct Dedup {
    last: HashMap<(String, String), i64>,
}

impl Dedup {
    /// The notifications not emitted within [`DEDUP_MS`]; records them as emitted.
    pub(crate) fn filter(&mut self, items: Vec<Notification>, now: i64) -> Vec<Notification> {
        self.last.retain(|_, at| now - *at < DEDUP_MS);
        items
            .into_iter()
            .filter(|n| {
                let k = (n.kind.clone(), n.key.clone());
                if self.last.contains_key(&k) {
                    return false;
                }
                self.last.insert(k, now);
                true
            })
            .collect()
    }
}

async fn gather(control: &ControlPlane) -> Result<Inputs, String> {
    let usage = control
        .usage_list(None, false)
        .await
        .map_err(|e| e.message)?;
    let accounts = control.list_accounts(None).await.map_err(|e| e.message)?;
    let db = control.db.clone();
    let clients = blocking(move || db.clients().list().map_err(storage))
        .await
        .map_err(|e| e.message)?;
    let now = now_ms();
    let clients = clients
        .into_iter()
        .map(|c| {
            let policy = crate::clients::parse_policy(&c.policy_json);
            ClientSpend {
                cost_today_usd: control.policies.usage_today(c.id.as_str(), now).cost_usd,
                name: c.name,
                enabled: c.enabled,
                daily_cost_limit_usd: policy.daily_cost_limit_usd,
            }
        })
        .collect();
    Ok(Inputs {
        usage,
        routes: control.route_status(),
        accounts,
        clients,
    })
}

/// Background notification loop (restartable).
pub(crate) async fn run(
    control: Arc<ControlPlane>,
    events: broadcast::Sender<Event>,
    shutdown: CancellationToken,
) -> Result<(), String> {
    let mut dedup = Dedup::default();
    loop {
        tokio::select! {
            () = tokio::time::sleep(TICK) => {}
            () = shutdown.cancelled() => return Ok(()),
        }
        // Errors skip a tick instead of restarting the task, which would lose `dedup`.
        let db = control.db.clone();
        let settings = match blocking(move || NotifySettings::load(&db).map_err(storage)).await {
            Ok(s) if s.enabled => s,
            Ok(_) => continue,
            Err(e) => {
                tracing::warn!(error = %e.message, "notification settings unavailable");
                continue;
            }
        };
        let inputs = match gather(&control).await {
            Ok(i) => i,
            Err(e) => {
                tracing::warn!(error = %e, "notification check failed");
                continue;
            }
        };
        let now = now_ms();
        for n in dedup.filter(evaluate(&inputs, &settings, now), now) {
            tracing::info!(kind = %n.kind, key = %n.key, "notification");
            let data = serde_json::to_value(&n).map_err(|e| e.to_string())?;
            // No receivers is normal when no UI is connected.
            let _ = events.send(Event::new(NOTIFY_EVENT, data));
        }
    }
}

#[cfg(test)]
mod tests {
    use loadout_ipc::api::{SpendInfo, UsageBalance, UsageWindow};

    use super::*;

    const NOW: i64 = 1_800_000_000_000;

    fn usage(
        windows: Vec<(&str, Option<f64>, Option<i64>)>,
        balances: Vec<(&str, f64)>,
    ) -> AccountUsage {
        AccountUsage {
            provider: "openai".into(),
            alias: "main".into(),
            account_type: "api_key".into(),
            supported: true,
            fetched_at_ms: Some(NOW),
            source: None,
            confidence: None,
            error: None,
            account_label: None,
            plan: None,
            windows: windows
                .into_iter()
                .map(|(label, left, resets)| UsageWindow {
                    scope: "account".into(),
                    label: label.into(),
                    unit: "percent".into(),
                    used: None,
                    limit: None,
                    remaining_percent: left,
                    window_seconds: None,
                    resets_at_ms: resets,
                })
                .collect(),
            balances: balances
                .into_iter()
                .map(|(currency, amount)| UsageBalance {
                    currency: currency.into(),
                    amount,
                    included: None,
                    topup: None,
                    granted: None,
                })
                .collect(),
            spend_today: SpendInfo::default(),
            spend_month: SpendInfo::default(),
        }
    }

    fn route(name: &str, state: &str) -> TargetStatus {
        TargetStatus {
            name: name.into(),
            protocol: "openai_chat".into(),
            trust: "official".into(),
            state: state.into(),
            unavailable_until_ms: None,
            consecutive_failures: 0,
            in_flight: 0,
            successes: 0,
            failures: 0,
            last_error: None,
            tier: "UNMEASURED".into(),
            tier_pinned: false,
            ttft_p50_ms: None,
            ttft_p90_ms: None,
            tps: None,
            samples: 0,
            last_sample_ms: None,
        }
    }

    fn account(alias: &str, enabled: bool, expires: Option<i64>) -> AccountInfo {
        AccountInfo {
            provider: "codex".into(),
            alias: alias.into(),
            display_name: None,
            account_type: "subscription".into(),
            enabled,
            position: 0,
            has_key: true,
            models: 1,
            enabled_models: 1,
            transport_opt_in: true,
            login_expires_at_ms: expires,
            quota_reserve: None,
        }
    }

    fn client(name: &str, limit: Option<f64>, cost: f64) -> ClientSpend {
        ClientSpend {
            name: name.into(),
            enabled: true,
            daily_cost_limit_usd: limit,
            cost_today_usd: cost,
        }
    }

    fn kinds_and_keys(v: &[Notification]) -> Vec<(&str, &str)> {
        v.iter()
            .map(|n| (n.kind.as_str(), n.key.as_str()))
            .collect()
    }

    #[test]
    fn quota_and_balance_thresholds() {
        let inputs = Inputs {
            usage: vec![usage(
                vec![
                    ("5h", Some(7.0), Some(NOW + 1000)),
                    ("weekly", Some(40.0), None),
                    ("stale", Some(1.0), Some(NOW - 1)),
                    ("unknown", None, None),
                ],
                vec![("USD", 0.5), ("CNY", 0.1)],
            )],
            ..Inputs::default()
        };
        let out = evaluate(&inputs, &NotifySettings::default(), NOW);
        assert_eq!(
            kinds_and_keys(&out),
            vec![(QUOTA_LOW, "openai/main/5h"), (BALANCE_LOW, "openai/main")]
        );
        assert_eq!(out[0].value, Some(7.0));
        assert!(out[0].body.contains("7%"), "{}", out[0].body);

        let off = NotifySettings {
            quota_percent: 0.0,
            balance_usd: 0.0,
            ..NotifySettings::default()
        };
        assert!(evaluate(&inputs, &off, NOW).is_empty());
        let disabled = NotifySettings {
            enabled: false,
            ..NotifySettings::default()
        };
        assert!(evaluate(&inputs, &disabled, NOW).is_empty());
    }

    #[test]
    fn auth_failures_are_reported_once_per_account() {
        let inputs = Inputs {
            routes: vec![
                route("openai/main/gpt-4o", "AUTH_FAILED"),
                route("openai/main/gpt-4o-mini", "AUTH_FAILED"),
                route("openai/backup/gpt-4o", "COOLDOWN"),
                route("openai/other/gpt-4o", "HEALTHY"),
            ],
            ..Inputs::default()
        };
        let out = evaluate(&inputs, &NotifySettings::default(), NOW);
        assert_eq!(kinds_and_keys(&out), vec![(AUTH_FAILED, "openai/main")]);
    }

    #[test]
    fn expiring_logins() {
        let hour = 3_600_000;
        let inputs = Inputs {
            accounts: vec![
                account("soon", true, Some(NOW + 5 * hour + 10)),
                account("expired", true, Some(NOW - hour)),
                account("renewing", true, Some(NOW + 23 * hour + 1)),
                account("later", true, Some(NOW + 72 * hour)),
                account("off", false, Some(NOW + hour)),
                account("apikey", true, None),
            ],
            ..Inputs::default()
        };
        let out = evaluate(&inputs, &NotifySettings::default(), NOW);
        assert_eq!(
            kinds_and_keys(&out),
            vec![
                (LOGIN_EXPIRING, "codex/soon"),
                (LOGIN_EXPIRING, "codex/expired")
            ]
        );
        assert_eq!(out[0].value, Some(5.0));
        assert!(out[1].body.contains("expired"));
    }

    #[test]
    fn client_cost_limits() {
        let mut disabled = client("old", Some(1.0), 5.0);
        disabled.enabled = false;
        let inputs = Inputs {
            clients: vec![
                client("trading", Some(2.0), 2.0),
                client("under", Some(2.0), 1.99),
                client("unlimited", None, 100.0),
                disabled,
            ],
            ..Inputs::default()
        };
        let out = evaluate(&inputs, &NotifySettings::default(), NOW);
        assert_eq!(kinds_and_keys(&out), vec![(CLIENT_COST_LIMIT, "trading")]);
    }

    #[test]
    fn dedup_suppresses_repeats_for_six_hours() {
        let inputs = Inputs {
            routes: vec![route("openai/main/gpt-4o", "AUTH_FAILED")],
            clients: vec![client("trading", Some(1.0), 1.0)],
            ..Inputs::default()
        };
        let s = NotifySettings::default();
        let mut dedup = Dedup::default();
        assert_eq!(dedup.filter(evaluate(&inputs, &s, NOW), NOW).len(), 2);
        assert!(
            dedup
                .filter(evaluate(&inputs, &s, NOW), NOW + 60_000)
                .is_empty()
        );
        assert!(
            dedup
                .filter(evaluate(&inputs, &s, NOW), NOW + DEDUP_MS - 1)
                .is_empty()
        );
        assert_eq!(
            dedup
                .filter(evaluate(&inputs, &s, NOW), NOW + DEDUP_MS)
                .len(),
            2
        );
    }

    #[test]
    fn settings_load_defaults_and_overrides() {
        let db = loadout_storage::Database::open_in_memory().expect("db");
        assert_eq!(
            NotifySettings::load(&db).expect("load"),
            NotifySettings::default()
        );
        db.settings()
            .set(settings::NOTIFY_QUOTA_PERCENT, "25")
            .expect("set");
        db.settings()
            .set(settings::NOTIFY_ENABLED, "off")
            .expect("set");
        let s = NotifySettings::load(&db).expect("load");
        assert_eq!(s.quota_percent, 25.0);
        assert!(!s.enabled);
        assert!(
            settings::find(settings::NOTIFY_QUOTA_PERCENT)
                .expect("def")
                .validate("101")
                .is_err()
        );
    }
}
