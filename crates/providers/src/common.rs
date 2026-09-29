// SPDX-License-Identifier: Apache-2.0

//! Helpers shared by the built-in modules.

use std::time::Duration;

use loadout_protocol::Protocol;
use loadout_provider_sdk::time::{now_ms, number_field, timestamp_ms};
use loadout_provider_sdk::usage::{QuotaUnit, QuotaWindow};
use loadout_provider_sdk::{
    CatalogModel, DriverContext, Drivers, Endpoint, ProviderConfig, ProviderDescriptor,
    ProviderError, join_url, validate_base_url,
};
use loadout_secrets::Secret;
use reqwest::header::HeaderMap;
use serde_json::Value;

pub(crate) use crate::anthropic_compatible::ANTHROPIC_VERSION;
pub(crate) use crate::openai_compatible::{bearer_endpoint, parse_model_list};

pub(crate) const CATALOG_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const USAGE_TIMEOUT: Duration = Duration::from_secs(20);

pub(crate) const API_DRIVERS: Drivers = Drivers {
    discovery: false,
    auth: true,
    usage: false,
    catalog: true,
    transport: true,
    health: false,
};

pub(crate) const API_DRIVERS_WITH_USAGE: Drivers = Drivers {
    usage: true,
    ..API_DRIVERS
};

/// Validates the (possibly defaulted) base URL.
pub(crate) fn validate(
    d: &ProviderDescriptor,
    config: &ProviderConfig,
) -> Result<(), ProviderError> {
    let base = config.base_url(d)?;
    validate_base_url(
        &base,
        config.setting_bool("allow_insecure_http").unwrap_or(false),
    )
    .map(|_| ())
}

/// Bearer auth for every protocol; Messages additionally gets `x-api-key` and
/// `anthropic-version`, which Anthropic-compatible gateways commonly expect.
pub(crate) fn bearer_all(
    base: &str,
    protocol: Protocol,
    key: Option<&Secret>,
) -> Result<Endpoint, ProviderError> {
    let mut e = bearer_endpoint(base, protocol.upstream_path(), key)?;
    if protocol == Protocol::Messages {
        e = e.header("anthropic-version", ANTHROPIC_VERSION)?;
        if let Some(k) = key.filter(|k| !k.is_empty()) {
            e = e.secret_header("x-api-key", "", k)?;
        }
    }
    Ok(e)
}

/// GET `path` under the base URL with bearer auth, returning JSON.
pub(crate) async fn get_bearer(
    ctx: &DriverContext<'_>,
    base: &str,
    path: &str,
    key: Option<&Secret>,
    timeout: Duration,
) -> Result<Value, ProviderError> {
    let endpoint = bearer_endpoint(base, path, key)?;
    let mut headers: HeaderMap = endpoint.headers;
    headers.insert("accept", "application/json".parse().expect("static"));
    ctx.http.get_json(&endpoint.url, headers, timeout).await
}

/// Lists models via `GET {base}/models` and applies `protocols_for` to each.
pub(crate) async fn list_openai_models(
    ctx: &DriverContext<'_>,
    base: &str,
    protocols_for: impl Fn(&str, &Value) -> Vec<Protocol>,
) -> Result<Vec<CatalogModel>, ProviderError> {
    let v = get_bearer(ctx, base, "/models", ctx.key, CATALOG_TIMEOUT).await?;
    let raw: Vec<Value> = v
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut models = parse_model_list(&v)?;
    for m in &mut models {
        let entry = raw
            .iter()
            .find(|r| r.get("id").and_then(Value::as_str) == Some(m.upstream_id.as_str()))
            .unwrap_or(&Value::Null);
        m.protocols = protocols_for(&m.upstream_id, entry);
    }
    Ok(models)
}

/// Builds a quota window from an object with a percentage or used/limit plus a reset given
/// as seconds-from-now or an absolute time.
pub(crate) fn window(
    scope: &str,
    label: &str,
    window_seconds: Option<u64>,
    v: &Value,
) -> Option<QuotaWindow> {
    let percent = number_field(
        v,
        &[
            "usagePercent",
            "usage_percent",
            "usedPercent",
            "used_percent",
            "percent",
            "utilization",
        ],
    );
    let used = number_field(v, &["used", "usage", "usedMicroCents"]);
    let limit = number_field(v, &["limit", "limitMicroCents", "total"]);
    let remaining = number_field(v, &["remaining", "remain"]);
    let reset_in = number_field(
        v,
        &[
            "resetInSec",
            "reset_in_sec",
            "resetsInSeconds",
            "resets_in_seconds",
        ],
    );
    let reset_at = [
        "resetsAt",
        "resets_at",
        "resetAt",
        "reset_at",
        "resetTime",
        "reset_time",
    ]
    .iter()
    .find_map(|k| v.get(*k).and_then(timestamp_ms));
    let resets_at_ms = reset_in
        .map(|s| now_ms() + (s * 1000.0) as i64)
        .or(reset_at);

    let (unit, used, limit) = if let Some(p) = percent {
        (QuotaUnit::Percent, Some(p.clamp(0.0, 100.0)), None)
    } else if let (Some(u), Some(l)) = (used, limit) {
        (QuotaUnit::Requests, Some(u), Some(l))
    } else if let (Some(r), Some(l)) = (remaining, limit) {
        (QuotaUnit::Requests, Some(l - r), Some(l))
    } else {
        return None;
    };
    Some(QuotaWindow {
        scope: scope.to_owned(),
        label: label.to_owned(),
        unit,
        used,
        limit,
        window_seconds,
        resets_at_ms,
    })
}

/// The first object value among `keys`.
pub(crate) fn object<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter()
        .find_map(|k| v.get(*k).filter(|x| x.is_object()))
}

pub(crate) const FIVE_HOURS: u64 = 5 * 3600;
pub(crate) const WEEK: u64 = 7 * 86_400;
pub(crate) const MONTH: u64 = 30 * 86_400;

/// Joins an absolute path onto the scheme+host of `base` (for endpoints outside the
/// versioned API path).
pub(crate) fn origin_join(base: &str, path: &str) -> Result<String, ProviderError> {
    let url = reqwest::Url::parse(base).map_err(|e| ProviderError::Config(e.to_string()))?;
    let origin = url.origin().ascii_serialization();
    Ok(join_url(&origin, path))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn windows_from_percent_and_counts() {
        let w = window(
            "5h",
            "5h",
            Some(FIVE_HOURS),
            &json!({"usagePercent": 78, "resetInSec": 60}),
        )
        .expect("w");
        assert_eq!(w.unit, QuotaUnit::Percent);
        assert_eq!(w.used, Some(78.0));
        assert!(w.resets_at_ms.expect("reset") > now_ms());

        let w = window(
            "weekly",
            "Weekly",
            None,
            &json!({"limit": "100", "remaining": "25", "resetTime": "2026-10-14T00:00:00Z"}),
        )
        .expect("w");
        assert_eq!((w.used, w.limit), (Some(75.0), Some(100.0)));
        assert_eq!(w.resets_at_ms, Some(1_791_936_000_000));
        assert_eq!(w.remaining_fraction(), Some(0.25));

        assert!(window("x", "x", None, &json!({"foo": 1})).is_none());
    }

    #[test]
    fn origin_join_strips_path() {
        assert_eq!(
            origin_join("https://api.deepseek.com/v1", "/user/balance").expect("url"),
            "https://api.deepseek.com/user/balance"
        );
    }
}
