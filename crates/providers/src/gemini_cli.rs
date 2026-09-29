// SPDX-License-Identifier: Apache-2.0

//! Gemini CLI (Google login). ADR-009: discovery and usage only — no inference. Google's
//! documentation warns that using the Gemini CLI backend from third-party software may
//! violate its terms, so Loadout never sends prompts through it.
//!
//! Usage: `POST https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist` (project and
//! tier) then `…:retrieveUserQuota` (`buckets[].remainingFraction / resetTime / modelId`),
//! with the access token from `~/.gemini/oauth_creds.json` — read-only, never refreshed.
//! Undocumented endpoints: trust `PRIVATE_ENDPOINT`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use loadout_provider_sdk::time::{now_ms, number, number_field, timestamp_ms};
use loadout_provider_sdk::usage::{
    Confidence, Plan, QuotaUnit, QuotaWindow, UsageReport, UsageSource,
};
use loadout_provider_sdk::{
    AuthMethod, BoxFuture, Capabilities, Discovered, DriverContext, FailureKind,
    ProviderDescriptor, ProviderError, ProviderModule, UpstreamFailure, find_executable,
};
use loadout_secrets::Secret;
use serde_json::{Value, json};

use crate::codex::SUBSCRIPTION_DRIVERS;

static DESCRIPTOR: ProviderDescriptor = ProviderDescriptor {
    kind: "gemini-cli",
    display_name: "Gemini CLI (Google login)",
    fixed_slug: Some("gemini-cli"),
    default_base_url: None,
    auth: AuthMethod::ExternalManaged,
    transports: &[],
    drivers: SUBSCRIPTION_DRIVERS,
    capabilities: Capabilities::RATE_LIMIT_QUERY,
    default_model_protocols: &[],
};

const BASE: &str = "https://cloudcode-pa.googleapis.com/v1internal";

fn gemini_dir(home: &Path) -> PathBuf {
    home.join(".gemini")
}

pub(crate) fn read_token(home: &Path) -> Result<(Secret, Option<i64>), ProviderError> {
    let text = std::fs::read_to_string(gemini_dir(home).join("oauth_creds.json"))
        .map_err(|_| ProviderError::Config("Gemini CLI is not logged in with Google".into()))?;
    let v: Value = serde_json::from_str(&text)
        .map_err(|_| ProviderError::InvalidResponse("unreadable Gemini CLI credentials".into()))?;
    let token = v
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            ProviderError::Config("Gemini CLI credentials have no access token".into())
        })?;
    let expiry = v.get("expiry_date").and_then(number).map(|ms| ms as i64);
    Ok((Secret::new(token), expiry))
}

fn auth_type(home: &Path) -> Option<String> {
    let v: Value = serde_json::from_str(
        &std::fs::read_to_string(gemini_dir(home).join("settings.json")).ok()?,
    )
    .ok()?;
    v.get("security")
        .and_then(|s| s.get("auth"))
        .and_then(|a| a.get("selectedType"))
        .or_else(|| v.get("selectedAuthType"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Project id and tier name from `loadCodeAssist`.
pub(crate) fn parse_code_assist(v: &Value) -> (Option<String>, Option<String>) {
    let project = match v.get("cloudaicompanionProject") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(o @ Value::Object(_)) => o
            .get("id")
            .or_else(|| o.get("projectId"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        _ => None,
    };
    let tier = v
        .get("paidTier")
        .or_else(|| v.get("currentTier"))
        .and_then(|t| t.get("name").or_else(|| t.get("id")))
        .and_then(Value::as_str)
        .map(str::to_owned);
    (project, tier)
}

/// One window per model: the most constrained bucket.
pub(crate) fn parse_quota(v: &Value) -> Result<Vec<QuotaWindow>, ProviderError> {
    let buckets = v
        .get("buckets")
        .and_then(Value::as_array)
        .ok_or_else(|| ProviderError::InvalidResponse("quota response has no buckets".into()))?;
    let mut by_model: BTreeMap<String, (f64, Option<i64>)> = BTreeMap::new();
    for b in buckets {
        let Some(remaining) = number_field(b, &["remainingFraction"]) else {
            continue;
        };
        let model = b
            .get("modelId")
            .and_then(Value::as_str)
            .unwrap_or("all")
            .to_owned();
        let reset = b.get("resetTime").and_then(timestamp_ms);
        let entry = by_model.entry(model).or_insert((remaining, reset));
        if remaining < entry.0 {
            *entry = (remaining, reset);
        }
    }
    Ok(by_model
        .into_iter()
        .map(|(model, (remaining, reset))| QuotaWindow {
            scope: format!("daily:{model}"),
            label: model,
            unit: QuotaUnit::Percent,
            used: Some(((1.0 - remaining) * 100.0).clamp(0.0, 100.0)),
            limit: None,
            window_seconds: Some(86_400),
            resets_at_ms: reset,
        })
        .collect())
}

#[derive(Debug, Default)]
pub struct GeminiCli;

impl ProviderModule for GeminiCli {
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
        let executable = find_executable("gemini");
        let login = read_token(home).ok();
        if executable.is_none() && login.is_none() && !gemini_dir(home).exists() {
            return None;
        }
        Some(Discovered {
            installed: executable.is_some(),
            executable,
            logged_in: login.is_some(),
            auth: auth_type(home).or_else(|| login.as_ref().map(|_| "oauth-personal".into())),
            detail: None,
            expired: login.and_then(|(_, e)| e).is_some_and(|e| e <= now_ms()),
            config_path: Some(gemini_dir(home)),
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
            let (token, expiry) = read_token(home)?;
            if expiry.is_some_and(|e| e <= now_ms()) {
                return Err(ProviderError::Upstream(UpstreamFailure::new(
                    FailureKind::Auth,
                    None,
                    "the Gemini CLI login expired; run `gemini` once to refresh it",
                )));
            }
            let mut headers = reqwest::header::HeaderMap::new();
            let mut auth =
                reqwest::header::HeaderValue::from_str(&format!("Bearer {}", token.expose()))
                    .map_err(|_| ProviderError::InvalidResponse("malformed Gemini token".into()))?;
            auth.set_sensitive(true);
            headers.insert("authorization", auth);
            let assist = ctx
                .http
                .post_json(
                    &format!("{BASE}:loadCodeAssist"),
                    headers.clone(),
                    &json!({"metadata": {"ideType": "IDE_UNSPECIFIED", "pluginType": "GEMINI"}}),
                    crate::common::USAGE_TIMEOUT,
                )
                .await?;
            let (project, tier) = parse_code_assist(&assist);
            let body = match &project {
                Some(p) => json!({"project": p}),
                None => json!({}),
            };
            let quota = ctx
                .http
                .post_json(
                    &format!("{BASE}:retrieveUserQuota"),
                    headers,
                    &body,
                    crate::common::USAGE_TIMEOUT,
                )
                .await?;
            let mut report = UsageReport::new(UsageSource::PrivateEndpoint, Confidence::Exact);
            report.windows = parse_quota(&quota)?;
            report.plan = tier.map(|t| Plan {
                name: t,
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
    use super::*;

    #[test]
    fn parses_code_assist_and_quota() {
        let (project, tier) = parse_code_assist(&json!({
            "currentTier": {"id": "standard-tier", "name": "Gemini Code Assist"},
            "cloudaicompanionProject": {"id": "proj-123"}
        }));
        assert_eq!(project.as_deref(), Some("proj-123"));
        assert_eq!(tier.as_deref(), Some("Gemini Code Assist"));
        assert_eq!(
            parse_code_assist(&json!({"cloudaicompanionProject": "p"}))
                .0
                .as_deref(),
            Some("p")
        );

        let windows = parse_quota(&json!({"buckets": [
            {"remainingFraction": 0.8, "resetTime": "2026-09-30T00:00:00Z", "modelId": "gemini-pro", "tokenType": "REQUESTS"},
            {"remainingFraction": 0.25, "resetTime": "2026-09-30T00:00:00Z", "modelId": "gemini-pro", "tokenType": "TOKENS"},
            {"remainingFraction": 1, "modelId": "gemini-flash"}
        ]}))
        .expect("quota");
        assert_eq!(windows.len(), 2);
        let pro = windows
            .iter()
            .find(|w| w.label == "gemini-pro")
            .expect("pro");
        assert_eq!(pro.used, Some(75.0), "most constrained bucket wins");
        assert!(parse_quota(&json!({})).is_err());
    }

    #[test]
    fn discovery_reads_no_token_into_output() {
        let home = tempfile::tempdir().expect("tempdir");
        let dir = home.path().join(".gemini");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(
            dir.join("oauth_creds.json"),
            r#"{"access_token":"ya29.secret","expiry_date":1000}"#,
        )
        .expect("write");
        std::fs::write(
            dir.join("settings.json"),
            r#"{"security":{"auth":{"selectedType":"oauth-personal"}}}"#,
        )
        .expect("write");
        let d = GeminiCli.discover(home.path()).expect("found");
        assert!(d.logged_in);
        assert!(d.expired);
        assert_eq!(d.auth.as_deref(), Some("oauth-personal"));
        assert!(!format!("{d:?}").contains("ya29"));
    }
}
