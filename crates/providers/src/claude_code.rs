// SPDX-License-Identifier: Apache-2.0

//! Claude Code (Claude subscription). ADR-009: discovery and usage only — no inference.
//!
//! Claude Code is found as a CLI on PATH, the native installer's `~/.local/bin/claude.exe`,
//! or the copy bundled with the Claude desktop app (ExecPlan 008).
//!
//! Usage sources (`usage_source` setting):
//! - `endpoint` — `GET https://api.anthropic.com/api/oauth/usage` (`anthropic-beta:
//!   oauth-2025-04-20`) with the access token Claude Code keeps in
//!   `~/.claude/.credentials.json` (or `%CLAUDE_CONFIG_DIR%`). Undocumented endpoint (trust
//!   `PRIVATE_ENDPOINT`, see docs/providers/claude-code.md); the token is read per refresh,
//!   never stored, never refreshed by Loadout.
//! - `local` — the usage samples the Claude desktop app writes to
//!   `%APPDATA%\Claude\plan-usage-history.json`. No network, no token.
//! - `auto` (default) — `endpoint` when the CLI credentials file exists, else / on failure
//!   `local`.

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

const API_BASE: &str = "https://api.anthropic.com";
const USAGE_PATH: &str = "/api/oauth/usage";
const OAUTH_BETA: &str = "oauth-2025-04-20";
const FIVE_HOURS: u64 = 18_000;
const WEEK: u64 = 604_800;

fn credentials_path(home: &Path) -> PathBuf {
    config_dir("CLAUDE_CONFIG_DIR", home, ".claude").join(".credentials.json")
}

/// Where the Claude desktop app may keep its data. The app ships as an MSIX package, so its
/// `%APPDATA%\Claude` is redirected to `%LOCALAPPDATA%\Packages\Claude_<id>\LocalCache\
/// Roaming\Claude`; only processes inside the package see the redirected path, so Loadout
/// must look at the real one (ExecPlan 008 follow-up). The unpackaged location is kept for
/// other installs. Derived from `home` (not `%APPDATA%`) so tests never see the real
/// user's data.
fn desktop_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![home.join("AppData").join("Roaming").join("Claude")];
    if let Ok(packages) = std::fs::read_dir(home.join("AppData").join("Local").join("Packages")) {
        let mut packaged: Vec<PathBuf> = packages
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("Claude_"))
            .map(|e| e.path().join("LocalCache").join("Roaming").join("Claude"))
            .collect();
        packaged.sort();
        dirs.extend(packaged);
    }
    dirs
}

/// The desktop app's data directory: the one with usage samples (most recently written),
/// else the first that exists, else the unpackaged default.
fn desktop_dir(home: &Path) -> PathBuf {
    let dirs = desktop_dirs(home);
    let modified = |d: &PathBuf| {
        std::fs::metadata(d.join("plan-usage-history.json"))
            .and_then(|m| m.modified())
            .ok()
    };
    dirs.iter()
        .filter_map(|d| modified(d).map(|t| (t, d)))
        .max_by_key(|(t, _)| *t)
        .map(|(_, d)| d.clone())
        .or_else(|| dirs.iter().find(|d| d.is_dir()).cloned())
        .unwrap_or_else(|| dirs[0].clone())
}

fn desktop_usage_path(home: &Path) -> PathBuf {
    desktop_dir(home).join("plan-usage-history.json")
}

/// `1.2.10` → `[1, 2, 10]`; non-numeric parts sort first.
fn version_key(name: &str) -> Vec<u64> {
    name.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// The newest `claude.exe` bundled with the desktop app:
/// `claude-code\<version>\<build>\claude.exe`.
fn bundled_executable(desktop: &Path) -> Option<PathBuf> {
    std::fs::read_dir(desktop.join("claude-code"))
        .ok()?
        .flatten()
        .filter_map(|version| {
            let exe = std::fs::read_dir(version.path())
                .ok()?
                .flatten()
                .map(|build| build.path().join("claude.exe"))
                .find(|p| p.is_file())?;
            Some((version_key(&version.file_name().to_string_lossy()), exe))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, exe)| exe)
}

fn find_claude(home: &Path) -> Option<PathBuf> {
    find_executable("claude")
        .or_else(|| {
            Some(home.join(".local").join("bin").join("claude.exe")).filter(|p| p.is_file())
        })
        .or_else(|| bundled_executable(&desktop_dir(home)))
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
    let specs: [(&str, &str, &str, u64); 4] = [
        ("five_hour", "5h", "5h", FIVE_HOURS),
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

/// The newest sample in the desktop app's usage history. The account id (`org`) is not
/// kept.
#[derive(Debug, PartialEq)]
pub(crate) struct DesktopSample {
    pub at_ms: i64,
    pub five_hour: Option<f64>,
    pub seven_day: Option<f64>,
}

pub(crate) fn latest_desktop_sample(path: &Path) -> Result<DesktopSample, ProviderError> {
    let text = std::fs::read_to_string(path).map_err(|_| {
        ProviderError::Config("the Claude desktop app has no usage history on this machine".into())
    })?;
    let v: Value = serde_json::from_str(&text).map_err(|_| {
        ProviderError::InvalidResponse("unreadable Claude desktop usage history".into())
    })?;
    v.get("samples")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let u = s.get("u")?;
            Some(DesktopSample {
                at_ms: s.get("t").and_then(Value::as_i64)?,
                five_hour: number_field(u, &["fh"]),
                seven_day: number_field(u, &["sd"]),
            })
        })
        .max_by_key(|s| s.at_ms)
        .ok_or_else(|| {
            ProviderError::Config("the Claude desktop app has not recorded usage yet".into())
        })
}

/// Windows from a desktop sample; a window is dropped once the sample is older than the
/// window itself (its percentage no longer says anything).
pub(crate) fn desktop_report(
    sample: &DesktopSample,
    now: i64,
) -> Result<UsageReport, ProviderError> {
    let age_ms = now.saturating_sub(sample.at_ms).max(0);
    let mut report = UsageReport::new(UsageSource::LocalFiles, Confidence::Estimated);
    for (used, scope, label, secs) in [
        (sample.five_hour, "5h", "5h", FIVE_HOURS),
        (sample.seven_day, "weekly", "Weekly", WEEK),
    ] {
        let Some(used) = used else { continue };
        if age_ms > i64::try_from(secs * 1000).unwrap_or(i64::MAX) {
            continue;
        }
        report.windows.push(QuotaWindow {
            scope: scope.to_owned(),
            label: label.to_owned(),
            unit: QuotaUnit::Percent,
            used: Some(used.clamp(0.0, 100.0)),
            limit: None,
            window_seconds: Some(secs),
            resets_at_ms: None,
        });
    }
    if report.windows.is_empty() {
        return Err(ProviderError::Config(
            "the Claude desktop app's usage samples are out of date; open the app to refresh them"
                .into(),
        ));
    }
    Ok(report)
}

fn via_desktop(home: &Path) -> Result<UsageReport, ProviderError> {
    desktop_report(&latest_desktop_sample(&desktop_usage_path(home))?, now_ms())
}

async fn via_endpoint(ctx: &DriverContext<'_>, home: &Path) -> Result<UsageReport, ProviderError> {
    let (token, info) = read_login(&credentials_path(home))?;
    if info.expires_at_ms.is_some_and(|e| e <= now_ms()) {
        return Err(ProviderError::Upstream(UpstreamFailure::new(
            FailureKind::Auth,
            None,
            "the Claude Code login expired; run `claude` once to refresh it",
        )));
    }
    let mut headers = reqwest::header::HeaderMap::new();
    let mut auth = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", token.expose()))
        .map_err(|_| ProviderError::InvalidResponse("malformed Claude Code token".into()))?;
    auth.set_sensitive(true);
    headers.insert("authorization", auth);
    headers.insert("anthropic-beta", OAUTH_BETA.parse().expect("static"));
    headers.insert("accept", "application/json".parse().expect("static"));
    let base = ctx.config.base_url.as_deref().unwrap_or(API_BASE);
    let url = format!("{}{USAGE_PATH}", base.trim_end_matches('/'));
    let v = ctx
        .http
        .get_json(&url, headers, crate::common::USAGE_TIMEOUT)
        .await?;
    let mut report = parse_usage(&v)?;
    report.plan = info.subscription.map(|s| Plan {
        name: format!("Claude {s}"),
        price_usd_monthly: None,
        renews_at_ms: None,
        auto_renew: None,
    });
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
        let executable = find_claude(home);
        let login = read_login(&path).ok().map(|(_, info)| info);
        let desktop = desktop_dir(home);
        let desktop_login = latest_desktop_sample(&desktop_usage_path(home)).is_ok();
        if executable.is_none() && login.is_none() && !desktop.is_dir() {
            return None;
        }
        let (auth, detail, config_path) = match &login {
            Some(l) => (Some("oauth"), l.subscription.clone(), path.parent()),
            None if desktop_login => (
                Some("desktop-app"),
                Some("Claude desktop app".to_owned()),
                Some(desktop.as_path()),
            ),
            None => (None, None, path.parent()),
        };
        Some(Discovered {
            installed: executable.is_some(),
            executable,
            logged_in: login.is_some() || desktop_login,
            auth: auth.map(str::to_owned),
            detail,
            expired: login
                .as_ref()
                .and_then(|l| l.expires_at_ms)
                .is_some_and(|e| e <= now_ms()),
            config_path: config_path.map(Path::to_path_buf),
        })
    }

    fn usage_sources(&self) -> &'static [&'static str] {
        &["endpoint", "local"]
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
            match ctx.config.usage_source() {
                "local" => via_desktop(home),
                "endpoint" => via_endpoint(&ctx, home).await,
                "off" => Err(ProviderError::NotSupported("usage (turned off)")),
                _ if credentials_path(home).is_file() => match via_endpoint(&ctx, home).await {
                    Ok(r) => Ok(r),
                    Err(e) => via_desktop(home).map_err(|_| e),
                },
                _ => via_desktop(home),
            }
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

    fn ctx_config(source: &str) -> loadout_provider_sdk::ProviderConfig {
        let mut cfg = crate::testutil::config("claude-code", "claude-code", "https://unused.test");
        cfg.settings = json!({ "usage_source": source });
        cfg
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
        let cfg = ctx_config("endpoint");
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

    const ORG: &str = "2d4c8920-0000-4000-8000-00000000abcd";

    fn write_desktop(home: &Path, samples: Value) {
        let dir = desktop_dir(home);
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(
            dir.join("plan-usage-history.json"),
            json!({"version": 2, "samples": samples}).to_string(),
        )
        .expect("write");
    }

    #[test]
    fn desktop_sample_is_newest_and_drops_stale_windows() {
        let home = tempfile::tempdir().expect("tempdir");
        let now = 1_800_000_000_000_i64;
        write_desktop(
            home.path(),
            json!([
                {"t": now - 60_000, "org": ORG, "u": {"fh": 12, "sd": 40}},
                {"t": now - 3_600_000, "org": ORG, "u": {"fh": 90, "sd": 39}},
                {"t": "bad", "org": ORG, "u": {"fh": 1}},
            ]),
        );
        let sample = latest_desktop_sample(&desktop_usage_path(home.path())).expect("sample");
        assert_eq!(sample.five_hour, Some(12.0));
        let r = desktop_report(&sample, now).expect("report");
        assert_eq!(r.source, UsageSource::LocalFiles);
        assert_eq!(r.windows.len(), 2);
        assert_eq!(
            (r.windows[1].scope.as_str(), r.windows[1].used),
            ("weekly", Some(40.0))
        );

        // Six hours later the 5h percentage is meaningless; the weekly one still holds.
        let r = desktop_report(&sample, now + 6 * 3_600_000).expect("report");
        assert_eq!(r.windows.len(), 1);
        assert_eq!(r.windows[0].scope, "weekly");
        // After a week nothing is left.
        let err = desktop_report(&sample, now + 8 * 86_400_000).expect_err("stale");
        assert!(err.to_string().contains("out of date"));
        assert!(!format!("{sample:?} {err:?} {r:?}").contains(ORG));
    }

    #[test]
    fn desktop_history_errors_are_readable() {
        let home = tempfile::tempdir().expect("tempdir");
        let path = desktop_usage_path(home.path());
        assert!(latest_desktop_sample(&path).is_err());
        write_desktop(home.path(), json!([]));
        assert!(
            latest_desktop_sample(&path)
                .expect_err("empty")
                .to_string()
                .contains("not recorded")
        );
        std::fs::write(&path, "{not json").expect("write");
        assert!(latest_desktop_sample(&path).is_err());
    }

    #[test]
    fn discovers_the_desktop_app_install() {
        let home = tempfile::tempdir().expect("tempdir");
        let desktop = desktop_dir(home.path());
        for (version, build) in [("2.1.9", "aaa"), ("2.1.10", "bbb")] {
            let dir = desktop.join("claude-code").join(version).join(build);
            std::fs::create_dir_all(&dir).expect("mkdir");
            std::fs::write(dir.join("claude.exe"), "").expect("write");
        }
        assert_eq!(
            bundled_executable(&desktop),
            Some(
                desktop
                    .join("claude-code")
                    .join("2.1.10")
                    .join("bbb")
                    .join("claude.exe")
            )
        );
        write_desktop(
            home.path(),
            json!([{"t": now_ms(), "org": ORG, "u": {"fh": 0, "sd": 5}}]),
        );
        if std::env::var_os("CLAUDE_CONFIG_DIR").is_some() {
            return;
        }
        let found = ClaudeCode.discover(home.path()).expect("found");
        assert!(found.installed && found.logged_in && !found.expired);
        assert_eq!(found.auth.as_deref(), Some("desktop-app"));
        assert_eq!(found.config_path.as_deref(), Some(desktop.as_path()));
        assert!(!format!("{found:?}").contains(ORG));
    }

    #[test]
    fn finds_the_msix_packaged_desktop_app() {
        let home = tempfile::tempdir().expect("tempdir");
        // Unpackaged directory without data, packaged one with usage samples.
        std::fs::create_dir_all(home.path().join("AppData").join("Roaming").join("Claude"))
            .expect("mkdir");
        let packaged = home
            .path()
            .join("AppData")
            .join("Local")
            .join("Packages")
            .join("Claude_pzs8sxrjxfjjc")
            .join("LocalCache")
            .join("Roaming")
            .join("Claude");
        std::fs::create_dir_all(packaged.join("claude-code").join("2.1.1").join("b"))
            .expect("mkdir");
        std::fs::write(
            packaged
                .join("claude-code")
                .join("2.1.1")
                .join("b")
                .join("claude.exe"),
            "",
        )
        .expect("write");
        std::fs::write(
            packaged.join("plan-usage-history.json"),
            json!({"version": 2, "samples": [{"t": now_ms(), "org": ORG, "u": {"fh": 1, "sd": 2}}]})
                .to_string(),
        )
        .expect("write");
        assert_eq!(desktop_dir(home.path()), packaged);
        assert!(latest_desktop_sample(&desktop_usage_path(home.path())).is_ok());
        if std::env::var_os("CLAUDE_CONFIG_DIR").is_some() {
            return;
        }
        let found = ClaudeCode.discover(home.path()).expect("found");
        assert!(found.installed && found.logged_in);
        assert_eq!(found.config_path.as_deref(), Some(packaged.as_path()));
    }

    #[tokio::test]
    async fn local_source_never_reads_the_token() {
        if std::env::var_os("CLAUDE_CONFIG_DIR").is_some() {
            return;
        }
        let home = tempfile::tempdir().expect("tempdir");
        let creds = home.path().join(".claude").join(".credentials.json");
        std::fs::create_dir_all(creds.parent().expect("parent")).expect("mkdir");
        std::fs::write(
            &creds,
            r#"{"claudeAiOauth":{"accessToken":"t","expiresAt":99999999999999}}"#,
        )
        .expect("write");
        write_desktop(
            home.path(),
            json!([{"t": now_ms(), "org": ORG, "u": {"fh": 3, "sd": 7}}]),
        );
        let http = crate::testutil::http();
        for source in ["local", "auto"] {
            let cfg = ctx_config(source);
            let r = ClaudeCode
                .fetch_usage(DriverContext {
                    http: &http,
                    config: &cfg,
                    key: None,
                    home: Some(home.path()),
                })
                .await
                .expect("usage");
            // `local` reads the file directly; `auto` tries the (unreachable) endpoint and
            // falls back to it.
            assert_eq!(r.source, UsageSource::LocalFiles, "{source}");
        }
        let cfg = ctx_config("off");
        assert!(
            ClaudeCode
                .fetch_usage(DriverContext {
                    http: &http,
                    config: &cfg,
                    key: None,
                    home: Some(home.path()),
                })
                .await
                .is_err()
        );
    }
}
