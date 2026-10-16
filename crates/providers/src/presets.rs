// SPDX-License-Identifier: Apache-2.0

//! Built-in API-key platforms that speak OpenAI Chat Completions (and some also Anthropic
//! Messages) with a Bearer key: one table row each instead of one module each (ExecPlan 018).
//!
//! A row fixes the slug, the default base URL (the part before `/chat/completions`), the
//! origin-relative Messages path when the platform offers one, the catalog path, a documented
//! model list for platforms whose catalog endpoint is missing or undocumented, and an optional
//! balance endpoint read with the same key. Platform quirks stay in this file; nothing outside
//! the providers crate sees them (ADR-005). Details per platform: `docs/providers/presets.md`.

use loadout_protocol::{Protocol, chat};
use loadout_provider_sdk::time::number_field;
use loadout_provider_sdk::usage::{Balance, Confidence, UsageReport, UsageSource};
use loadout_provider_sdk::{
    AuthMethod, BoxFuture, Capabilities, CatalogModel, DriverContext, Drivers, Endpoint,
    ProviderConfig, ProviderDescriptor, ProviderError, ProviderModule, TransportSpec, TrustLevel,
};
use loadout_secrets::Secret;
use serde_json::Value;

use crate::common::{self, ANTHROPIC_VERSION, origin_join};

const CHAT: &[TransportSpec] = &[TransportSpec {
    protocol: Protocol::Chat,
    trust: TrustLevel::OfficialApi,
}];

const CHAT_AND_MESSAGES: &[TransportSpec] = &[
    TransportSpec {
        protocol: Protocol::Chat,
        trust: TrustLevel::OfficialApi,
    },
    TransportSpec {
        protocol: Protocol::Messages,
        trust: TrustLevel::OfficialApi,
    },
];

const fn descriptor(
    kind: &'static str,
    display_name: &'static str,
    base_url: &'static str,
    messages: bool,
    balance: bool,
) -> ProviderDescriptor {
    let mut capabilities = Capabilities::STREAMING.union(Capabilities::MODEL_DISCOVERY);
    if balance {
        capabilities = capabilities.union(Capabilities::BALANCE_QUERY);
    }
    ProviderDescriptor {
        kind,
        display_name,
        fixed_slug: Some(kind),
        default_base_url: Some(base_url),
        auth: AuthMethod::ApiKey,
        transports: if messages { CHAT_AND_MESSAGES } else { CHAT },
        drivers: Drivers {
            usage: balance,
            ..common::API_DRIVERS
        },
        capabilities,
        default_model_protocols: if messages {
            &[Protocol::Chat, Protocol::Messages]
        } else {
            &[Protocol::Chat]
        },
    }
}

/// A balance endpoint read with the API key.
struct BalanceApi {
    /// Origin-relative path, e.g. `/v1/users/me/balance`.
    path: &'static str,
    parse: fn(&Value) -> Result<UsageReport, ProviderError>,
}

/// One built-in platform.
pub(crate) struct ApiPreset {
    descriptor: ProviderDescriptor,
    /// Origin-relative Anthropic Messages endpoint, e.g. `/anthropic/v1/messages`.
    messages_path: Option<&'static str>,
    /// Catalog path under the base URL.
    models_path: &'static str,
    /// Documented model ids, used when the catalog endpoint is missing or lists nothing.
    known_models: &'static [&'static str],
    max_tokens_field: chat::MaxTokensField,
    /// Chat Completions models that take `max_completion_tokens` although the platform
    /// default is `max_tokens` (id prefixes).
    max_completion_tokens_models: &'static [&'static str],
    balance: Option<BalanceApi>,
}

const fn preset(descriptor: ProviderDescriptor, messages_path: Option<&'static str>) -> ApiPreset {
    ApiPreset {
        descriptor,
        messages_path,
        models_path: "/models",
        known_models: &[],
        max_tokens_field: chat::MaxTokensField::MaxTokens,
        max_completion_tokens_models: &[],
        balance: None,
    }
}

pub(crate) static PRESETS: [ApiPreset; 14] = [
    // ---- China ----
    ApiPreset {
        known_models: &[
            "sensenova-6.8-flash-lite",
            "deepseek-v4-flash",
            "deepseek-flash",
            "glm-5.2",
            "kimi-k3",
        ],
        max_completion_tokens_models: &["kimi-k3"],
        ..preset(
            descriptor(
                "sensenova",
                "SenseNova (商汤日日新)",
                "https://token.sensenova.cn/v1",
                true,
                false,
            ),
            Some("/v1/messages"),
        )
    },
    ApiPreset {
        balance: Some(BalanceApi {
            path: "/v1/users/me/balance",
            parse: parse_moonshot_balance,
        }),
        ..preset(
            descriptor(
                "moonshot",
                "Moonshot AI (Kimi API)",
                "https://api.moonshot.cn/v1",
                true,
                true,
            ),
            Some("/anthropic/v1/messages"),
        )
    },
    ApiPreset {
        known_models: &["glm-5.3", "glm-5.2", "glm-5.1", "glm-4.7-flash"],
        ..preset(
            descriptor(
                "zhipu",
                "Zhipu BigModel (智谱 GLM)",
                "https://open.bigmodel.cn/api/paas/v4",
                true,
                false,
            ),
            Some("/api/anthropic/v1/messages"),
        )
    },
    ApiPreset {
        known_models: &["qwen3.8-max", "qwen3.7-plus", "qwen3.8-flash"],
        ..preset(
            descriptor(
                "dashscope",
                "Alibaba Bailian (阿里云百炼 Qwen)",
                "https://dashscope.aliyuncs.com/compatible-mode/v1",
                true,
                false,
            ),
            Some("/apps/anthropic/v1/messages"),
        )
    },
    preset(
        descriptor(
            "volcengine",
            "Volcengine Ark (火山方舟 Doubao)",
            "https://ark.cn-beijing.volces.com/api/v3",
            true,
            false,
        ),
        Some("/api/compatible/v1/messages"),
    ),
    ApiPreset {
        models_path: "/models?sub_type=chat",
        ..preset(
            descriptor(
                "siliconflow",
                "SiliconFlow (硅基流动)",
                "https://api.siliconflow.cn/v1",
                true,
                false,
            ),
            Some("/v1/messages"),
        )
    },
    preset(
        descriptor(
            "minimax",
            "MiniMax",
            "https://api.minimaxi.com/v1",
            true,
            false,
        ),
        Some("/anthropic/v1/messages"),
    ),
    preset(
        descriptor(
            "stepfun",
            "StepFun (阶跃星辰)",
            "https://api.stepfun.com/v1",
            true,
            false,
        ),
        Some("/v1/messages"),
    ),
    preset(
        descriptor(
            "qianfan",
            "Baidu Qianfan (百度千帆)",
            "https://qianfan.baidubce.com/v2",
            true,
            false,
        ),
        Some("/anthropic/v1/messages"),
    ),
    // ---- Global ----
    ApiPreset {
        max_tokens_field: chat::MaxTokensField::MaxCompletionTokens,
        ..preset(
            descriptor("xai", "xAI (Grok)", "https://api.x.ai/v1", false, false),
            None,
        )
    },
    preset(
        descriptor(
            "gemini",
            "Google Gemini API",
            "https://generativelanguage.googleapis.com/v1beta/openai",
            false,
            false,
        ),
        None,
    ),
    preset(
        descriptor(
            "mistral",
            "Mistral AI",
            "https://api.mistral.ai/v1",
            false,
            false,
        ),
        None,
    ),
    preset(
        descriptor(
            "groq",
            "Groq",
            "https://api.groq.com/openai/v1",
            false,
            false,
        ),
        None,
    ),
    preset(
        descriptor(
            "cerebras",
            "Cerebras",
            "https://api.cerebras.ai/v1",
            false,
            false,
        ),
        None,
    ),
];

/// Catalog ids that are clearly not chat models (embeddings, speech, images, safety
/// classifiers…); catalogs of these platforms mix them in.
fn is_chat_model(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    ![
        "embed",
        "rerank",
        "tts",
        "speech",
        "whisper",
        "asr",
        "audio",
        "transcribe",
        "image",
        "imagen",
        "video",
        "veo",
        "dall-e",
        "seedream",
        "seedance",
        "wanx",
        "wan2",
        "cogview",
        "cogvideo",
        "flux",
        "stable-diffusion",
        "paraformer",
        "cosyvoice",
        "sambert",
        "moderation",
        "guard",
        "ocr",
        "realtime",
        "aqa",
        "sensenova-u1",
    ]
    .iter()
    .any(|x| id.contains(x))
}

/// Whether a catalog entry's own metadata says it is not a usable chat model: text output
/// missing (`output_modalities`), chat completion unsupported (`capabilities`), or retired
/// (`active: false`).
fn entry_excluded(entry: &Value) -> bool {
    let no_text = entry
        .get("output_modalities")
        .and_then(Value::as_array)
        .is_some_and(|a| !a.iter().any(|m| m == "text"));
    let no_chat = entry
        .get("capabilities")
        .and_then(|c| c.get("completion_chat"))
        .and_then(Value::as_bool)
        == Some(false);
    let inactive = entry.get("active").and_then(Value::as_bool) == Some(false);
    no_text || no_chat || inactive
}

fn catalog_missing(e: &ProviderError) -> bool {
    match e {
        ProviderError::InvalidResponse(_) => true,
        ProviderError::Upstream(f) => matches!(f.status, Some(404 | 405 | 501)),
        _ => false,
    }
}

/// `GET /v1/users/me/balance` → `{"data":{"available_balance","voucher_balance","cash_balance"}}`
/// in CNY (no currency field).
pub(crate) fn parse_moonshot_balance(v: &Value) -> Result<UsageReport, ProviderError> {
    let d = v
        .get("data")
        .filter(|d| d.is_object())
        .ok_or_else(|| ProviderError::InvalidResponse("balance response has no `data`".into()))?;
    let amount = number_field(d, &["available_balance"]).ok_or_else(|| {
        ProviderError::InvalidResponse("balance response has no available_balance".into())
    })?;
    let mut report = UsageReport::new(UsageSource::OfficialApi, Confidence::Exact);
    report.balances.push(Balance {
        currency: "CNY".into(),
        amount,
        included: number_field(d, &["voucher_balance"]),
        topup: number_field(d, &["cash_balance"]),
        granted: None,
    });
    Ok(report)
}

/// Registry handle for one row of [`PRESETS`].
#[derive(Clone, Copy)]
pub struct Preset(&'static ApiPreset);

impl std::fmt::Debug for Preset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Preset")
            .field(&self.0.descriptor.kind)
            .finish()
    }
}

impl Preset {
    /// Every built-in preset.
    pub fn all() -> impl Iterator<Item = Self> {
        PRESETS.iter().map(Self)
    }
}

impl ProviderModule for Preset {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        &self.0.descriptor
    }

    fn validate(&self, config: &ProviderConfig) -> Result<(), ProviderError> {
        common::validate(&self.0.descriptor, config)
    }

    fn endpoint(
        &self,
        config: &ProviderConfig,
        protocol: Protocol,
        key: Option<&Secret>,
    ) -> Result<Endpoint, ProviderError> {
        let base = config.base_url(&self.0.descriptor)?;
        match (protocol, self.0.messages_path) {
            (Protocol::Chat, _) => common::bearer_endpoint(&base, protocol.upstream_path(), key),
            // Platforms document either `x-api-key` or Bearer here; send both.
            (Protocol::Messages, Some(path)) => {
                let mut e = Endpoint::new(origin_join(&base, path)?)
                    .header("anthropic-version", ANTHROPIC_VERSION)?;
                if let Some(k) = key.filter(|k| !k.is_empty()) {
                    e = e.secret_header("x-api-key", "", k)?.secret_header(
                        "authorization",
                        "Bearer ",
                        k,
                    )?;
                }
                Ok(e)
            }
            _ => Err(ProviderError::NotSupported("this protocol")),
        }
    }

    fn prepare_request(&self, _config: &ProviderConfig, protocol: Protocol, body: &mut Value) {
        if protocol != Protocol::Chat || self.0.max_completion_tokens_models.is_empty() {
            return;
        }
        let Some(obj) = body.as_object_mut() else {
            return;
        };
        let wants = obj.get("model").and_then(Value::as_str).is_some_and(|m| {
            self.0
                .max_completion_tokens_models
                .iter()
                .any(|p| m.starts_with(p))
        });
        if wants
            && !obj.contains_key("max_completion_tokens")
            && let Some(n) = obj.remove("max_tokens")
        {
            obj.insert("max_completion_tokens".into(), n);
        }
    }

    fn chat_options(&self, _config: &ProviderConfig) -> chat::EncodeOptions {
        chat::EncodeOptions {
            max_tokens_field: self.0.max_tokens_field,
        }
    }

    fn list_models<'a>(
        &'a self,
        ctx: DriverContext<'a>,
    ) -> BoxFuture<'a, Result<Vec<CatalogModel>, ProviderError>> {
        Box::pin(async move {
            let p = self.0;
            let base = ctx.config.base_url(&p.descriptor)?;
            let protocols = p.descriptor.default_model_protocols;
            let listed = common::list_openai_models_at(&ctx, &base, p.models_path, |_, entry| {
                if entry_excluded(entry) {
                    Vec::new()
                } else {
                    protocols.to_vec()
                }
            });
            let mut models = match listed.await {
                Ok(models) => models,
                // Only a missing catalog falls back; a rejected key or an unreachable host
                // must surface.
                Err(e) if p.known_models.is_empty() || !catalog_missing(&e) => return Err(e),
                Err(_) => Vec::new(),
            };
            models.retain(|m| !m.protocols.is_empty() && is_chat_model(&m.upstream_id));
            for m in &mut models {
                // Gemini lists `models/gemini-…`; requests take the bare id.
                if let Some(id) = m.upstream_id.strip_prefix("models/") {
                    m.upstream_id = id.to_owned();
                }
            }
            models.sort_by(|a, b| a.upstream_id.cmp(&b.upstream_id));
            if models.is_empty() {
                models = p
                    .known_models
                    .iter()
                    .map(|id| {
                        let mut m = CatalogModel::new(*id);
                        m.protocols = protocols.to_vec();
                        m
                    })
                    .collect();
            }
            Ok(models)
        })
    }

    fn fetch_usage<'a>(
        &'a self,
        ctx: DriverContext<'a>,
    ) -> BoxFuture<'a, Result<UsageReport, ProviderError>> {
        Box::pin(async move {
            let balance = self
                .0
                .balance
                .as_ref()
                .ok_or(ProviderError::NotSupported("usage"))?;
            let key = ctx.key.ok_or(ProviderError::MissingKey)?;
            let url = origin_join(&ctx.config.base_url(&self.0.descriptor)?, balance.path)?;
            let e = common::bearer_endpoint(&url, "", Some(key))?;
            let v = ctx
                .http
                .get_json(
                    e.url.trim_end_matches('/'),
                    e.headers,
                    common::USAGE_TIMEOUT,
                )
                .await?;
            (balance.parse)(&v)
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::testutil::{MockUpstream, config, http};

    fn get(kind: &str) -> Preset {
        Preset::all()
            .find(|p| p.descriptor().kind == kind)
            .expect("preset")
    }

    fn defaults(kind: &str) -> ProviderConfig {
        ProviderConfig {
            base_url: None,
            ..config(kind, kind, "x")
        }
    }

    #[test]
    fn every_preset_is_consistent() {
        let key = Secret::new("sk-test");
        let mut kinds = std::collections::HashSet::new();
        for p in Preset::all() {
            let d = p.descriptor();
            assert!(kinds.insert(d.kind), "duplicate {}", d.kind);
            assert_eq!(d.fixed_slug, Some(d.kind));
            assert!(d.kind.bytes().all(|b| b.is_ascii_lowercase()), "{}", d.kind);
            let cfg = defaults(d.kind);
            p.validate(&cfg).expect("default base URL is valid");
            let base = d.default_base_url.expect("base");
            assert!(
                base.starts_with("https://") && !base.ends_with('/'),
                "{base}"
            );

            let chat = p.endpoint(&cfg, Protocol::Chat, Some(&key)).expect("chat");
            assert_eq!(chat.url, format!("{base}/chat/completions"));
            assert_eq!(chat.headers["authorization"], "Bearer sk-test");

            let messages = p.endpoint(&cfg, Protocol::Messages, Some(&key));
            assert_eq!(
                messages.is_ok(),
                d.transport(Protocol::Messages).is_some(),
                "{}",
                d.kind
            );
            if let Ok(m) = messages {
                assert!(m.url.ends_with("/v1/messages"), "{}", m.url);
                assert_eq!(m.headers["x-api-key"], "sk-test");
                assert_eq!(m.headers["authorization"], "Bearer sk-test");
            }
            assert!(p.endpoint(&cfg, Protocol::Responses, Some(&key)).is_err());
            assert_eq!(
                p.0.balance.is_some(),
                d.capabilities.contains(Capabilities::BALANCE_QUERY)
            );
            assert_eq!(p.0.balance.is_some(), d.drivers.usage);
        }
    }

    #[test]
    fn sensenova_endpoints() {
        let p = get("sensenova");
        let key = Secret::new("sk-sn");
        let cfg = defaults("sensenova");
        assert_eq!(
            p.endpoint(&cfg, Protocol::Chat, Some(&key)).expect("e").url,
            "https://token.sensenova.cn/v1/chat/completions"
        );
        assert_eq!(
            p.endpoint(&cfg, Protocol::Messages, Some(&key))
                .expect("e")
                .url,
            "https://token.sensenova.cn/v1/messages"
        );
        // A custom base URL moves Messages with it.
        let cfg = config("sensenova", "sensenova", "https://sn.example.com/v1");
        assert_eq!(
            p.endpoint(&cfg, Protocol::Messages, None).expect("e").url,
            "https://sn.example.com/v1/messages"
        );
    }

    #[test]
    fn sensenova_kimi_takes_max_completion_tokens() {
        let p = get("sensenova");
        let cfg = defaults("sensenova");
        let mut body = json!({"model": "kimi-k3", "max_tokens": 9});
        p.prepare_request(&cfg, Protocol::Chat, &mut body);
        assert_eq!(
            body,
            json!({"model": "kimi-k3", "max_completion_tokens": 9})
        );
        let mut body = json!({"model": "glm-5.2", "max_tokens": 9});
        p.prepare_request(&cfg, Protocol::Chat, &mut body);
        assert_eq!(body["max_tokens"], 9);
        let mut body = json!({"model": "kimi-k3", "max_tokens": 9});
        p.prepare_request(&cfg, Protocol::Messages, &mut body);
        assert_eq!(body["max_tokens"], 9);
    }

    #[test]
    fn chat_model_filter() {
        for yes in [
            "sensenova-6.8-flash-lite",
            "kimi-k3",
            "glm-5.2",
            "qwen3.8-max",
            "grok-4",
            "gemini-3-pro",
            "mistral-large-latest",
            "llama-3.3-70b-versatile",
        ] {
            assert!(is_chat_model(yes), "{yes}");
        }
        for no in [
            "sensenova-u1.5-lite",
            "text-embedding-v4",
            "BAAI/bge-reranker-v2-m3",
            "whisper-large-v3",
            "llama-guard-4-12b",
            "grok-imagine-image",
            "imagen-4.0-generate-001",
            "mistral-ocr-latest",
            "cosyvoice-v2",
        ] {
            assert!(!is_chat_model(no), "{no}");
        }
        assert!(entry_excluded(
            &json!({"id": "u", "output_modalities": ["image"]})
        ));
        assert!(entry_excluded(
            &json!({"id": "m", "capabilities": {"completion_chat": false}})
        ));
        assert!(entry_excluded(&json!({"id": "g", "active": false})));
        assert!(!entry_excluded(
            &json!({"id": "s", "output_modalities": ["text"], "active": true})
        ));
    }

    #[tokio::test]
    async fn catalog_filters_and_strips_prefix() {
        let up = MockUpstream::json(
            "/v1/models",
            json!({"data": [
                {"id": "sensenova-6.8-flash-lite", "context_length": 262144, "output_modalities": ["text"]},
                {"id": "sensenova-u1.5-fast", "output_modalities": ["image"]},
                {"id": "models/gemini-3-pro"},
                {"id": "text-embedding-3"}
            ]}),
        )
        .await;
        let cfg = config("sensenova", "sensenova", &format!("{}/v1", up.base));
        let key = Secret::new("sk-sn");
        let http = http();
        let models = get("sensenova")
            .list_models(DriverContext {
                http: &http,
                config: &cfg,
                key: Some(&key),
                home: None,
            })
            .await
            .expect("models");
        let ids: Vec<_> = models.iter().map(|m| m.upstream_id.as_str()).collect();
        assert_eq!(ids, ["gemini-3-pro", "sensenova-6.8-flash-lite"]);
        assert_eq!(models[1].context_window, Some(262_144));
        assert_eq!(models[1].protocols, [Protocol::Chat, Protocol::Messages]);
        assert_eq!(
            up.last_header("authorization").as_deref(),
            Some("Bearer sk-sn")
        );
    }

    #[tokio::test]
    async fn missing_catalog_falls_back_but_bad_key_does_not() {
        let http = http();
        let key = Secret::new("k");
        let ctx = |cfg| DriverContext {
            http: &http,
            config: cfg,
            key: Some(&key),
            home: None,
        };

        let up = MockUpstream::status("/elsewhere", 200, "{}").await;
        let cfg = config("zhipu", "zhipu", &format!("{}/api/paas/v4", up.base));
        let models = get("zhipu").list_models(ctx(&cfg)).await.expect("fallback");
        assert!(models.iter().any(|m| m.upstream_id == "glm-5.2"));

        let up = MockUpstream::status("/api/paas/v4/models", 401, r#"{"error":{}}"#).await;
        let cfg = config("zhipu", "zhipu", &format!("{}/api/paas/v4", up.base));
        assert!(get("zhipu").list_models(ctx(&cfg)).await.is_err());

        // Without a documented list a missing catalog is an error, not an empty account.
        let up = MockUpstream::status("/elsewhere", 200, "{}").await;
        let cfg = config("minimax", "minimax", &format!("{}/v1", up.base));
        assert!(get("minimax").list_models(ctx(&cfg)).await.is_err());
    }

    #[tokio::test]
    async fn siliconflow_asks_for_chat_models_only() {
        let up = MockUpstream::json("/v1/models", json!({"data": [{"id": "Qwen/Qwen3-8B"}]})).await;
        let cfg = config("siliconflow", "siliconflow", &format!("{}/v1", up.base));
        let key = Secret::new("k");
        let http = http();
        let models = get("siliconflow")
            .list_models(DriverContext {
                http: &http,
                config: &cfg,
                key: Some(&key),
                home: None,
            })
            .await
            .expect("models");
        assert_eq!(models[0].upstream_id, "Qwen/Qwen3-8B");
    }

    #[test]
    fn parses_moonshot_balance() {
        let v = json!({"code": 0, "data": {"available_balance": 49.58894, "voucher_balance": 46.58893, "cash_balance": 3.00001}, "scode": "0x0", "status": true});
        let r = parse_moonshot_balance(&v).expect("balance");
        let b = &r.balances[0];
        assert_eq!(b.currency, "CNY");
        assert_eq!(b.amount, 49.58894);
        assert_eq!(b.included, Some(46.58893));
        assert_eq!(b.topup, Some(3.00001));
        assert!(parse_moonshot_balance(&json!({"code": 1})).is_err());
    }

    #[tokio::test]
    async fn moonshot_balance_uses_the_origin() {
        let up = MockUpstream::json(
            "/v1/users/me/balance",
            json!({"data": {"available_balance": 1.5}}),
        )
        .await;
        let cfg = config("moonshot", "moonshot", &format!("{}/v1", up.base));
        let key = Secret::new("sk-ms");
        let http = http();
        let r = get("moonshot")
            .fetch_usage(DriverContext {
                http: &http,
                config: &cfg,
                key: Some(&key),
                home: None,
            })
            .await
            .expect("usage");
        assert_eq!(r.balances[0].amount, 1.5);
        let groq = defaults("groq");
        let err = get("groq")
            .fetch_usage(DriverContext {
                http: &http,
                config: &groq,
                key: Some(&key),
                home: None,
            })
            .await
            .expect_err("no balance API");
        assert!(matches!(err, ProviderError::NotSupported(_)));
    }
}
