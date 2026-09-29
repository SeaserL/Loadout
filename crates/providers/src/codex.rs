// SPDX-License-Identifier: Apache-2.0

//! Codex (ChatGPT subscription). ADR-009: discovery and usage only — no inference.
//!
//! Usage comes from the official CLI's app-server (`codex app-server`: JSON-RPC 2.0 without
//! the `jsonrpc` field, newline-delimited over stdio; `initialize` → `initialized` →
//! `account/rateLimits/read`). Credentials stay managed by Codex; Loadout never reads its
//! tokens. Fallback: the latest `rate_limits` recorded in the CLI's local session logs
//! (`$CODEX_HOME/sessions/**/rollout-*.jsonl`).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use loadout_provider_sdk::time::{now_ms, number_field, parse_rfc3339, timestamp_ms};
use loadout_provider_sdk::usage::{
    Confidence, Plan, QuotaUnit, QuotaWindow, UsageReport, UsageSource,
};
use loadout_provider_sdk::{
    AuthMethod, BoxFuture, Capabilities, Discovered, DriverContext, Drivers, ProviderDescriptor,
    ProviderError, ProviderModule, config_dir, find_executable,
};
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

pub(crate) const SUBSCRIPTION_DRIVERS: Drivers = Drivers {
    discovery: true,
    auth: true,
    usage: true,
    catalog: false,
    transport: false,
    health: false,
};

static DESCRIPTOR: ProviderDescriptor = ProviderDescriptor {
    kind: "codex",
    display_name: "Codex (ChatGPT)",
    fixed_slug: Some("codex"),
    default_base_url: None,
    auth: AuthMethod::ExternalManaged,
    transports: &[],
    drivers: SUBSCRIPTION_DRIVERS,
    capabilities: Capabilities::RATE_LIMIT_QUERY,
    default_model_protocols: &[],
};

const RPC_TIMEOUT: Duration = Duration::from_secs(20);
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn codex_home(home: &Path) -> PathBuf {
    config_dir("CODEX_HOME", home, ".codex")
}

fn scope_for(minutes: Option<f64>) -> (String, String, Option<u64>) {
    match minutes.map(|m| m.round() as u64) {
        Some(300) => ("5h".into(), "5h".into(), Some(18_000)),
        Some(10_080) => ("weekly".into(), "Weekly".into(), Some(604_800)),
        Some(m) => (format!("{m}m"), format!("{m} min"), Some(m * 60)),
        None => ("window".into(), "Window".into(), None),
    }
}

/// `rateLimits` from the app-server (`usedPercent`, `windowDurationMins`, `resetsAt`) or a
/// session log (`used_percent`, `window_minutes`, `resets_in_seconds` / `resets_at`).
pub(crate) fn windows_from(limits: &Value, event_time_ms: Option<i64>) -> Vec<QuotaWindow> {
    let mut out = Vec::new();
    for key in ["primary", "secondary"] {
        let Some(w) = limits.get(key).filter(|w| w.is_object()) else {
            continue;
        };
        let Some(used) = number_field(w, &["usedPercent", "used_percent"]) else {
            continue;
        };
        let minutes = number_field(
            w,
            &["windowDurationMins", "window_minutes", "windowMinutes"],
        );
        let resets_at_ms = ["resetsAt", "resets_at"]
            .iter()
            .find_map(|k| w.get(*k).and_then(timestamp_ms))
            .or_else(|| {
                number_field(w, &["resetsInSeconds", "resets_in_seconds"])
                    .map(|s| event_time_ms.unwrap_or_else(now_ms) + (s * 1000.0) as i64)
            });
        let (scope, label, secs) = scope_for(minutes);
        out.push(QuotaWindow {
            scope,
            label,
            unit: QuotaUnit::Percent,
            used: Some(used.clamp(0.0, 100.0)),
            limit: None,
            window_seconds: secs,
            resets_at_ms,
        });
    }
    out
}

async fn send<W: AsyncWrite + Unpin>(w: &mut W, msg: Value) -> Result<(), ProviderError> {
    let mut line = msg.to_string();
    line.push('\n');
    w.write_all(line.as_bytes())
        .await
        .map_err(|e| ProviderError::InvalidResponse(format!("codex app-server: {e}")))?;
    w.flush()
        .await
        .map_err(|e| ProviderError::InvalidResponse(format!("codex app-server: {e}")))
}

/// Reads lines until the response with `id` (skipping notifications and other ids).
async fn response<R: AsyncBufRead + Unpin>(r: &mut R, id: u64) -> Result<Value, ProviderError> {
    let mut line = String::new();
    loop {
        line.clear();
        let n = r
            .read_line(&mut line)
            .await
            .map_err(|e| ProviderError::InvalidResponse(format!("codex app-server: {e}")))?;
        if n == 0 {
            return Err(ProviderError::InvalidResponse(
                "codex app-server closed the connection".into(),
            ));
        }
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        if v.get("id").and_then(Value::as_u64) != Some(id) {
            continue;
        }
        if let Some(err) = v.get("error") {
            let message = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("error");
            return Err(ProviderError::InvalidResponse(format!(
                "codex app-server: {message}"
            )));
        }
        return Ok(v.get("result").cloned().unwrap_or(Value::Null));
    }
}

/// The app-server conversation. Returns (rate limits result, account result).
pub(crate) async fn rpc_rate_limits<R, W>(
    r: &mut R,
    w: &mut W,
) -> Result<(Value, Value), ProviderError>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    send(w, json!({
        "method": "initialize",
        "id": 0,
        "params": {"clientInfo": {"name": "loadout", "title": "Loadout", "version": env!("CARGO_PKG_VERSION")}}
    }))
    .await?;
    response(r, 0).await?;
    send(w, json!({"method": "initialized", "params": {}})).await?;

    send(w, json!({"method": "account/read", "id": 1})).await?;
    let account = response(r, 1).await.unwrap_or(Value::Null);

    // Right after the handshake the limits can still be empty; ask again once.
    for id in [2u64, 3] {
        send(w, json!({"method": "account/rateLimits/read", "id": id})).await?;
        let result = response(r, id).await?;
        let limits = result.get("rateLimits").unwrap_or(&result);
        if !windows_from(limits, None).is_empty() || id == 3 {
            return Ok((result, account));
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    unreachable!("loop returns on the last attempt")
}

pub(crate) fn report_from_rpc(
    result: &Value,
    account: &Value,
) -> Result<UsageReport, ProviderError> {
    let limits = result.get("rateLimits").unwrap_or(result);
    let windows = windows_from(limits, None);
    if windows.is_empty() {
        return Err(ProviderError::InvalidResponse(
            "Codex reported no rate-limit windows".into(),
        ));
    }
    let mut report = UsageReport::new(UsageSource::OfficialCli, Confidence::Exact);
    report.windows = windows;
    let acc = account.get("account").unwrap_or(account);
    let plan = limits
        .get("planType")
        .or_else(|| acc.get("planType"))
        .and_then(Value::as_str);
    report.plan = plan.map(|p| Plan {
        name: format!("ChatGPT {p}"),
        price_usd_monthly: None,
        renews_at_ms: None,
        auto_renew: None,
    });
    report.account_label = acc.get("email").and_then(Value::as_str).map(str::to_owned);
    Ok(report)
}

async fn via_app_server() -> Result<UsageReport, ProviderError> {
    let exe = find_executable("codex").ok_or_else(|| {
        ProviderError::Config("the Codex CLI is not installed (codex not on PATH)".into())
    })?;
    let mut child = tokio::process::Command::new(&exe)
        .arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| ProviderError::Config(format!("cannot start codex app-server: {e}")))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| ProviderError::Config("no stdin".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ProviderError::Config("no stdout".into()))?;
    let mut reader = BufReader::new(stdout);
    let outcome = tokio::time::timeout(RPC_TIMEOUT, rpc_rate_limits(&mut reader, &mut stdin)).await;
    drop(stdin);
    let _ = child.kill().await;
    let (result, account) = outcome.map_err(|_| {
        ProviderError::InvalidResponse("codex app-server did not answer in time".into())
    })??;
    report_from_rpc(&result, &account)
}

/// Newest session log first (file names embed the start time; directories are dated).
fn newest_session_files(codex_home: &Path, limit: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![codex_home.join("sessions")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "jsonl") {
                files.push(p);
            }
        }
    }
    files.sort();
    files.reverse();
    files.truncate(limit);
    files
}

/// The last `rate_limits` found in a session log line.
pub(crate) fn rate_limits_in_log(text: &str) -> Option<(Value, Option<i64>)> {
    text.lines()
        .rev()
        .filter(|l| l.contains("rate_limits"))
        .find_map(|line| {
            let v: Value = serde_json::from_str(line).ok()?;
            let payload = v.get("payload").unwrap_or(&v);
            let limits = payload
                .get("rate_limits")
                .or_else(|| payload.get("info").and_then(|i| i.get("rate_limits")))
                .filter(|l| l.is_object())?
                .clone();
            let ts = v
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(parse_rfc3339);
            Some((limits, ts))
        })
}

fn via_session_logs(home: &Path) -> Result<UsageReport, ProviderError> {
    for file in newest_session_files(&codex_home(home), 5) {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        if let Some((limits, ts)) = rate_limits_in_log(&text) {
            let windows = windows_from(&limits, ts);
            if windows.is_empty() {
                continue;
            }
            let mut report = UsageReport::new(UsageSource::LocalFiles, Confidence::Estimated);
            report.windows = windows;
            return Ok(report);
        }
    }
    Err(ProviderError::InvalidResponse(
        "no rate limits in Codex session logs".into(),
    ))
}

#[derive(Debug, Default)]
pub struct Codex;

impl ProviderModule for Codex {
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
        let dir = codex_home(home);
        let auth_file = dir.join("auth.json");
        let executable = find_executable("codex");
        if executable.is_none() && !dir.exists() {
            return None;
        }
        // Only the auth mode is read; token fields are ignored.
        let auth = std::fs::read_to_string(&auth_file)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .map(|v| {
                v.get("auth_mode")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        if v.get("OPENAI_API_KEY").is_some_and(|k| !k.is_null()) {
                            "api_key"
                        } else {
                            "chatgpt"
                        }
                        .to_owned()
                    })
            });
        Some(Discovered {
            installed: executable.is_some(),
            executable,
            logged_in: auth.is_some(),
            auth,
            detail: None,
            expired: false,
            config_path: Some(dir),
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
            match via_app_server().await {
                Ok(r) => Ok(r),
                Err(app_err) => via_session_logs(home).map_err(|_| app_err),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    use super::*;

    #[test]
    fn parses_app_server_limits() {
        let result = json!({"rateLimits": {
            "primary": {"usedPercent": 42, "windowDurationMins": 300, "resetsAt": 1_788_265_323},
            "secondary": {"usedPercent": 61, "windowDurationMins": 10080, "resetsAt": 1_788_765_541},
            "planType": "pro"
        }});
        let account = json!({"account": {"type": "chatgpt", "email": "me@example.com"}});
        let r = report_from_rpc(&result, &account).expect("report");
        assert_eq!(r.windows[0].scope, "5h");
        assert_eq!(r.windows[0].resets_at_ms, Some(1_788_265_323_000));
        assert_eq!(r.windows[1].scope, "weekly");
        assert_eq!(r.windows[1].used, Some(61.0));
        assert_eq!(r.plan.expect("plan").name, "ChatGPT pro");
        assert_eq!(r.account_label.as_deref(), Some("me@example.com"));
        assert!(report_from_rpc(&json!({"rateLimits": {}}), &Value::Null).is_err());
    }

    #[test]
    fn parses_session_log_rate_limits() {
        let log = concat!(
            r#"{"timestamp":"2026-09-29T10:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":10.0,"window_minutes":300,"resets_in_seconds":600}}}}"#,
            "\n",
            r#"{"timestamp":"2026-09-29T11:00:00Z","type":"response_item","payload":{"type":"message"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-29T12:00:00Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"primary":{"used_percent":35.5,"window_minutes":300,"resets_in_seconds":3600},"secondary":{"used_percent":20,"window_minutes":10080,"resets_at":1790000000}}}}"#,
            "\n",
        );
        let (limits, ts) = rate_limits_in_log(log).expect("limits");
        let windows = windows_from(&limits, ts);
        assert_eq!(windows[0].used, Some(35.5), "latest entry wins");
        let noon = parse_rfc3339("2026-09-29T12:00:00Z").expect("ts");
        assert_eq!(
            windows[0].resets_at_ms,
            Some(noon + 3_600_000),
            "relative to the event time"
        );
        assert_eq!(windows[1].resets_at_ms, Some(1_790_000_000_000));
    }

    #[test]
    fn session_log_fallback_reads_newest_file() {
        let home = tempfile::tempdir().expect("tempdir");
        let day = home
            .path()
            .join(".codex")
            .join("sessions")
            .join("2026")
            .join("09")
            .join("29");
        std::fs::create_dir_all(&day).expect("mkdir");
        std::fs::write(
            day.join("rollout-2026-09-29T08-00-00-a.jsonl"),
            r#"{"payload":{"rate_limits":{"primary":{"used_percent":5,"window_minutes":300}}}}"#,
        )
        .expect("write");
        std::fs::write(
            day.join("rollout-2026-09-29T09-00-00-b.jsonl"),
            r#"{"payload":{"rate_limits":{"primary":{"used_percent":55,"window_minutes":300}}}}"#,
        )
        .expect("write");
        // CODEX_HOME may be set on the machine running the tests; use the explicit path.
        let r = {
            let files = newest_session_files(&home.path().join(".codex"), 5);
            assert!(files[0].ends_with("rollout-2026-09-29T09-00-00-b.jsonl"));
            let text = std::fs::read_to_string(&files[0]).expect("read");
            let (limits, ts) = rate_limits_in_log(&text).expect("limits");
            windows_from(&limits, ts)
        };
        assert_eq!(r[0].used, Some(55.0));
    }

    /// A scripted app-server on an in-memory pipe.
    #[tokio::test]
    async fn app_server_conversation() {
        let (client_side, server_side) = tokio::io::duplex(4096);
        let (client_read, mut client_write) = tokio::io::split(client_side);
        let (server_read, mut server_write) = tokio::io::split(server_side);
        let server = tokio::spawn(async move {
            let mut lines = BufReader::new(server_read).lines();
            let mut seen = Vec::new();
            while let Ok(Some(line)) = lines.next_line().await {
                let v: Value = serde_json::from_str(&line).expect("json");
                assert!(
                    v.get("jsonrpc").is_none(),
                    "the jsonrpc field is omitted on the wire"
                );
                let method = v["method"].as_str().expect("method").to_owned();
                seen.push(method.clone());
                let reply = match (method.as_str(), v.get("id")) {
                    ("initialize", Some(id)) => {
                        Some(json!({"id": id, "result": {"userAgent": "codex"}}))
                    }
                    ("account/read", Some(id)) => Some(
                        json!({"id": id, "result": {"account": {"email": "x@y.z", "planType": "plus"}}}),
                    ),
                    // First read is empty, the retry has data.
                    ("account/rateLimits/read", Some(id)) if id == 2 => {
                        Some(json!({"id": id, "result": {"rateLimits": {}}}))
                    }
                    ("account/rateLimits/read", Some(id)) => Some(
                        json!({"id": id, "result": {"rateLimits": {"primary": {"usedPercent": 12, "windowDurationMins": 300}}}}),
                    ),
                    _ => None,
                };
                // Interleave a notification to check it is skipped.
                server_write
                    .write_all(b"{\"method\":\"thread/started\",\"params\":{}}\n")
                    .await
                    .expect("write");
                if let Some(r) = reply {
                    server_write
                        .write_all(format!("{r}\n").as_bytes())
                        .await
                        .expect("write");
                }
                if seen.len() == 5 {
                    break;
                }
            }
            seen
        });
        let mut reader = BufReader::new(client_read);
        let (result, account) = rpc_rate_limits(&mut reader, &mut client_write)
            .await
            .expect("rpc");
        let r = report_from_rpc(&result, &account).expect("report");
        assert_eq!(r.windows[0].used, Some(12.0));
        assert_eq!(r.plan.expect("plan").name, "ChatGPT plus");
        drop(client_write);
        let seen = server.await.expect("server");
        assert_eq!(
            seen,
            [
                "initialize",
                "initialized",
                "account/read",
                "account/rateLimits/read",
                "account/rateLimits/read"
            ]
        );
    }
}
