// SPDX-License-Identifier: Apache-2.0

//! DeepSeek API: Chat at `https://api.deepseek.com/v1`, Anthropic-compatible Messages at
//! `https://api.deepseek.com/anthropic`. Usage: `GET /user/balance`.

use loadout_protocol::Protocol;
use loadout_provider_sdk::time::number;
use loadout_provider_sdk::usage::{Balance, Confidence, UsageReport, UsageSource};
use loadout_provider_sdk::{
    AuthMethod, BoxFuture, Capabilities, CatalogModel, DriverContext, Endpoint, ProviderConfig,
    ProviderDescriptor, ProviderError, ProviderModule, TransportSpec, TrustLevel,
};
use loadout_secrets::Secret;
use serde_json::Value;

use crate::common::{self, ANTHROPIC_VERSION, origin_join};

static DESCRIPTOR: ProviderDescriptor = ProviderDescriptor {
    kind: "deepseek",
    display_name: "DeepSeek",
    fixed_slug: Some("deepseek"),
    default_base_url: Some("https://api.deepseek.com/v1"),
    auth: AuthMethod::ApiKey,
    transports: &[
        TransportSpec {
            protocol: Protocol::Chat,
            trust: TrustLevel::OfficialApi,
        },
        TransportSpec {
            protocol: Protocol::Messages,
            trust: TrustLevel::OfficialApi,
        },
    ],
    drivers: common::API_DRIVERS_WITH_USAGE,
    capabilities: Capabilities::STREAMING
        .union(Capabilities::MODEL_DISCOVERY)
        .union(Capabilities::BALANCE_QUERY),
    default_model_protocols: &[Protocol::Chat, Protocol::Messages],
};

pub(crate) fn parse_balance(v: &Value) -> Result<UsageReport, ProviderError> {
    let infos = v
        .get("balance_infos")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ProviderError::InvalidResponse("balance response has no balance_infos".into())
        })?;
    let mut report = UsageReport::new(UsageSource::OfficialApi, Confidence::Exact);
    for b in infos {
        let (Some(currency), Some(total)) = (
            b.get("currency").and_then(Value::as_str),
            b.get("total_balance").and_then(number),
        ) else {
            continue;
        };
        report.balances.push(Balance {
            currency: currency.to_owned(),
            amount: total,
            included: b.get("granted_balance").and_then(number),
            topup: b.get("topped_up_balance").and_then(number),
            granted: None,
        });
    }
    Ok(report)
}

#[derive(Debug, Default)]
pub struct DeepSeek;

impl ProviderModule for DeepSeek {
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
        let base = config.base_url(&DESCRIPTOR)?;
        match protocol {
            Protocol::Chat => common::bearer_endpoint(&base, protocol.upstream_path(), key),
            Protocol::Messages => {
                let mut e = Endpoint::new(origin_join(&base, "/anthropic/v1/messages")?)
                    .header("anthropic-version", ANTHROPIC_VERSION)?;
                if let Some(k) = key {
                    e = e.secret_header("x-api-key", "", k)?;
                }
                Ok(e)
            }
            Protocol::Responses => Err(ProviderError::NotSupported("this protocol")),
        }
    }

    fn list_models<'a>(
        &'a self,
        ctx: DriverContext<'a>,
    ) -> BoxFuture<'a, Result<Vec<CatalogModel>, ProviderError>> {
        Box::pin(async move {
            let base = ctx.config.base_url(&DESCRIPTOR)?;
            common::list_openai_models(&ctx, &base, |_, _| vec![Protocol::Chat, Protocol::Messages])
                .await
        })
    }

    fn fetch_usage<'a>(
        &'a self,
        ctx: DriverContext<'a>,
    ) -> BoxFuture<'a, Result<UsageReport, ProviderError>> {
        Box::pin(async move {
            let key = ctx.key.ok_or(ProviderError::MissingKey)?;
            let url = origin_join(&ctx.config.base_url(&DESCRIPTOR)?, "/user/balance")?;
            let e = common::bearer_endpoint(&url, "", Some(key))?;
            let v = ctx
                .http
                .get_json(
                    e.url.trim_end_matches('/'),
                    e.headers,
                    common::USAGE_TIMEOUT,
                )
                .await?;
            parse_balance(&v)
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::testutil::config;

    #[test]
    fn parses_balance() {
        let v = json!({"is_available": true, "balance_infos": [
            {"currency": "CNY", "total_balance": "110.00", "granted_balance": "10.00", "topped_up_balance": "100.00"}
        ]});
        let r = parse_balance(&v).expect("balance");
        assert_eq!(r.balances[0].currency, "CNY");
        assert_eq!(r.balances[0].amount, 110.0);
        assert_eq!(r.balances[0].included, Some(10.0));
        assert!(
            !r.is_exhausted(),
            "a plain balance without a grant is never 'exhausted' by itself"
        );
    }

    #[test]
    fn messages_use_the_anthropic_path() {
        let cfg = config("deepseek", "deepseek", "https://api.deepseek.com/v1");
        let key = Secret::new("sk-ds");
        let e = DeepSeek
            .endpoint(&cfg, Protocol::Messages, Some(&key))
            .expect("endpoint");
        assert_eq!(e.url, "https://api.deepseek.com/anthropic/v1/messages");
        assert_eq!(e.headers["x-api-key"], "sk-ds");
        let c = DeepSeek
            .endpoint(&cfg, Protocol::Chat, Some(&key))
            .expect("endpoint");
        assert_eq!(c.url, "https://api.deepseek.com/v1/chat/completions");
    }
}
