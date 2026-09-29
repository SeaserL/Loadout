// SPDX-License-Identifier: Apache-2.0

//! Claude Code (Claude subscription). ADR-009: discovery and usage only — no inference.
//!
//! Usage: `GET https://api.anthropic.com/api/oauth/usage` (`anthropic-beta:
//! oauth-2025-04-20`) with the access token Claude Code keeps in
//! `~/.claude/.credentials.json` (or `%CLAUDE_CONFIG_DIR%`). This is an undocumented
//! endpoint (trust `PRIVATE_ENDPOINT`, see docs/providers/claude-code.md); the token is read
//! per refresh, never stored, never refreshed by Loadout.

use std::path::{Path, PathBuf};
use std::time::Duration;

use loadout_provider_sdk::time::{now_ms, number_field, timestamp_ms};
use loadout_provider_sdk::usage::{
    Confidence, Plan, QuotaUnit, QuotaWindow, UsageReport, UsageSource,
};
use loadout_provider_sdk::{
    AuthMethod, BoxFuture, Capabilities, Discovered, DriverContext, FailureKind,
    ProviderDescriptor, ProviderError, ProviderModule, UpstreamFailure, config_dir,
    find_executable,
};
use loadout_secrets::Secret;
use serde_json::Value;

use crate::codex::SUBSCRIPTION_DRIVERS;

static DESCRIPTOR: ProviderDescriptor = ProviderDescriptor {
    kind: "claude-code",
    display_name: "Claude Code (Claude subscription)",
    fixed_slug: Some("claude-code"),
    default_base_url: None,
    auth: AuthMethod::ExternalManaged,
    transports: &[],
    drivers: SUBSCRIPTION_DRIVERS,
    capabilities: Capabilities::RATE_LIMIT_QUERY,
    default_model_protocols: &[],
};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const OAUTH_BETA: &str = "oauth-2025-04-20";

fn credentials_path(home: &Path) -> PathBuf {
    config_dir("CLAUDE_CONFIG_DIR", home, ".claude").join(".credentials.json")
}

/// Non-secret facts about the stored login.
#[derive(Debug, PartialEq)]
pub(crate) struct LoginInfo {
    pub subscription: Option<String>,
    pub expires_at_ms: Option<i64>,
}

/// Reads the credentials file; returns the access token (wrapped) and non-secret info.
pub(crate) fn read_login(path: &Path) -> Result<(Secret, LoginInfo), ProviderError> {
    let text = std::fs::read_to_string(path).map_err(|_| {
        ProviderError::Config("Claude Code is not logged in (no credentials file)".into())
    })?;
    let v: Value = serde_json::from_str(&text)
        .map_err(|_| ProviderError::InvalidResponse("unreadable Claude Code credentials".into()))?;
    let oauth = v
        .get("claudeAiOauth")
        .filter(|o| o.is_object())
        .ok_or_else(|| ProviderError::Config("Claude Code has no subscription login".into()))?;
    let token = oauth
        .get("accessToken")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| ProviderError::Config("Claude Code login has no access token".into()))?;
    Ok((
        Secret::new(token),
        LoginInfo {
            subscription: oauth
                .get("subscriptionType")
                .and_then(Value::as_str)
                .map(str::to_owned),
            expires_at_ms: oauth.get("expiresAt").and_then(timestamp_ms),
        },
    ))
}

pub(crate) fn parse_usage(v: &Value) -> Result<UsageReport, ProviderError> {
    const WEEK: u64 = 604_800;
    let specs: [(&str, &str, &str, u64); 4] = [
        ("five_hour", "5h", "5h", 18_000),
        ("seven_day", "weekly", "Weekly", WEEK),
        ("seven_day_opus", "weekly:opus", "Weekly Opus", WEEK),
        ("seven_day_sonnet", "weekly:sonnet", "Weekly Sonnet", WEEK),
    ];
    let mut report = UsageReport::new(UsageSource::PrivateEndpoint, Confidence::Exact);
    for (key, scope, label, secs) in specs {
        let Some(w) = v.get(key).filter(|w| w.is_object()) else {
            continue;
        };
        // `utilization` is already a percentage (0–100).
        let Some(used) = number_field(w, &["utilization"]) else {
            continue;
        };
        report.windows.push(QuotaWindow {
            scope: scope.to_owned(),
            label: label.to_owned(),
            unit: QuotaUnit::Percent,
            used: Some(used.clamp(0.0, 100.0)),
            limit: None,
            window_seconds: Some(secs),
            resets_at_ms: w.get("resets_at").and_then(timestamp_ms),
        });
    }
    if report.windows.is_empty() {
        return Err(ProviderError::InvalidResponse(
            "Claude usage response has no windows".into(),
        ));
    }
    Ok(report)
}

#[derive(Debug, Default)]
pub struct ClaudeCode;

impl ProviderModule for ClaudeCode {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        &DESCRIPTOR
    }

    fn validate(
        &self,
        _config: &loadout_provider_sdk::ProviderConfig,
    ) -> Result<(), ProviderError> {
        Ok(())
    }

    fn discover(&self, home: &Path) -> Option<Discovered> {
        let path = credentials_path(home);
        let executable = find_executable("claude");
        let login = read_login(&path).ok().map(|(_, info)| info);
        if executable.is_none() && login.is_none() {
            return None;
        }
        Some(Discovered {
            installed: executable.is_some(),
            executable,
            logged_in: login.is_some(),
            auth: login.as_ref().map(|_| "oauth".to_owned()),
            detail: login.as_ref().and_then(|l| l.subscription.clone()),
            expired: login
                .as_ref()
                .and_then(|l| l.expires_at_ms)
                .is_some_and(|e| e <= now_ms()),
            config_path: path.parent().map(Path::to_path_buf),
        })
    }

    fn usage_interval(&self) -> Duration {
        Duration::from_secs(600)
    }

    fn fetch_usage<'a>(
        &'a self,
        ctx: DriverContext<'a>,
    ) -> BoxFuture<'a, Result<UsageReport, ProviderError>> {
        Box::pin(async move {
            let home = ctx
                .home
                .ok_or_else(|| ProviderError::Config("no home directory".into()))?;
            let (token, info) = read_login(&credentials_path(home))?;
            if info.expires_at_ms.is_some_and(|e| e <= now_ms()) {
                return Err(ProviderError::Upstream(UpstreamFailure::new(
                    FailureKind::Auth,
                    None,
                    "the Claude Code login expired; run `claude` once to refresh it",
                )));
            }
            let mut headers = reqwest::header::HeaderMap::new();
            let mut auth =
                reqwest::header::HeaderValue::from_str(&format!("Bearer {}", token.expose()))
                    .map_err(|_| {
                        ProviderError::InvalidResponse("malformed Claude Code token".into())
                    })?;
            auth.set_sensitive(true);
            headers.insert("authorization", auth);
            headers.insert("anthropic-beta", OAUTH_BETA.parse().expect("static"));
            headers.insert("accept", "application/json".parse().expect("static"));
            let v = ctx
                .http
                .get_json(USAGE_URL, headers, crate::common::USAGE_TIMEOUT)
                .await?;
            let mut report = parse_usage(&v)?;
            report.plan = info.subscription.map(|s| Plan {
                name: format!("Claude {s}"),
                price_usd_monthly: None,
                renews_at_ms: None,
                auto_renew: None,
            });
            Ok(report)
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_oauth_usage() {
        let v = json!({
            "five_hour": {"utilization": 37.0, "resets_at": "2026-09-29T15:00:00Z"},
            "seven_day": {"utilization": 52, "resets_at": "2026-10-03T00:00:00+00:00"},
            "seven_day_opus": null,
            "seven_day_sonnet": {"utilization": 12.5, "resets_at": null},
            "extra_usage": {"is_enabled": false}
        });
        let r = parse_usage(&v).expect("usage");
        assert_eq!(r.windows.len(), 3);
        assert_eq!(
            r.windows[0].used,
            Some(37.0),
            "utilization is already a percentage"
        );
        assert_eq!(r.windows[1].scope, "weekly");
        assert_eq!(r.windows[2].resets_at_ms, None);
        assert_eq!(r.source, UsageSource::PrivateEndpoint);
        assert!(parse_usage(&json!({})).is_err());
    }

    #[test]
    fn login_file_parsing_keeps_token_out_of_info_and_errors() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(".credentials.json");
        std::fs::write(
            &path,
            r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-secret","refreshToken":"r","expiresAt":1790000000000,"subscriptionType":"max"}}"#,
        )
        .expect("write");
        let (token, info) = read_login(&path).expect("login");
        assert_eq!(token.expose(), "sk-ant-oat01-secret");
        assert_eq!(
            info,
            LoginInfo {
                subscription: Some("max".into()),
                expires_at_ms: Some(1_790_000_000_000)
            }
        );
        assert!(!format!("{info:?} {token:?}").contains("secret"));

        std::fs::write(&path, r#"{"other":{}}"#).expect("write");
        assert!(read_login(&path).is_err());
        assert!(read_login(&dir.path().join("missing.json")).is_err());
    }

    #[tokio::test]
    async fn expired_login_is_reported_without_network() {
        let home = tempfile::tempdir().expect("tempdir");
        let path = home.path().join(".claude").join(".credentials.json");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(
            &path,
            r#"{"claudeAiOauth":{"accessToken":"t","expiresAt":1000}}"#,
        )
        .expect("write");
        // Only meaningful when CLAUDE_CONFIG_DIR is not set on the machine running tests.
        if std::env::var_os("CLAUDE_CONFIG_DIR").is_some() {
            return;
        }
        let http = crate::testutil::http();
        let cfg = crate::testutil::config("claude-code", "claude-code", "https://unused.test");
        let err = ClaudeCode
            .fetch_usage(DriverContext {
                http: &http,
                config: &cfg,
                key: None,
                home: Some(home.path()),
            })
            .await
            .expect_err("expired");
        assert!(err.to_string().contains("run `claude`"));
    }
}
