// SPDX-License-Identifier: Apache-2.0

//! Generic Anthropic-compatible provider: `/messages` under a user-supplied base URL, e.g.
//! `https://api.anthropic.com/v1`.
//!
//! Settings (`providers.config_json`):
//! - `auth_style`: `"x-api-key"` (default) or `"bearer"`
//! - `anthropic_version`: default `2023-06-01`
//! - `default_max_tokens`: used when a converted request has no max_tokens (default 8192)
//! - `allow_insecure_http`

use std::time::Duration;

use loadout_protocol::{Protocol, messages};
use loadout_provider_sdk::{
    AuthMethod, BoxFuture, Capabilities, CatalogModel, DriverContext, Drivers, Endpoint,
    ProviderConfig, ProviderDescriptor, ProviderError, ProviderModule, TransportSpec, TrustLevel,
    join_url, validate_base_url,
};
use loadout_secrets::Secret;
use serde_json::Value;

pub(crate) static DESCRIPTOR: ProviderDescriptor = ProviderDescriptor {
    kind: "anthropic-compatible",
    display_name: "Anthropic-compatible API",
    fixed_slug: None,
    default_base_url: None,
    auth: AuthMethod::ApiKey,
    transports: &[TransportSpec {
        protocol: Protocol::Messages,
        trust: TrustLevel::OfficialApi,
    }],
    drivers: Drivers {
        discovery: false,
        auth: true,
        usage: false,
        catalog: true,
        transport: true,
        health: false,
    },
    capabilities: Capabilities::STREAMING.union(Capabilities::MODEL_DISCOVERY),
    default_model_protocols: &[Protocol::Messages],
};

pub(crate) const ANTHROPIC_VERSION: &str = "2023-06-01";
const CATALOG_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Default)]
pub struct AnthropicCompatible;

/// Endpoint with Anthropic-style authentication.
pub(crate) fn anthropic_endpoint(
    config: &ProviderConfig,
    base: &str,
    path: &str,
    key: Option<&Secret>,
) -> Result<Endpoint, ProviderError> {
    let mut e = Endpoint::new(join_url(base, path)).header(
        "anthropic-version",
        config
            .setting_str("anthropic_version")
            .unwrap_or(ANTHROPIC_VERSION),
    )?;
    if let Some(key) = key.filter(|k| !k.is_empty()) {
        e = match config.setting_str("auth_style") {
            Some("bearer") => e.secret_header("authorization", "Bearer ", key)?,
            _ => e.secret_header("x-api-key", "", key)?,
        };
    }
    Ok(e)
}

pub(crate) fn parse_anthropic_models(v: &Value) -> Result<Vec<CatalogModel>, ProviderError> {
    let data = v.get("data").and_then(Value::as_array).ok_or_else(|| {
        ProviderError::InvalidResponse("models response has no `data` array".into())
    })?;
    Ok(data
        .iter()
        .filter_map(|m| {
            let mut model = CatalogModel::new(m.get("id").and_then(Value::as_str)?);
            model.display_name = m
                .get("display_name")
                .and_then(Value::as_str)
                .map(str::to_owned);
            model.protocols = vec![Protocol::Messages];
            model.capabilities |= Capabilities::VISION;
            Some(model)
        })
        .collect())
}

impl ProviderModule for AnthropicCompatible {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        &DESCRIPTOR
    }

    fn validate(&self, config: &ProviderConfig) -> Result<(), ProviderError> {
        let base = config.base_url(&DESCRIPTOR)?;
        validate_base_url(
            &base,
            config.setting_bool("allow_insecure_http").unwrap_or(false),
        )?;
        match config.setting_str("auth_style") {
            None | Some("x-api-key" | "bearer") => Ok(()),
            Some(other) => Err(ProviderError::Config(format!(
                "auth_style must be x-api-key or bearer, not {other:?}"
            ))),
        }
    }

    fn endpoint(
        &self,
        config: &ProviderConfig,
        protocol: Protocol,
        key: Option<&Secret>,
    ) -> Result<Endpoint, ProviderError> {
        if protocol != Protocol::Messages {
            return Err(ProviderError::NotSupported("this protocol"));
        }
        anthropic_endpoint(config, &config.base_url(&DESCRIPTOR)?, "/messages", key)
    }

    fn messages_options(&self, config: &ProviderConfig) -> messages::EncodeOptions {
        messages::EncodeOptions {
            default_max_tokens: config
                .settings
                .get("default_max_tokens")
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(messages::DEFAULT_MAX_TOKENS),
        }
    }

    fn list_models<'a>(
        &'a self,
        ctx: DriverContext<'a>,
    ) -> BoxFuture<'a, Result<Vec<CatalogModel>, ProviderError>> {
        Box::pin(async move {
            let base = ctx.config.base_url(&DESCRIPTOR)?;
            let e = anthropic_endpoint(ctx.config, &base, "/models?limit=1000", ctx.key)?;
            let v = ctx
                .http
                .get_json(&e.url, e.headers, CATALOG_TIMEOUT)
                .await?;
            parse_anthropic_models(&v)
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::testutil::{MockUpstream, config, http};

    #[test]
    fn endpoint_headers() {
        let cfg = config(
            "claude",
            "anthropic-compatible",
            "https://api.anthropic.test/v1",
        );
        let key = Secret::new("sk-ant-xyz");
        let e = AnthropicCompatible
            .endpoint(&cfg, Protocol::Messages, Some(&key))
            .expect("endpoint");
        assert_eq!(e.url, "https://api.anthropic.test/v1/messages");
        assert_eq!(e.headers["x-api-key"], "sk-ant-xyz");
        assert_eq!(e.headers["anthropic-version"], ANTHROPIC_VERSION);
        assert!(e.headers.get("authorization").is_none());

        let mut bearer = cfg.clone();
        bearer.settings = json!({"auth_style": "bearer", "anthropic_version": "2099-01-01"});
        let e = AnthropicCompatible
            .endpoint(&bearer, Protocol::Messages, Some(&key))
            .expect("endpoint");
        assert_eq!(e.headers["authorization"], "Bearer sk-ant-xyz");
        assert_eq!(e.headers["anthropic-version"], "2099-01-01");
        assert!(
            AnthropicCompatible
                .endpoint(&cfg, Protocol::Chat, Some(&key))
                .is_err()
        );
    }

    #[test]
    fn default_max_tokens_setting() {
        let mut cfg = config("c", "anthropic-compatible", "https://a.test/v1");
        assert_eq!(
            AnthropicCompatible
                .messages_options(&cfg)
                .default_max_tokens,
            messages::DEFAULT_MAX_TOKENS
        );
        cfg.settings = json!({"default_max_tokens": 32000});
        assert_eq!(
            AnthropicCompatible
                .messages_options(&cfg)
                .default_max_tokens,
            32000
        );
    }

    #[tokio::test]
    async fn lists_models() {
        let upstream = MockUpstream::json(
            "/v1/models",
            json!({"data": [{"type": "model", "id": "claude-x", "display_name": "Claude X"}], "has_more": false}),
        )
        .await;
        let cfg = config(
            "c",
            "anthropic-compatible",
            &format!("{}/v1", upstream.base),
        );
        let key = Secret::new("k");
        let http = http();
        let models = AnthropicCompatible
            .list_models(DriverContext {
                http: &http,
                config: &cfg,
                key: Some(&key),
                home: None,
            })
            .await
            .expect("models");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].display_name.as_deref(), Some("Claude X"));
        assert_eq!(models[0].protocols, vec![Protocol::Messages]);
        assert_eq!(upstream.last_header("x-api-key").as_deref(), Some("k"));
    }
}
