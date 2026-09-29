// SPDX-License-Identifier: Apache-2.0

//! Command Code Provider API (`https://api.commandcode.ai/provider/v1`, docs:
//! commandcode.ai/docs/provider). Every model lists the routes serving it in
//! `supported_endpoints`. There is no usage endpoint for API keys (the web dashboard needs
//! browser cookies, which ADR-009 rules out), so usage is estimated from Loadout's own
//! request records.

use loadout_protocol::Protocol;
use loadout_provider_sdk::{
    AuthMethod, BoxFuture, Capabilities, CatalogModel, DriverContext, Endpoint, ProviderConfig,
    ProviderDescriptor, ProviderError, ProviderModule, TransportSpec, TrustLevel,
};
use loadout_secrets::Secret;
use serde_json::Value;

use crate::common;

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
    drivers: common::API_DRIVERS,
    capabilities: Capabilities::STREAMING.union(Capabilities::MODEL_DISCOVERY),
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
}
