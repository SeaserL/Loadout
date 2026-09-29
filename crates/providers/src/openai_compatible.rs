// SPDX-License-Identifier: Apache-2.0

//! Generic OpenAI-compatible provider: any server exposing `/chat/completions` (and optionally
//! `/responses`) under a user-supplied base URL, e.g. `https://api.example.com/v1`.
//!
//! Settings (`providers.config_json`):
//! - `max_tokens_field`: `"max_tokens"` (default) or `"max_completion_tokens"`
//! - `allow_insecure_http`: permit plain http to a non-loopback host

use std::time::Duration;

use loadout_protocol::{Protocol, chat};
use loadout_provider_sdk::{
    AuthMethod, BoxFuture, Capabilities, CatalogModel, DriverContext, Drivers, Endpoint,
    ProviderConfig, ProviderDescriptor, ProviderError, ProviderModule, TransportSpec, TrustLevel,
    join_url, validate_base_url,
};
use loadout_secrets::Secret;
use reqwest::header::HeaderMap;
use serde_json::Value;

pub(crate) static DESCRIPTOR: ProviderDescriptor = ProviderDescriptor {
    kind: "openai-compatible",
    display_name: "OpenAI-compatible API",
    fixed_slug: None,
    default_base_url: None,
    auth: AuthMethod::OptionalApiKey,
    transports: &[
        TransportSpec {
            protocol: Protocol::Chat,
            trust: TrustLevel::OfficialApi,
        },
        TransportSpec {
            protocol: Protocol::Responses,
            trust: TrustLevel::OfficialApi,
        },
    ],
    drivers: Drivers {
        discovery: false,
        auth: true,
        usage: false,
        catalog: true,
        transport: true,
        health: false,
    },
    capabilities: Capabilities::STREAMING.union(Capabilities::MODEL_DISCOVERY),
    default_model_protocols: &[Protocol::Chat],
};

const CATALOG_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Default)]
pub struct OpenAiCompatible;

pub(crate) fn bearer_endpoint(
    base: &str,
    path: &str,
    key: Option<&Secret>,
) -> Result<Endpoint, ProviderError> {
    let mut endpoint = Endpoint::new(join_url(base, path));
    if let Some(key) = key.filter(|k| !k.is_empty()) {
        endpoint = endpoint.secret_header("authorization", "Bearer ", key)?;
    }
    Ok(endpoint)
}

/// Parses an OpenAI-style `GET /models` response (`{"data":[{"id":…}]}`), picking up the
/// optional metadata some servers include (OpenRouter, vLLM, LiteLLM…).
pub(crate) fn parse_model_list(v: &Value) -> Result<Vec<CatalogModel>, ProviderError> {
    let data = v.get("data").and_then(Value::as_array).ok_or_else(|| {
        ProviderError::InvalidResponse("models response has no `data` array".into())
    })?;
    let mut models: Vec<CatalogModel> = data
        .iter()
        .filter_map(|m| {
            let id = m.get("id").and_then(Value::as_str)?;
            let mut model = CatalogModel::new(id);
            model.display_name = m
                .get("name")
                .or_else(|| m.get("display_name"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            model.context_window = ["context_length", "context_window", "max_model_len"]
                .iter()
                .find_map(|k| m.get(*k).and_then(Value::as_u64))
                .and_then(|n| u32::try_from(n).ok());
            Some(model)
        })
        .collect();
    models.sort_by(|a, b| a.upstream_id.cmp(&b.upstream_id));
    models.dedup_by(|a, b| a.upstream_id == b.upstream_id);
    Ok(models)
}

impl ProviderModule for OpenAiCompatible {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        &DESCRIPTOR
    }

    fn validate(&self, config: &ProviderConfig) -> Result<(), ProviderError> {
        let base = config.base_url(&DESCRIPTOR)?;
        validate_base_url(
            &base,
            config.setting_bool("allow_insecure_http").unwrap_or(false),
        )?;
        match config.setting_str("max_tokens_field") {
            None | Some("max_tokens" | "max_completion_tokens") => Ok(()),
            Some(other) => Err(ProviderError::Config(format!(
                "max_tokens_field must be max_tokens or max_completion_tokens, not {other:?}"
            ))),
        }
    }

    fn trust(&self, config: &ProviderConfig, protocol: Protocol) -> Option<TrustLevel> {
        DESCRIPTOR.transport(protocol)?;
        let local = config
            .base_url
            .as_deref()
            .and_then(|b| reqwest::Url::parse(b).ok())
            .is_some_and(|u| loadout_provider_sdk::is_loopback_url(&u));
        Some(if local {
            TrustLevel::Local
        } else {
            TrustLevel::OfficialApi
        })
    }

    fn endpoint(
        &self,
        config: &ProviderConfig,
        protocol: Protocol,
        key: Option<&Secret>,
    ) -> Result<Endpoint, ProviderError> {
        if DESCRIPTOR.transport(protocol).is_none() {
            return Err(ProviderError::NotSupported("this protocol"));
        }
        bearer_endpoint(
            &config.base_url(&DESCRIPTOR)?,
            protocol.upstream_path(),
            key,
        )
    }

    fn chat_options(&self, config: &ProviderConfig) -> chat::EncodeOptions {
        chat::EncodeOptions {
            max_tokens_field: match config.setting_str("max_tokens_field") {
                Some("max_completion_tokens") => chat::MaxTokensField::MaxCompletionTokens,
                _ => chat::MaxTokensField::MaxTokens,
            },
        }
    }

    fn list_models<'a>(
        &'a self,
        ctx: DriverContext<'a>,
    ) -> BoxFuture<'a, Result<Vec<CatalogModel>, ProviderError>> {
        Box::pin(async move {
            let endpoint = bearer_endpoint(&ctx.config.base_url(&DESCRIPTOR)?, "/models", ctx.key)?;
            let mut headers: HeaderMap = endpoint.headers;
            headers.insert("accept", "application/json".parse().expect("static"));
            let v = ctx
                .http
                .get_json(&endpoint.url, headers, CATALOG_TIMEOUT)
                .await?;
            parse_model_list(&v)
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::testutil::{MockUpstream, config, http};

    #[test]
    fn endpoint_uses_bearer_and_protocol_path() {
        let cfg = config("my-vllm", "openai-compatible", "https://llm.example/v1/");
        let key = Secret::new("sk-test-123");
        let e = OpenAiCompatible
            .endpoint(&cfg, Protocol::Chat, Some(&key))
            .expect("endpoint");
        assert_eq!(e.url, "https://llm.example/v1/chat/completions");
        assert_eq!(e.headers["authorization"], "Bearer sk-test-123");
        assert!(e.headers["authorization"].is_sensitive());
        assert!(!format!("{e:?}").contains("sk-test-123"));

        let no_key = OpenAiCompatible
            .endpoint(&cfg, Protocol::Responses, None)
            .expect("endpoint");
        assert_eq!(no_key.url, "https://llm.example/v1/responses");
        assert!(no_key.headers.get("authorization").is_none());

        assert!(
            OpenAiCompatible
                .endpoint(&cfg, Protocol::Messages, None)
                .is_err()
        );
    }

    #[test]
    fn trust_is_local_for_loopback() {
        let local = config("ollama", "openai-compatible", "http://127.0.0.1:11434/v1");
        assert_eq!(
            OpenAiCompatible.trust(&local, Protocol::Chat),
            Some(TrustLevel::Local)
        );
        let remote = config("x", "openai-compatible", "https://x.test/v1");
        assert_eq!(
            OpenAiCompatible.trust(&remote, Protocol::Chat),
            Some(TrustLevel::OfficialApi)
        );
    }

    #[test]
    fn validation_and_options() {
        assert!(
            OpenAiCompatible
                .validate(&config("a", "openai-compatible", "http://10.1.1.1/v1"))
                .is_err()
        );
        let mut cfg = config("a", "openai-compatible", "https://a.test/v1");
        cfg.settings = json!({"max_tokens_field": "max_completion_tokens"});
        assert!(OpenAiCompatible.validate(&cfg).is_ok());
        assert_eq!(
            OpenAiCompatible.chat_options(&cfg).max_tokens_field,
            chat::MaxTokensField::MaxCompletionTokens
        );
        cfg.settings = json!({"max_tokens_field": "nope"});
        assert!(OpenAiCompatible.validate(&cfg).is_err());
    }

    #[tokio::test]
    async fn lists_models_from_upstream() {
        let upstream = MockUpstream::json(
            "/v1/models",
            json!({"object": "list", "data": [
                {"id": "b-model", "context_length": 128000},
                {"id": "a-model", "name": "Model A"},
                {"id": "a-model"}
            ]}),
        )
        .await;
        let cfg = config("m", "openai-compatible", &format!("{}/v1", upstream.base));
        let key = Secret::new("sk-list");
        let http = http();
        let models = OpenAiCompatible
            .list_models(DriverContext {
                http: &http,
                config: &cfg,
                key: Some(&key),
            })
            .await
            .expect("models");
        assert_eq!(
            models
                .iter()
                .map(|m| m.upstream_id.as_str())
                .collect::<Vec<_>>(),
            vec!["a-model", "b-model"]
        );
        assert_eq!(models[1].context_window, Some(128_000));
        assert_eq!(
            upstream.last_header("authorization").as_deref(),
            Some("Bearer sk-list")
        );
    }

    #[tokio::test]
    async fn catalog_errors_are_classified() {
        let upstream =
            MockUpstream::status("/v1/models", 401, r#"{"error":{"message":"bad key"}}"#).await;
        let cfg = config("m", "openai-compatible", &format!("{}/v1", upstream.base));
        let http = http();
        let err = OpenAiCompatible
            .list_models(DriverContext {
                http: &http,
                config: &cfg,
                key: None,
            })
            .await
            .expect_err("401");
        match err {
            ProviderError::Upstream(f) => {
                assert_eq!(f.kind, loadout_provider_sdk::FailureKind::Auth);
                assert_eq!(f.message, "bad key");
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
