// SPDX-License-Identifier: Apache-2.0

//! OpenRouter (`https://openrouter.ai/api/v1`). The catalog carries prices (USD per token,
//! as strings), context length, input modalities and supported parameters. Usage:
//! `GET /key` → credit limit, remaining credit and daily / weekly / monthly spend.

use loadout_protocol::Protocol;
use loadout_provider_sdk::time::{number, number_field};
use loadout_provider_sdk::usage::{
    Balance, Confidence, Plan, QuotaUnit, QuotaWindow, UsageReport, UsageSource,
};
use loadout_provider_sdk::{
    AuthMethod, BoxFuture, Capabilities, CatalogModel, DriverContext, Endpoint, ProviderConfig,
    ProviderDescriptor, ProviderError, ProviderModule, TransportSpec, TrustLevel,
};
use loadout_secrets::Secret;
use serde_json::Value;

use crate::common::{self, MONTH, WEEK};

static DESCRIPTOR: ProviderDescriptor = ProviderDescriptor {
    kind: "openrouter",
    display_name: "OpenRouter",
    fixed_slug: Some("openrouter"),
    default_base_url: Some("https://openrouter.ai/api/v1"),
    auth: AuthMethod::ApiKey,
    transports: &[TransportSpec {
        protocol: Protocol::Chat,
        trust: TrustLevel::OfficialApi,
    }],
    drivers: common::API_DRIVERS_WITH_USAGE,
    capabilities: Capabilities::STREAMING
        .union(Capabilities::MODEL_DISCOVERY)
        .union(Capabilities::BALANCE_QUERY),
    default_model_protocols: &[Protocol::Chat],
};

fn per_million(pricing: &Value, key: &str) -> Option<f64> {
    pricing
        .get(key)
        .and_then(number)
        .filter(|p| *p >= 0.0)
        .map(|p| p * 1_000_000.0)
}

pub(crate) fn parse_models(v: &Value) -> Result<Vec<CatalogModel>, ProviderError> {
    let data = v.get("data").and_then(Value::as_array).ok_or_else(|| {
        ProviderError::InvalidResponse("models response has no `data` array".into())
    })?;
    Ok(data
        .iter()
        .filter_map(|m| {
            let mut model = CatalogModel::new(m.get("id").and_then(Value::as_str)?);
            model.display_name = m.get("name").and_then(Value::as_str).map(str::to_owned);
            model.context_window = m
                .get("context_length")
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok());
            model.max_output_tokens = m
                .get("top_provider")
                .and_then(|t| t.get("max_completion_tokens"))
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok());
            model.protocols = vec![Protocol::Chat];
            let mut caps = Capabilities::STREAMING;
            let modalities = m
                .get("architecture")
                .and_then(|a| a.get("input_modalities"));
            if modalities
                .and_then(Value::as_array)
                .is_some_and(|a| a.iter().any(|x| x == "image"))
            {
                caps |= Capabilities::VISION;
            }
            if let Some(params) = m.get("supported_parameters").and_then(Value::as_array) {
                if params.iter().any(|p| p == "tools") {
                    caps |= Capabilities::TOOLS;
                }
                if params.iter().any(|p| p == "reasoning") {
                    caps |= Capabilities::REASONING;
                }
            }
            model.capabilities = caps;
            if let Some(pricing) = m.get("pricing") {
                model.input_price = per_million(pricing, "prompt");
                model.output_price = per_million(pricing, "completion");
                model.cache_read_price = per_million(pricing, "input_cache_read");
                model.cache_write_price = per_million(pricing, "input_cache_write");
            }
            Some(model)
        })
        .collect())
}

pub(crate) fn parse_key(v: &Value) -> Result<UsageReport, ProviderError> {
    let d = v
        .get("data")
        .filter(|d| d.is_object())
        .ok_or_else(|| ProviderError::InvalidResponse("key response has no `data`".into()))?;
    let mut report = UsageReport::new(UsageSource::OfficialApi, Confidence::Exact);
    if let (Some(limit), Some(remaining)) = (
        number_field(d, &["limit"]),
        number_field(d, &["limit_remaining"]),
    ) {
        report.balances.push(Balance {
            currency: "USD".into(),
            amount: remaining,
            included: None,
            topup: None,
            granted: Some(limit),
        });
    }
    let spend = [
        ("daily", "Today", 86_400),
        ("weekly", "This week", WEEK),
        ("monthly", "This month", MONTH),
    ];
    for (scope, label, secs) in spend {
        if let Some(used) = number_field(d, &[&format!("usage_{scope}")]) {
            report.windows.push(QuotaWindow {
                scope: format!("spend:{scope}"),
                label: label.to_owned(),
                unit: QuotaUnit::Usd,
                used: Some(used),
                limit: None,
                window_seconds: Some(secs),
                resets_at_ms: None,
            });
        }
    }
    if let Some(free) = d.get("free_model_daily_requests").filter(|f| f.is_object())
        && let (Some(used), Some(limit)) = (
            number_field(free, &["used"]),
            number_field(free, &["limit"]),
        )
    {
        report.windows.push(QuotaWindow {
            scope: "free:daily".into(),
            label: "Free models".into(),
            unit: QuotaUnit::Requests,
            used: Some(used),
            limit: Some(limit),
            window_seconds: Some(86_400),
            resets_at_ms: None,
        });
    }
    report.plan = d
        .get("is_free_tier")
        .and_then(Value::as_bool)
        .map(|free| Plan {
            name: if free { "Free tier" } else { "Paid" }.into(),
            price_usd_monthly: None,
            renews_at_ms: None,
            auto_renew: None,
        });
    report.account_label = d.get("label").and_then(Value::as_str).map(str::to_owned);
    Ok(report)
}

#[derive(Debug, Default)]
pub struct OpenRouter;

impl ProviderModule for OpenRouter {
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
        if protocol != Protocol::Chat {
            return Err(ProviderError::NotSupported("this protocol"));
        }
        common::bearer_endpoint(
            &config.base_url(&DESCRIPTOR)?,
            protocol.upstream_path(),
            key,
        )
    }

    fn list_models<'a>(
        &'a self,
        ctx: DriverContext<'a>,
    ) -> BoxFuture<'a, Result<Vec<CatalogModel>, ProviderError>> {
        Box::pin(async move {
            let base = ctx.config.base_url(&DESCRIPTOR)?;
            let v = common::get_bearer(&ctx, &base, "/models", ctx.key, common::CATALOG_TIMEOUT)
                .await?;
            parse_models(&v)
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
                common::get_bearer(&ctx, &base, "/key", Some(key), common::USAGE_TIMEOUT).await?;
            parse_key(&v)
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_catalog_prices_and_capabilities() {
        let v = json!({"data": [{
            "id": "anthropic/claude-x", "name": "Claude X", "context_length": 200000,
            "pricing": {"prompt": "0.000003", "completion": "0.000015", "input_cache_read": "0.0000003"},
            "top_provider": {"max_completion_tokens": 64000},
            "architecture": {"input_modalities": ["text", "image"]},
            "supported_parameters": ["tools", "reasoning", "temperature"]
        }]});
        let m = &parse_models(&v).expect("models")[0];
        assert_eq!(m.context_window, Some(200_000));
        assert_eq!(m.max_output_tokens, Some(64_000));
        assert!((m.input_price.expect("in") - 3.0).abs() < 1e-9);
        assert!((m.output_price.expect("out") - 15.0).abs() < 1e-9);
        assert!((m.cache_read_price.expect("cache") - 0.3).abs() < 1e-9);
        assert!(
            m.capabilities
                .contains(Capabilities::VISION | Capabilities::TOOLS | Capabilities::REASONING)
        );
    }

    #[test]
    fn parses_key_usage() {
        let v = json!({"data": {
            "label": "sk-or-v1-abc...xyz", "limit": 20, "limit_remaining": 12.5, "limit_reset": "monthly",
            "usage": 7.5, "usage_daily": 0.4, "usage_weekly": 2.1, "usage_monthly": 7.5,
            "is_free_tier": false,
            "free_model_daily_requests": {"used": 3, "limit": 1000, "remaining": 997}
        }});
        let r = parse_key(&v).expect("usage");
        assert_eq!(r.balances[0].amount, 12.5);
        assert_eq!(r.balances[0].granted, Some(20.0));
        assert_eq!(r.windows.len(), 4);
        assert_eq!(r.windows[2].used, Some(7.5));
        assert!(!r.is_exhausted());
        assert_eq!(r.plan.expect("plan").name, "Paid");

        let unlimited = json!({"data": {"limit": null, "limit_remaining": null, "usage_daily": 1}});
        assert!(parse_key(&unlimited).expect("usage").balances.is_empty());
    }
}
