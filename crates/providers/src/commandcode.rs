// SPDX-License-Identifier: Apache-2.0

//! Command Code Provider API (`https://api.commandcode.ai/provider/v1`, docs:
//! commandcode.ai/docs/provider). Every model lists the routes serving it in
//! `supported_endpoints`.
//!
//! Usage: the `cmd` CLI's undocumented `GET /alpha/billing/credits` (5-hour / weekly caps
//! and the monthly credit balance) and `GET /alpha/billing/subscriptions` (plan and period
//! end), both on the API origin with the same bearer key. They are not part of the published
//! Provider API and may change. The cookie-only `/internal/billing/*` routes are not used
//! (ADR-009).

use std::time::Duration;

use loadout_protocol::Protocol;
use loadout_provider_sdk::time::{number_field, timestamp_ms};
use loadout_provider_sdk::usage::{
    Balance, Confidence, Plan, QuotaUnit, QuotaWindow, UsageReport, UsageSource,
};
use loadout_provider_sdk::{
    AuthMethod, BoxFuture, Capabilities, CatalogModel, DriverContext, Endpoint, ProviderConfig,
    ProviderDescriptor, ProviderError, ProviderModule, TransportSpec, TrustLevel,
};
use loadout_secrets::Secret;
use serde_json::Value;

use crate::common::{self, FIVE_HOURS, MONTH, WEEK, object};

const CREDITS_PATH: &str = "/alpha/billing/credits";
const SUBSCRIPTIONS_PATH: &str = "/alpha/billing/subscriptions";
/// The subscription lookup is optional and known to be slow; the report is built without it.
const SUBSCRIPTION_TIMEOUT: Duration = Duration::from_secs(8);

/// Plan id → display name and monthly credit allowance in USD, used when the credits
/// response omits `monthlyCreditsGranted` (plan catalog as of 2026-10-16).
const PLANS: &[(&str, &str, f64)] = &[
    ("individual-go", "Go", 10.0),
    ("individual-go-v1", "Go", 10.0),
    ("individual-pro", "Pro", 30.0),
    ("individual-pro-v1", "Pro", 80.0),
    ("individual-goat", "GOAT", 70.0),
    ("individual-max", "Max", 150.0),
    ("individual-ultra", "Ultra", 300.0),
];

static DESCRIPTOR: ProviderDescriptor = ProviderDescriptor {
    kind: "commandcode",
    display_name: "Command Code",
    fixed_slug: Some("commandcode"),
    default_base_url: Some("https://api.commandcode.ai/provider/v1"),
    auth: AuthMethod::ApiKey,
    transports: &[
        TransportSpec {
            protocol: Protocol::Chat,
            trust: TrustLevel::OfficialSubscriptionApi,
        },
        TransportSpec {
            protocol: Protocol::Responses,
            trust: TrustLevel::OfficialSubscriptionApi,
        },
        TransportSpec {
            protocol: Protocol::Messages,
            trust: TrustLevel::OfficialSubscriptionApi,
        },
    ],
    drivers: common::API_DRIVERS_WITH_USAGE,
    capabilities: Capabilities::STREAMING
        .union(Capabilities::MODEL_DISCOVERY)
        .union(Capabilities::USAGE_QUERY),
    default_model_protocols: &[Protocol::Chat],
};

/// Maps `supported_endpoints` (e.g. `["/chat/completions", "/messages"]`) to protocols.
pub(crate) fn protocols_from(entry: &Value) -> Vec<Protocol> {
    let mut out = Vec::new();
    if let Some(list) = entry.get("supported_endpoints").and_then(Value::as_array) {
        for e in list.iter().filter_map(Value::as_str) {
            let e = e.to_ascii_lowercase();
            let p = if e.contains("chat") {
                Protocol::Chat
            } else if e.contains("responses") {
                Protocol::Responses
            } else if e.contains("messages") {
                Protocol::Messages
            } else {
                continue;
            };
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    if out.is_empty() {
        out.push(Protocol::Chat);
    }
    out
}

/// What `/alpha/billing/subscriptions` reported.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Subscription {
    /// `None` on the free tier (`data: null`).
    pub plan_id: Option<String>,
    pub period_end_ms: Option<i64>,
}

/// `None` when the response does not tell (`success: false` is transient, not the free tier).
pub(crate) fn parse_subscription(v: &Value) -> Option<Subscription> {
    if v.get("success").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    let data = v.get("data")?;
    if data.is_null() {
        return Some(Subscription::default());
    }
    data.is_object().then(|| Subscription {
        plan_id: data
            .get("planId")
            .and_then(Value::as_str)
            .map(str::to_owned),
        period_end_ms: data.get("currentPeriodEnd").and_then(timestamp_ms),
    })
}

/// A capped rolling window (`{cap, used, resetAt}`) as a percentage; skipped without a cap.
fn cap_window(scope: &str, label: &str, seconds: u64, v: &Value) -> Option<QuotaWindow> {
    let cap = number_field(v, &["cap"]).filter(|c| *c > 0.0)?;
    let used = number_field(v, &["used"])?;
    Some(QuotaWindow {
        scope: scope.to_owned(),
        label: label.to_owned(),
        unit: QuotaUnit::Percent,
        used: Some((used / cap * 100.0).clamp(0.0, 100.0)),
        limit: None,
        window_seconds: Some(seconds),
        resets_at_ms: v.get("resetAt").and_then(timestamp_ms),
    })
}

/// Builds the report from `/alpha/billing/credits` plus the subscription, when known.
///
/// `monthlyCredits` is the remaining monthly allowance in USD. Purchased (top-up) credits
/// are spent once it runs out, so they are added to the monthly window's limit: the window
/// is exhausted only when both are gone.
pub(crate) fn parse_usage(
    v: &Value,
    subscription: Option<&Subscription>,
) -> Result<UsageReport, ProviderError> {
    let credits = object(v, &["credits"]).ok_or_else(|| {
        ProviderError::InvalidResponse("Command Code credits response has no `credits`".into())
    })?;
    let remaining = number_field(credits, &["monthlyCredits"])
        .ok_or_else(|| {
            ProviderError::InvalidResponse("Command Code credits have no `monthlyCredits`".into())
        })?
        .max(0.0);
    let purchased = number_field(credits, &["purchasedCredits"])
        .unwrap_or(0.0)
        .max(0.0);
    // Undocumented routes, labelled like the other CLI-internal usage sources.
    let mut report = UsageReport::new(UsageSource::PrivateEndpoint, Confidence::Exact);

    let limits = object(v, &["windowLimits"]).or_else(|| object(credits, &["windowLimits"]));
    for (scope, label, seconds, key) in [
        ("5h", "5h", FIVE_HOURS, "fiveHour"),
        ("weekly", "Weekly", WEEK, "weekly"),
    ] {
        if let Some(w) = limits
            .and_then(|l| object(l, &[key]))
            .and_then(|o| cap_window(scope, label, seconds, o))
        {
            report.windows.push(w);
        }
    }

    let plan_id = subscription.and_then(|s| s.plan_id.as_deref());
    let catalog = plan_id.and_then(|id| PLANS.iter().find(|(p, _, _)| *p == id));
    let granted = number_field(credits, &["monthlyCreditsGranted"])
        .filter(|g| *g > 0.0)
        .or(catalog.map(|(_, _, usd)| *usd));
    if let Some(granted) = granted {
        report.windows.push(QuotaWindow {
            scope: "monthly".into(),
            label: "Monthly".into(),
            unit: QuotaUnit::Usd,
            used: Some((granted - remaining).max(0.0)),
            limit: Some(granted + purchased),
            window_seconds: Some(MONTH),
            resets_at_ms: subscription.and_then(|s| s.period_end_ms),
        });
    }
    // Without a known allowance (or with top-ups) the spendable credit is shown as a balance.
    if granted.is_none() || purchased > 0.0 {
        report.balances.push(Balance {
            currency: "USD".into(),
            amount: remaining + purchased,
            included: Some(remaining),
            topup: (purchased > 0.0).then_some(purchased),
            granted: None,
        });
    }

    report.plan = subscription.map(|s| Plan {
        name: match (plan_id, catalog) {
            (_, Some((_, name, _))) => format!("Command Code {name}"),
            (Some(id), None) => id.to_owned(),
            (None, _) => "Command Code Free".into(),
        },
        price_usd_monthly: None,
        renews_at_ms: s.period_end_ms,
        auto_renew: None,
    });
    Ok(report)
}

#[derive(Debug, Default)]
pub struct CommandCode;

impl ProviderModule for CommandCode {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        &DESCRIPTOR
    }

    fn validate(&self, config: &ProviderConfig) -> Result<(), ProviderError> {
        common::validate(&DESCRIPTOR, config)
    }

    fn endpoint(
        &self,
        config: &ProviderConfig,
        protocol: Protocol,
        key: Option<&Secret>,
    ) -> Result<Endpoint, ProviderError> {
        common::bearer_all(&config.base_url(&DESCRIPTOR)?, protocol, key)
    }

    fn list_models<'a>(
        &'a self,
        ctx: DriverContext<'a>,
    ) -> BoxFuture<'a, Result<Vec<CatalogModel>, ProviderError>> {
        Box::pin(async move {
            let base = ctx.config.base_url(&DESCRIPTOR)?;
            common::list_openai_models(&ctx, &base, |_, entry| protocols_from(entry)).await
        })
    }

    fn fetch_usage<'a>(
        &'a self,
        ctx: DriverContext<'a>,
    ) -> BoxFuture<'a, Result<UsageReport, ProviderError>> {
        Box::pin(async move {
            let key = ctx.key.ok_or(ProviderError::MissingKey)?;
            // The billing routes live on the API origin, outside `/provider/v1`.
            let origin = common::origin_join(&ctx.config.base_url(&DESCRIPTOR)?, "")?;
            let (credits, subscription) = tokio::join!(
                common::get_bearer(
                    &ctx,
                    &origin,
                    CREDITS_PATH,
                    Some(key),
                    common::USAGE_TIMEOUT
                ),
                common::get_bearer(
                    &ctx,
                    &origin,
                    SUBSCRIPTIONS_PATH,
                    Some(key),
                    SUBSCRIPTION_TIMEOUT
                ),
            );
            let subscription = match subscription {
                Ok(v) => parse_subscription(&v),
                Err(e) => {
                    tracing::debug!(error = %e, "Command Code subscription lookup failed");
                    None
                }
            };
            parse_usage(&credits?, subscription.as_ref())
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::testutil::{MockUpstream, config, http};

    #[test]
    fn maps_supported_endpoints() {
        assert_eq!(
            protocols_from(
                &json!({"supported_endpoints": ["/chat/completions", "/messages", "/v1/responses"]})
            ),
            vec![Protocol::Chat, Protocol::Messages, Protocol::Responses]
        );
        assert_eq!(protocols_from(&json!({})), vec![Protocol::Chat]);
    }

    #[tokio::test]
    async fn catalog_uses_supported_endpoints() {
        let up = MockUpstream::json(
            "/provider/v1/models",
            json!({"data": [
                {"id": "deepseek/deepseek-v4-flash", "supported_endpoints": ["/chat/completions"]},
                {"id": "claude-sonnet-4-6", "supported_endpoints": ["/messages", "/chat/completions"]}
            ]}),
        )
        .await;
        let cfg = config(
            "commandcode",
            "commandcode",
            &format!("{}/provider/v1", up.base),
        );
        let key = Secret::new("cc-key");
        let http = http();
        let models = CommandCode
            .list_models(DriverContext {
                http: &http,
                config: &cfg,
                key: Some(&key),
                home: None,
            })
            .await
            .expect("models");
        let claude = models
            .iter()
            .find(|m| m.upstream_id == "claude-sonnet-4-6")
            .expect("claude");
        assert_eq!(claude.protocols, vec![Protocol::Messages, Protocol::Chat]);
        assert_eq!(
            up.last_header("authorization").as_deref(),
            Some("Bearer cc-key")
        );
    }

    fn credits() -> Value {
        json!({
            "credits": {
                "monthlyCredits": "52.5",
                "purchasedCredits": 0,
                "premiumMonthlyCredits": 0,
                "opensourceMonthlyCredits": 0
            },
            "windowLimits": {
                "fiveHour": {"cap": 20, "used": "5", "resetAt": 1_791_936_000},
                "weekly": {"cap": "100", "used": 40, "resetAt": "2026-10-14T00:00:00Z"}
            }
        })
    }

    #[test]
    fn parses_credit_windows_with_plan_allowance() {
        let sub = Subscription {
            plan_id: Some("individual-goat".into()),
            period_end_ms: Some(1_792_540_800_000),
        };
        let r = parse_usage(&credits(), Some(&sub)).expect("usage");
        assert_eq!(r.source, UsageSource::PrivateEndpoint);
        let scopes: Vec<_> = r.windows.iter().map(|w| w.scope.as_str()).collect();
        assert_eq!(scopes, ["5h", "weekly", "monthly"]);
        assert_eq!(r.windows[0].unit, QuotaUnit::Percent);
        assert_eq!(r.windows[0].used, Some(25.0));
        assert_eq!(r.windows[0].resets_at_ms, Some(1_791_936_000_000));
        assert_eq!(r.windows[1].used, Some(40.0));
        assert_eq!(r.windows[1].resets_at_ms, Some(1_791_936_000_000));
        let monthly = &r.windows[2];
        assert_eq!(monthly.unit, QuotaUnit::Usd);
        assert_eq!((monthly.used, monthly.limit), (Some(17.5), Some(70.0)));
        assert_eq!(monthly.resets_at_ms, Some(1_792_540_800_000));
        assert!(r.balances.is_empty());
        let plan = r.plan.expect("plan");
        assert_eq!(plan.name, "Command Code GOAT");
        assert_eq!(plan.renews_at_ms, Some(1_792_540_800_000));
    }

    #[test]
    fn top_ups_extend_the_monthly_window() {
        // Granted from the response; `windowLimits` nested in `credits`, a window with no cap.
        let v = json!({"credits": {
            "monthlyCredits": 0,
            "purchasedCredits": 5,
            "monthlyCreditsGranted": 30,
            "windowLimits": {"fiveHour": {"cap": 0, "used": 3}, "weekly": {"cap": 10, "used": 10}}
        }});
        let r = parse_usage(&v, None).expect("usage");
        let scopes: Vec<_> = r.windows.iter().map(|w| w.scope.as_str()).collect();
        assert_eq!(scopes, ["weekly", "monthly"]);
        let monthly = &r.windows[1];
        assert_eq!((monthly.used, monthly.limit), (Some(30.0), Some(35.0)));
        assert!(!monthly.is_exhausted());
        assert!(r.windows[0].is_exhausted());
        assert_eq!(r.balances[0].amount, 5.0);
        assert_eq!(r.balances[0].topup, Some(5.0));
        assert!(r.plan.is_none());
    }

    #[test]
    fn unknown_allowance_falls_back_to_a_balance() {
        let free = parse_subscription(&json!({"success": true, "data": null})).expect("free");
        let r = parse_usage(&credits(), Some(&free)).expect("usage");
        assert_eq!(r.windows.len(), 2);
        assert_eq!(r.balances[0].amount, 52.5);
        assert_eq!(r.balances[0].granted, None);
        assert_eq!(r.plan.expect("plan").name, "Command Code Free");

        let other = Subscription {
            plan_id: Some("team-seat".into()),
            period_end_ms: None,
        };
        let r = parse_usage(&credits(), Some(&other)).expect("usage");
        assert_eq!(r.plan.expect("plan").name, "team-seat");

        assert!(parse_usage(&json!({"credits": {}}), None).is_err());
        assert!(parse_usage(&json!({}), None).is_err());
    }

    #[test]
    fn parses_subscriptions() {
        assert_eq!(
            parse_subscription(&json!({"success": true, "data": {
                "planId": "individual-pro", "status": "active", "currentPeriodEnd": 1_792_540_800_000_i64
            }})),
            Some(Subscription {
                plan_id: Some("individual-pro".into()),
                period_end_ms: Some(1_792_540_800_000),
            })
        );
        assert_eq!(parse_subscription(&json!({"success": false})), None);
        assert_eq!(
            parse_subscription(&json!({"success": false, "data": null})),
            None
        );
    }

    #[tokio::test]
    async fn fetches_usage_from_the_api_origin() {
        let up = MockUpstream::routes(&[
            (CREDITS_PATH, 200, credits().to_string()),
            (
                SUBSCRIPTIONS_PATH,
                200,
                json!({"success": true, "data": {
                    "planId": "individual-max", "status": "active",
                    "currentPeriodEnd": "2026-10-21T00:00:00Z"
                }})
                .to_string(),
            ),
        ])
        .await;
        let cfg = config(
            "commandcode",
            "commandcode",
            &format!("{}/provider/v1", up.base),
        );
        let key = Secret::new("cc-key");
        let http = http();
        let ctx = DriverContext {
            http: &http,
            config: &cfg,
            key: Some(&key),
            home: None,
        };
        let r = CommandCode.fetch_usage(ctx).await.expect("usage");
        assert_eq!(r.windows.len(), 3);
        assert_eq!(r.windows[2].limit, Some(150.0));
        assert_eq!(r.windows[2].resets_at_ms, Some(1_792_540_800_000));
        assert_eq!(r.plan.expect("plan").name, "Command Code Max");
        assert_eq!(
            up.last_header("authorization").as_deref(),
            Some("Bearer cc-key")
        );
        assert_eq!(
            up.last_header("accept").as_deref(),
            Some("application/json")
        );
    }

    #[tokio::test]
    async fn subscription_failure_does_not_fail_usage() {
        let up = MockUpstream::routes(&[
            (CREDITS_PATH, 200, credits().to_string()),
            (SUBSCRIPTIONS_PATH, 500, "{}".into()),
        ])
        .await;
        let cfg = config(
            "commandcode",
            "commandcode",
            &format!("{}/provider/v1", up.base),
        );
        let key = Secret::new("cc-key");
        let http = http();
        let ctx = DriverContext {
            http: &http,
            config: &cfg,
            key: Some(&key),
            home: None,
        };
        let r = CommandCode.fetch_usage(ctx).await.expect("usage");
        assert_eq!(r.windows.len(), 2);
        assert!(r.plan.is_none());

        let ctx = DriverContext {
            http: &http,
            config: &cfg,
            key: None,
            home: None,
        };
        assert!(matches!(
            CommandCode.fetch_usage(ctx).await,
            Err(ProviderError::MissingKey)
        ));

        let denied = MockUpstream::status(CREDITS_PATH, 401, r#"{"error":"unauthorized"}"#).await;
        let cfg = config(
            "commandcode",
            "commandcode",
            &format!("{}/provider/v1", denied.base),
        );
        let ctx = DriverContext {
            http: &http,
            config: &cfg,
            key: Some(&key),
            home: None,
        };
        let err = CommandCode.fetch_usage(ctx).await.expect_err("401");
        assert!(!err.to_string().contains("cc-key"));
    }
}
