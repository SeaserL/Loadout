// SPDX-License-Identifier: Apache-2.0

//! Kimi Code (docs: kimi.com/code/docs). Kimi members create API keys in the Kimi Code
//! console for third-party tools; one base URL serves Chat (`/chat/completions`) and
//! Messages (`/messages`). Region: `https://api.kimi.ai/coding/v1` (international,
//! default) or `https://api.kimi.com/coding/v1` (setting `region = "china"`).
//!
//! Usage: `GET {base}/usages`, authenticated with the API key or — for accounts imported
//! from the official Kimi Code CLI — the CLI's current access token, read-only from
//! `~/.kimi-code/credentials/kimi-code.json` and never refreshed (ADR-009).

use std::path::Path;

use loadout_protocol::Protocol;
use loadout_provider_sdk::time::{now_ms, number, timestamp_ms};
use loadout_provider_sdk::usage::{Confidence, Plan, UsageReport, UsageSource};
use loadout_provider_sdk::{
    AuthMethod, BoxFuture, Capabilities, CatalogModel, DriverContext, Endpoint, ProviderConfig,
    ProviderDescriptor, ProviderError, ProviderModule, TransportSpec, TrustLevel, UpstreamFailure,
};
use loadout_secrets::Secret;
use serde_json::Value;

use crate::common::{self, FIVE_HOURS, WEEK, window};

const INTERNATIONAL: &str = "https://api.kimi.ai/coding/v1";
const CHINA: &str = "https://api.kimi.com/coding/v1";
/// Documented models (fallback when `/models` is unavailable).
const KNOWN_MODELS: [&str; 4] = [
    "k3",
    "k3-256k",
    "kimi-for-coding",
    "kimi-for-coding-highspeed",
];

static DESCRIPTOR: ProviderDescriptor = ProviderDescriptor {
    kind: "kimi-code",
    display_name: "Kimi Code",
    fixed_slug: Some("kimi-code"),
    default_base_url: Some(INTERNATIONAL),
    auth: AuthMethod::ApiKey,
    transports: &[
        TransportSpec {
            protocol: Protocol::Chat,
            trust: TrustLevel::OfficialSubscriptionApi,
        },
        TransportSpec {
            protocol: Protocol::Messages,
            trust: TrustLevel::OfficialSubscriptionApi,
        },
    ],
    drivers: loadout_provider_sdk::Drivers {
        discovery: true,
        ..common::API_DRIVERS_WITH_USAGE
    },
    capabilities: Capabilities::STREAMING
        .union(Capabilities::MODEL_DISCOVERY)
        .union(Capabilities::USAGE_QUERY)
        .union(Capabilities::REASONING),
    default_model_protocols: &[Protocol::Chat, Protocol::Messages],
};

fn base(config: &ProviderConfig) -> String {
    config.base_url.clone().unwrap_or_else(|| {
        if config.setting_str("region") == Some("china") {
            CHINA.to_owned()
        } else {
            INTERNATIONAL.to_owned()
        }
    })
}

pub(crate) fn credentials_path(home: &Path) -> std::path::PathBuf {
    home.join(".kimi-code")
        .join("credentials")
        .join("kimi-code.json")
}

/// The Kimi Code CLI's current access token, if present and not expired.
pub(crate) fn cli_token(home: &Path) -> Result<Secret, ProviderError> {
    let path = credentials_path(home);
    let text = std::fs::read_to_string(&path).map_err(|_| {
        ProviderError::Config("Kimi Code CLI is not logged in (no credentials file)".into())
    })?;
    let v: Value = serde_json::from_str(&text).map_err(|_| {
        ProviderError::InvalidResponse("unreadable Kimi Code CLI credentials".into())
    })?;
    let token = ["access_token", "accessToken"]
        .iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str))
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            ProviderError::InvalidResponse("Kimi Code CLI credentials have no access token".into())
        })?;
    let expires = ["expires_at", "expiresAt", "expiry"]
        .iter()
        .find_map(|k| v.get(*k).and_then(timestamp_ms));
    if expires.is_some_and(|e| e <= now_ms()) {
        return Err(ProviderError::Upstream(UpstreamFailure::new(
            loadout_provider_sdk::FailureKind::Auth,
            None,
            "the Kimi Code CLI login expired; run `kimi` once to refresh it",
        )));
    }
    Ok(Secret::new(token))
}

pub(crate) fn parse_usage(v: &Value) -> Result<UsageReport, ProviderError> {
    let mut report = UsageReport::new(UsageSource::OfficialApi, Confidence::Exact);
    // Weekly quota: `usage` (or the FEATURE_CODING entry of `usages`).
    let weekly = v.get("usage").filter(|u| u.is_object()).or_else(|| {
        v.get("usages")
            .and_then(Value::as_array)
            .and_then(|a| {
                a.iter()
                    .find(|u| u.get("scope").and_then(Value::as_str) == Some("FEATURE_CODING"))
            })
            .and_then(|u| u.get("detail"))
    });
    if let Some(w) = weekly.and_then(|w| window("weekly", "Weekly", Some(WEEK), w)) {
        report.windows.push(w);
    }
    // Rolling limit: `limits[i].detail` with `limits[i].window`.
    if let Some(limits) = v.get("limits").and_then(Value::as_array) {
        for l in limits {
            let detail = l.get("detail").unwrap_or(l);
            let minutes = l.get("window").and_then(|w| {
                w.get("duration")
                    .and_then(number)
                    .or_else(|| w.get("minutes").and_then(number))
            });
            let secs = minutes.map(|m| (m * 60.0) as u64).unwrap_or(FIVE_HOURS);
            let (scope, label) = if secs == FIVE_HOURS {
                ("5h".to_owned(), "5h".to_owned())
            } else {
                (format!("{}m", secs / 60), format!("{}m", secs / 60))
            };
            if let Some(w) = window(&scope, &label, Some(secs), detail) {
                report.windows.push(w);
            }
        }
    }
    if report.windows.is_empty() {
        return Err(ProviderError::InvalidResponse(
            "Kimi Code usage has no windows".into(),
        ));
    }
    report
        .windows
        .sort_by_key(|w| w.window_seconds.unwrap_or(0));
    if let Some(name) = v.get("planName").and_then(Value::as_str) {
        report.plan = Some(Plan {
            name: name.to_owned(),
            price_usd_monthly: None,
            renews_at_ms: None,
            auto_renew: None,
        });
    }
    Ok(report)
}

#[derive(Debug, Default)]
pub struct KimiCode;

impl ProviderModule for KimiCode {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        &DESCRIPTOR
    }

    fn validate(&self, config: &ProviderConfig) -> Result<(), ProviderError> {
        match config.setting_str("region") {
            None | Some("china" | "international") => {}
            Some(other) => {
                return Err(ProviderError::Config(format!(
                    "region must be china or international, not {other:?}"
                )));
            }
        }
        loadout_provider_sdk::validate_base_url(&base(config), false).map(|_| ())
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
        common::bearer_all(&base(config), protocol, key)
    }

    fn list_models<'a>(
        &'a self,
        ctx: DriverContext<'a>,
    ) -> BoxFuture<'a, Result<Vec<CatalogModel>, ProviderError>> {
        Box::pin(async move {
            let protocols = vec![Protocol::Chat, Protocol::Messages];
            match common::list_openai_models(&ctx, &base(ctx.config), |_, _| protocols.clone())
                .await
            {
                Ok(models) if !models.is_empty() => Ok(models),
                Err(ProviderError::Upstream(f))
                    if f.kind == loadout_provider_sdk::FailureKind::Auth =>
                {
                    Err(ProviderError::Upstream(f))
                }
                _ => Ok(KNOWN_MODELS
                    .iter()
                    .map(|id| {
                        let mut m = CatalogModel::new(*id);
                        m.protocols = protocols.clone();
                        m
                    })
                    .collect()),
            }
        })
    }

    fn fetch_usage<'a>(
        &'a self,
        ctx: DriverContext<'a>,
    ) -> BoxFuture<'a, Result<UsageReport, ProviderError>> {
        Box::pin(async move {
            let (token, source) = match (ctx.key, ctx.home) {
                (Some(k), _) => (Secret::new(k.expose()), UsageSource::OfficialApi),
                (None, Some(home)) => (cli_token(home)?, UsageSource::OfficialCli),
                (None, None) => return Err(ProviderError::MissingKey),
            };
            let v = common::get_bearer(
                &ctx,
                &base(ctx.config),
                "/usages",
                Some(&token),
                common::USAGE_TIMEOUT,
            )
            .await?;
            let mut report = parse_usage(&v)?;
            report.source = source;
            Ok(report)
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::testutil::{MockUpstream, config, http};

    #[test]
    fn parses_code_api_usage() {
        let v = json!({
            "usage": {"limit": "2048", "used": "512", "remaining": "1536", "resetTime": "2026-10-05T00:00:00Z"},
            "limits": [{"window": {"duration": 300, "timeUnit": "TIME_UNIT_MINUTE"},
                        "detail": {"limit": "200", "remaining": "150", "resetTime": "2026-09-29T18:00:00Z"}}],
            "planName": "Kimi Allegro"
        });
        let r = parse_usage(&v).expect("usage");
        assert_eq!(r.windows.len(), 2);
        assert_eq!(r.windows[0].scope, "5h");
        assert_eq!(
            (r.windows[0].used, r.windows[0].limit),
            (Some(50.0), Some(200.0))
        );
        assert_eq!(r.windows[1].scope, "weekly");
        assert_eq!(r.windows[1].used, Some(512.0));
        assert_eq!(r.plan.expect("plan").name, "Kimi Allegro");
        assert!(parse_usage(&json!({})).is_err());
    }

    #[test]
    fn region_selects_host() {
        let mut cfg = ProviderConfig {
            slug: "kimi-code".into(),
            kind: "kimi-code".into(),
            base_url: None,
            settings: json!({}),
        };
        let e = KimiCode
            .endpoint(&cfg, Protocol::Messages, None)
            .expect("endpoint");
        assert_eq!(e.url, "https://api.kimi.ai/coding/v1/messages");
        cfg.settings = json!({"region": "china"});
        let e = KimiCode
            .endpoint(&cfg, Protocol::Chat, None)
            .expect("endpoint");
        assert_eq!(e.url, "https://api.kimi.com/coding/v1/chat/completions");
        cfg.settings = json!({"region": "mars"});
        assert!(KimiCode.validate(&cfg).is_err());
        assert!(KimiCode.endpoint(&cfg, Protocol::Responses, None).is_err());
    }

    #[test]
    fn cli_token_is_read_only_and_expiry_aware() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(cli_token(dir.path()).is_err(), "no file");
        let path = credentials_path(dir.path());
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, r#"{"access_token":"tok-1","expires_at":4102444800}"#)
            .expect("write");
        assert_eq!(cli_token(dir.path()).expect("token").expose(), "tok-1");
        std::fs::write(&path, r#"{"access_token":"tok-1","expires_at":1000}"#).expect("write");
        let err = cli_token(dir.path()).expect_err("expired");
        assert!(err.to_string().contains("run `kimi`"));
        assert!(!err.to_string().contains("tok-1"), "token never in errors");
    }

    #[tokio::test]
    async fn usage_with_cli_login_and_model_fallback() {
        let up = MockUpstream::json(
            "/coding/v1/usages",
            json!({"usage": {"limit": 10, "used": 1}}),
        )
        .await;
        let cfg = config("kimi-code", "kimi-code", &format!("{}/coding/v1", up.base));
        let dir = tempfile::tempdir().expect("tempdir");
        let path = credentials_path(dir.path());
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, r#"{"access_token":"cli-token"}"#).expect("write");
        let http = http();
        let ctx = DriverContext {
            http: &http,
            config: &cfg,
            key: None,
            home: Some(dir.path()),
        };
        let r = KimiCode.fetch_usage(ctx).await.expect("usage");
        assert_eq!(r.source, UsageSource::OfficialCli);
        assert_eq!(
            up.last_header("authorization").as_deref(),
            Some("Bearer cli-token")
        );

        // /models is not served by this mock: the documented list is used.
        let ctx = DriverContext {
            http: &http,
            config: &cfg,
            key: None,
            home: None,
        };
        let models = KimiCode.list_models(ctx).await.expect("models");
        assert!(models.iter().any(|m| m.upstream_id == "kimi-for-coding"));
    }
}
