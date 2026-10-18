// SPDX-License-Identifier: Apache-2.0

//! Opt-in bridge to the user's running international WorkBuddy client (ADR-016).

use loadout_protocol::Protocol;
use loadout_provider_sdk::{
    AuthMethod, BoxFuture, Capabilities, CatalogModel, DriverContext, Endpoint, ProviderConfig,
    ProviderDescriptor, ProviderError, ProviderModule, TransportSpec, TrustLevel,
};
use loadout_secrets::Secret;

use crate::{OpenAiCompatible, common, openai_compatible::bearer_endpoint};

static DESCRIPTOR: ProviderDescriptor = ProviderDescriptor {
    kind: "workbuddy-bridge",
    display_name: "WorkBuddy International (local IPC bridge)",
    fixed_slug: Some("workbuddy"),
    default_base_url: Some("http://127.0.0.1:18788/v1"),
    auth: AuthMethod::ApiKey,
    transports: &[TransportSpec {
        protocol: Protocol::Chat,
        trust: TrustLevel::PrivateEndpoint,
    }],
    drivers: common::API_DRIVERS,
    capabilities: Capabilities::STREAMING.union(Capabilities::MODEL_DISCOVERY),
    default_model_protocols: &[Protocol::Chat],
};

#[derive(Debug, Default)]
pub struct WorkBuddyBridge;

impl ProviderModule for WorkBuddyBridge {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        &DESCRIPTOR
    }

    fn validate(&self, config: &ProviderConfig) -> Result<(), ProviderError> {
        let base = config.base_url(&DESCRIPTOR)?;
        let url = reqwest::Url::parse(&base)
            .map_err(|_| ProviderError::Config("invalid bridge URL".into()))?;
        if url.scheme() != "http"
            || url.host_str() != Some("127.0.0.1")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path().trim_end_matches('/') != "/v1"
        {
            return Err(ProviderError::Config(
                "WorkBuddy bridge requires http://127.0.0.1:<port>/v1".into(),
            ));
        }
        Ok(())
    }

    fn account_concurrency(&self) -> Option<u32> {
        Some(1)
    }

    fn endpoint(
        &self,
        config: &ProviderConfig,
        protocol: Protocol,
        key: Option<&Secret>,
    ) -> Result<Endpoint, ProviderError> {
        self.validate(config)?;
        if protocol != Protocol::Chat {
            return Err(ProviderError::NotSupported("bridge supports Chat only"));
        }
        if key.is_none_or(Secret::is_empty) {
            return Err(ProviderError::Config("bridge API key is required".into()));
        }
        bearer_endpoint(
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
            self.validate(ctx.config)?;
            let mut config = ctx.config.clone();
            config.base_url = Some(config.base_url(&DESCRIPTOR)?);
            let models = OpenAiCompatible
                .list_models(DriverContext {
                    config: &config,
                    ..ctx
                })
                .await?;
            Ok(models
                .into_iter()
                .filter(|m| m.upstream_id == "deepseek-v4.1-flash")
                .map(|mut model| {
                    model.capabilities = Capabilities::STREAMING;
                    model.max_output_tokens = Some(8192);
                    model
                })
                .collect())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{MockUpstream, config, http};

    #[tokio::test]
    async fn bridge_keeps_private_trust_and_only_verified_model() {
        let upstream = MockUpstream::json(
            "/v1/models",
            serde_json::json!({"data": [
                {"id": "deepseek-v4.1-flash"}, {"id": "default-model"}
            ]}),
        )
        .await;
        let cfg = config(
            "workbuddy",
            "workbuddy-bridge",
            &format!("{}/v1", upstream.base),
        );
        let key = Secret::new("synthetic-bridge-key");
        let http = http();
        let models = WorkBuddyBridge
            .list_models(DriverContext {
                http: &http,
                config: &cfg,
                key: Some(&key),
                home: None,
            })
            .await
            .expect("bridge model catalog");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].upstream_id, "deepseek-v4.1-flash");
        assert!(!models[0].capabilities.contains(Capabilities::TOOLS));
        assert!(!models[0].capabilities.contains(Capabilities::VISION));
        assert_eq!(models[0].max_output_tokens, Some(8192));
        assert_eq!(
            WorkBuddyBridge.trust(&cfg, Protocol::Chat),
            Some(TrustLevel::PrivateEndpoint)
        );
        assert_eq!(WorkBuddyBridge.trust(&cfg, Protocol::Responses), None);
        assert!(
            WorkBuddyBridge
                .endpoint(&cfg, Protocol::Chat, None)
                .is_err()
        );
        let endpoint = WorkBuddyBridge
            .endpoint(&cfg, Protocol::Chat, Some(&key))
            .expect("authenticated endpoint");
        assert!(!format!("{endpoint:?}").contains("synthetic-bridge-key"));
    }

    #[test]
    fn bridge_rejects_remote_urls_and_url_credentials() {
        for base in [
            "https://example.test/v1",
            "http://localhost/v1",
            "http://127.0.0.1.evil.test/v1",
            "http://key@127.0.0.1/v1",
            "http://127.0.0.1/v1?key=x",
        ] {
            assert!(
                WorkBuddyBridge
                    .validate(&config("workbuddy", "workbuddy-bridge", base))
                    .is_err()
            );
        }
    }
}
