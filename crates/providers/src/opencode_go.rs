// SPDX-License-Identifier: Apache-2.0

//! OpenCode Go (`https://opencode.ai/zen/go/v1`): a subscription with an official API key
//! for third-party tools. Models are served over Chat, Responses or Messages depending on
//! the model family (docs: opencode.ai/docs/go). Usage: `GET /usage`.

use loadout_protocol::Protocol;
use loadout_provider_sdk::time::timestamp_ms;
use loadout_provider_sdk::usage::{Confidence, Plan, UsageReport, UsageSource};
use loadout_provider_sdk::{
    AuthMethod, BoxFuture, Capabilities, CatalogModel, DriverContext, Endpoint, ProviderConfig,
    ProviderDescriptor, ProviderError, ProviderModule, TransportSpec, TrustLevel,
};
use loadout_secrets::Secret;
use serde_json::Value;

use crate::common::{self, FIVE_HOURS, MONTH, WEEK, object, window};

static DESCRIPTOR: ProviderDescriptor = ProviderDescriptor {
    kind: "opencode-go",
    display_name: "OpenCode Go",
    fixed_slug: Some("opencode-go"),
    default_base_url: Some("https://opencode.ai/zen/go/v1"),
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

/// Endpoint per model family, as documented by OpenCode (can be overridden per model).
pub(crate) fn protocol_for(model: &str) -> Protocol {
    let m = model.to_ascii_lowercase();
    if m.contains("minimax") || m.contains("qwen") {
        Protocol::Messages
    } else if m.contains("grok") {
        Protocol::Responses
    } else {
        Protocol::Chat
    }
}

pub(crate) fn parse_usage(v: &Value) -> Result<UsageReport, ProviderError> {
    let root = v.get("usage").filter(|u| u.is_object()).unwrap_or(v);
    let mut report = UsageReport::new(UsageSource::OfficialApi, Confidence::Exact);
    let specs: [(&str, &str, u64, &[&str]); 3] = [
        (
            "5h",
            "5h",
            FIVE_HOURS,
            &["rolling", "rollingUsage", "fiveHour", "five_hour"],
        ),
        ("weekly", "Weekly", WEEK, &["weekly", "weeklyUsage", "week"]),
        (
            "monthly",
            "Monthly",
            MONTH,
            &["monthly", "monthlyUsage", "month"],
        ),
    ];
    for (scope, label, secs, keys) in specs {
        if let Some(w) = object(root, keys).and_then(|o| window(scope, label, Some(secs), o)) {
            report.windows.push(w);
        }
    }
    if report.windows.is_empty() {
        return Err(ProviderError::InvalidResponse(
            "OpenCode Go usage has no windows".into(),
        ));
    }
    let renews = ["renewsAt", "expiresAt", "endsAt"]
        .iter()
        .find_map(|k| root.get(*k).and_then(timestamp_ms));
    report.plan = Some(Plan {
        name: "OpenCode Go".into(),
        price_usd_monthly: None,
        renews_at_ms: renews,
        auto_renew: None,
    });
    Ok(report)
}

#[derive(Debug, Default)]
pub struct OpenCodeGo;

impl ProviderModule for OpenCodeGo {
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
            common::list_openai_models(&ctx, &base, |id, _| vec![protocol_for(id)]).await
        })
    }

    fn fetch_usage<'a>(
        &'a self,
        ctx: DriverContext<'a>,
    ) -> BoxFuture<'a, Result<UsageReport, ProviderError>> {
        Box::pin(async move {
            let key = ctx.key.ok_or(ProviderError::MissingKey)?;
            let base = ctx.config.base_url(&DESCRIPTOR)?;
            let v =
                common::get_bearer(&ctx, &base, "/usage", Some(key), common::USAGE_TIMEOUT).await?;
            parse_usage(&v)
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::testutil::{MockUpstream, config, http};
    use loadout_provider_sdk::usage::QuotaUnit;

    #[test]
    fn protocol_mapping_follows_model_family() {
        assert_eq!(protocol_for("minimax-m3"), Protocol::Messages);
        assert_eq!(protocol_for("qwen3.8-max"), Protocol::Messages);
        assert_eq!(protocol_for("grok-4.7"), Protocol::Responses);
        assert_eq!(protocol_for("kimi-k3"), Protocol::Chat);
        assert_eq!(protocol_for("deepseek-v4-flash"), Protocol::Chat);
    }

    #[test]
    fn parses_usage_windows() {
        let v = json!({"usage": {
            "rolling": {"usagePercent": 78.4, "resetInSec": 6240},
            "weekly": {"usagePercent": 58, "resetInSec": 266400},
            "monthly": {"used": 8.3, "limit": 12},
            "renewsAt": "2026-10-14T00:00:00Z"
        }});
        let r = parse_usage(&v).expect("usage");
        assert_eq!(r.windows.len(), 3);
        assert_eq!(r.windows[0].scope, "5h");
        assert_eq!(r.windows[0].unit, QuotaUnit::Percent);
        assert_eq!(r.windows[2].limit, Some(12.0));
        assert_eq!(r.plan.expect("plan").renews_at_ms, Some(1_791_936_000_000));

        // Older field names are accepted too.
        let legacy = json!({"rollingUsage": {"usagePercent": 10, "resetInSec": 5}});
        assert_eq!(parse_usage(&legacy).expect("usage").windows.len(), 1);
        assert!(parse_usage(&json!({"usage": {}})).is_err());
    }

    #[test]
    fn messages_endpoint_carries_anthropic_headers() {
        let cfg = config(
            "opencode-go",
            "opencode-go",
            "https://opencode.ai/zen/go/v1",
        );
        let key = Secret::new("sk-go");
        let e = OpenCodeGo
            .endpoint(&cfg, Protocol::Messages, Some(&key))
            .expect("endpoint");
        assert_eq!(e.url, "https://opencode.ai/zen/go/v1/messages");
        assert_eq!(e.headers["authorization"], "Bearer sk-go");
        assert_eq!(e.headers["x-api-key"], "sk-go");
        assert!(e.headers.contains_key("anthropic-version"));
        let chat = OpenCodeGo
            .endpoint(&cfg, Protocol::Chat, Some(&key))
            .expect("endpoint");
        assert!(chat.headers.get("x-api-key").is_none());
    }

    #[tokio::test]
    async fn fetches_usage_and_models() {
        let usage = MockUpstream::json(
            "/v1/usage",
            json!({"usage": {"rolling": {"usagePercent": 12, "resetInSec": 100}}}),
        )
        .await;
        let cfg = config("opencode-go", "opencode-go", &format!("{}/v1", usage.base));
        let key = Secret::new("sk-go");
        let http = http();
        let ctx = DriverContext {
            http: &http,
            config: &cfg,
            key: Some(&key),
            home: None,
        };
        let r = OpenCodeGo.fetch_usage(ctx).await.expect("usage");
        assert_eq!(r.windows[0].used, Some(12.0));
        assert_eq!(
            usage.last_header("authorization").as_deref(),
            Some("Bearer sk-go")
        );

        let models = MockUpstream::json(
            "/v1/models",
            json!({"data": [{"id": "minimax-m3"}, {"id": "kimi-k3"}]}),
        )
        .await;
        let cfg = config("opencode-go", "opencode-go", &format!("{}/v1", models.base));
        let ctx = DriverContext {
            http: &http,
            config: &cfg,
            key: Some(&key),
            home: None,
        };
        let list = OpenCodeGo.list_models(ctx).await.expect("models");
        assert_eq!(list[0].upstream_id, "kimi-k3");
        assert_eq!(list[0].protocols, vec![Protocol::Chat]);
        assert_eq!(list[1].protocols, vec![Protocol::Messages]);
    }
}
