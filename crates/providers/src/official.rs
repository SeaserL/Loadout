// SPDX-License-Identifier: Apache-2.0

//! Official first-party APIs: OpenAI (`https://api.openai.com/v1`, Chat + native Responses
//! with server-side state) and Anthropic (`https://api.anthropic.com/v1`, Messages), plus a
//! local Ollama server. Regular API keys expose no usage endpoint, so usage is estimated
//! from Loadout's request records.

use loadout_protocol::{Protocol, chat, messages};
use loadout_provider_sdk::{
    AuthMethod, BoxFuture, Capabilities, CatalogModel, DriverContext, Endpoint, ProviderConfig,
    ProviderDescriptor, ProviderError, ProviderModule, TransportSpec, TrustLevel,
};
use loadout_secrets::Secret;
use serde_json::Value;

use crate::anthropic_compatible::{anthropic_endpoint, parse_anthropic_models};
use crate::common;

// ---- OpenAI -------------------------------------------------------------------------------

static OPENAI: ProviderDescriptor = ProviderDescriptor {
    kind: "openai",
    display_name: "OpenAI API",
    fixed_slug: Some("openai"),
    default_base_url: Some("https://api.openai.com/v1"),
    auth: AuthMethod::ApiKey,
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
    drivers: common::API_DRIVERS,
    capabilities: Capabilities::STREAMING
        .union(Capabilities::MODEL_DISCOVERY)
        .union(Capabilities::RESPONSES_STATEFUL),
    default_model_protocols: &[Protocol::Chat, Protocol::Responses],
};

/// Whether an OpenAI model id is a text/chat model (the catalog also lists embeddings,
/// audio, image and moderation models).
pub(crate) fn is_openai_chat_model(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    let chat_family = ["gpt-", "o1", "o3", "o4", "chatgpt-", "codex-"]
        .iter()
        .any(|p| id.starts_with(p));
    let excluded = [
        "embedding",
        "tts",
        "whisper",
        "transcribe",
        "dall-e",
        "image",
        "audio",
        "realtime",
        "moderation",
        "search",
    ]
    .iter()
    .any(|x| id.contains(x));
    chat_family && !excluded
}

#[derive(Debug, Default)]
pub struct OpenAi;

impl ProviderModule for OpenAi {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        &OPENAI
    }

    fn validate(&self, config: &ProviderConfig) -> Result<(), ProviderError> {
        common::validate(&OPENAI, config)
    }

    fn endpoint(
        &self,
        config: &ProviderConfig,
        protocol: Protocol,
        key: Option<&Secret>,
    ) -> Result<Endpoint, ProviderError> {
        if OPENAI.transport(protocol).is_none() {
            return Err(ProviderError::NotSupported("this protocol"));
        }
        common::bearer_endpoint(&config.base_url(&OPENAI)?, protocol.upstream_path(), key)
    }

    fn chat_options(&self, _config: &ProviderConfig) -> chat::EncodeOptions {
        // Reasoning models reject `max_tokens`.
        chat::EncodeOptions {
            max_tokens_field: chat::MaxTokensField::MaxCompletionTokens,
        }
    }

    fn list_models<'a>(
        &'a self,
        ctx: DriverContext<'a>,
    ) -> BoxFuture<'a, Result<Vec<CatalogModel>, ProviderError>> {
        Box::pin(async move {
            let base = ctx.config.base_url(&OPENAI)?;
            let mut models = common::list_openai_models(&ctx, &base, |_, _| {
                vec![Protocol::Chat, Protocol::Responses]
            })
            .await?;
            models.retain(|m| is_openai_chat_model(&m.upstream_id));
            Ok(models)
        })
    }
}

// ---- Anthropic ----------------------------------------------------------------------------

static ANTHROPIC: ProviderDescriptor = ProviderDescriptor {
    kind: "anthropic",
    display_name: "Anthropic API",
    fixed_slug: Some("anthropic"),
    default_base_url: Some("https://api.anthropic.com/v1"),
    auth: AuthMethod::ApiKey,
    transports: &[TransportSpec {
        protocol: Protocol::Messages,
        trust: TrustLevel::OfficialApi,
    }],
    drivers: common::API_DRIVERS,
    capabilities: Capabilities::STREAMING.union(Capabilities::MODEL_DISCOVERY),
    default_model_protocols: &[Protocol::Messages],
};

#[derive(Debug, Default)]
pub struct Anthropic;

impl ProviderModule for Anthropic {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        &ANTHROPIC
    }

    fn validate(&self, config: &ProviderConfig) -> Result<(), ProviderError> {
        common::validate(&ANTHROPIC, config)
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
        anthropic_endpoint(config, &config.base_url(&ANTHROPIC)?, "/messages", key)
    }

    fn messages_options(&self, _config: &ProviderConfig) -> messages::EncodeOptions {
        messages::EncodeOptions {
            default_max_tokens: 16_384,
        }
    }

    fn list_models<'a>(
        &'a self,
        ctx: DriverContext<'a>,
    ) -> BoxFuture<'a, Result<Vec<CatalogModel>, ProviderError>> {
        Box::pin(async move {
            let base = ctx.config.base_url(&ANTHROPIC)?;
            let e = anthropic_endpoint(ctx.config, &base, "/models?limit=1000", ctx.key)?;
            let v: Value = ctx
                .http
                .get_json(&e.url, e.headers, common::CATALOG_TIMEOUT)
                .await?;
            parse_anthropic_models(&v)
        })
    }
}

// ---- Ollama -------------------------------------------------------------------------------

static OLLAMA: ProviderDescriptor = ProviderDescriptor {
    kind: "ollama",
    display_name: "Ollama (local)",
    fixed_slug: Some("ollama"),
    default_base_url: Some("http://127.0.0.1:11434/v1"),
    auth: AuthMethod::OptionalApiKey,
    transports: &[TransportSpec {
        protocol: Protocol::Chat,
        trust: TrustLevel::Local,
    }],
    drivers: common::API_DRIVERS,
    capabilities: Capabilities::STREAMING.union(Capabilities::MODEL_DISCOVERY),
    default_model_protocols: &[Protocol::Chat],
};

#[derive(Debug, Default)]
pub struct Ollama;

impl ProviderModule for Ollama {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        &OLLAMA
    }

    fn validate(&self, config: &ProviderConfig) -> Result<(), ProviderError> {
        common::validate(&OLLAMA, config)
    }

    fn trust(&self, config: &ProviderConfig, protocol: Protocol) -> Option<TrustLevel> {
        OLLAMA.transport(protocol)?;
        // A remote Ollama is still first-party software, but not on this machine.
        let local = reqwest::Url::parse(&config.base_url(&OLLAMA).ok()?)
            .ok()
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
        if protocol != Protocol::Chat {
            return Err(ProviderError::NotSupported("this protocol"));
        }
        common::bearer_endpoint(&config.base_url(&OLLAMA)?, protocol.upstream_path(), key)
    }

    fn list_models<'a>(
        &'a self,
        ctx: DriverContext<'a>,
    ) -> BoxFuture<'a, Result<Vec<CatalogModel>, ProviderError>> {
        Box::pin(async move {
            let base = ctx.config.base_url(&OLLAMA)?;
            common::list_openai_models(&ctx, &base, |_, _| vec![Protocol::Chat]).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::config;

    #[test]
    fn openai_model_filter() {
        for yes in [
            "gpt-5.6",
            "gpt-4o-mini",
            "o3",
            "o4-mini",
            "chatgpt-4o-latest",
            "codex-mini-latest",
        ] {
            assert!(is_openai_chat_model(yes), "{yes}");
        }
        for no in [
            "text-embedding-3-large",
            "tts-1",
            "whisper-1",
            "dall-e-3",
            "gpt-image-1",
            "gpt-4o-realtime-preview",
            "omni-moderation-latest",
            "gpt-4o-search-preview",
            "babbage-002",
        ] {
            assert!(!is_openai_chat_model(no), "{no}");
        }
    }

    #[test]
    fn endpoints_and_options() {
        let key = Secret::new("k");
        let cfg = ProviderConfig {
            base_url: None,
            ..config("openai", "openai", "x")
        };
        assert_eq!(
            OpenAi
                .endpoint(&cfg, Protocol::Responses, Some(&key))
                .expect("e")
                .url,
            "https://api.openai.com/v1/responses"
        );
        assert_eq!(
            OpenAi.chat_options(&cfg).max_tokens_field,
            chat::MaxTokensField::MaxCompletionTokens
        );
        assert!(
            OpenAi
                .descriptor()
                .capabilities
                .contains(Capabilities::RESPONSES_STATEFUL)
        );

        let cfg = ProviderConfig {
            base_url: None,
            ..config("anthropic", "anthropic", "x")
        };
        let e = Anthropic
            .endpoint(&cfg, Protocol::Messages, Some(&key))
            .expect("e");
        assert_eq!(e.url, "https://api.anthropic.com/v1/messages");
        assert_eq!(e.headers["x-api-key"], "k");

        let cfg = ProviderConfig {
            base_url: None,
            ..config("ollama", "ollama", "x")
        };
        assert_eq!(Ollama.trust(&cfg, Protocol::Chat), Some(TrustLevel::Local));
        let e = Ollama.endpoint(&cfg, Protocol::Chat, None).expect("e");
        assert_eq!(e.url, "http://127.0.0.1:11434/v1/chat/completions");
        assert!(e.headers.get("authorization").is_none());
    }
}
