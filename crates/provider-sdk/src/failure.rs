// SPDX-License-Identifier: Apache-2.0

//! Classification of upstream failures into what the router acts on (ADR-003 §7).

use std::fmt;
use std::time::Duration;

use reqwest::header::HeaderMap;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    /// 401 / 403: credentials rejected.
    Auth,
    /// The upstream says the account or organization is disabled, banned, suspended or
    /// deactivated: a terminal state that needs the user's attention.
    AccountDisabled,
    /// 429 without quota semantics.
    RateLimited {
        retry_after: Option<Duration>,
    },
    /// Balance, credits or subscription quota exhausted.
    QuotaExhausted {
        reset_after: Option<Duration>,
    },
    /// 5xx / overloaded.
    Server,
    /// No response in time.
    Timeout,
    /// Connection could not be established.
    Connect,
    /// The model does not exist for this account.
    ModelNotFound,
    /// The account's plan or permissions do not include this model (the key is fine). Loadout
    /// turns the model off for that account (ExecPlan 022).
    ModelUnavailable,
    /// The request itself is wrong; another target would reject it too.
    InvalidRequest,
    Other,
}

impl FailureKind {
    /// Whether trying the next candidate can help (ADR-003 §7).
    pub fn fails_over(self) -> bool {
        !matches!(self, Self::InvalidRequest | Self::Other)
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Auth => "upstream_auth",
            Self::AccountDisabled => "account_disabled",
            Self::RateLimited { .. } => "rate_limit",
            Self::QuotaExhausted { .. } => "quota_exhausted",
            Self::Server => "upstream_error",
            Self::Timeout => "upstream_timeout",
            Self::Connect => "transport_unavailable",
            Self::ModelNotFound => "model_not_found",
            Self::ModelUnavailable => "model_unavailable",
            Self::InvalidRequest => "invalid_request",
            Self::Other => "upstream_error",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpstreamFailure {
    pub kind: FailureKind,
    pub status: Option<u16>,
    /// Sanitized upstream message (secrets masked, length-limited).
    pub message: String,
}

impl UpstreamFailure {
    pub fn new(kind: FailureKind, status: Option<u16>, message: impl AsRef<str>) -> Self {
        Self {
            kind,
            status,
            message: sanitize(message.as_ref()),
        }
    }

    /// From a transport-level error (no HTTP response).
    pub fn from_reqwest(e: &reqwest::Error) -> Self {
        let kind = if e.is_timeout() {
            FailureKind::Timeout
        } else if e.is_connect() {
            FailureKind::Connect
        } else {
            FailureKind::Server
        };
        // reqwest errors include the URL but never headers.
        Self::new(kind, None, e.to_string())
    }
}

impl fmt::Display for UpstreamFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.status {
            Some(s) => write!(f, "HTTP {s}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

const MAX_MESSAGE: usize = 500;

fn sanitize(message: &str) -> String {
    let redacted = loadout_secrets::redact_str(message);
    let mut out: String = redacted.chars().take(MAX_MESSAGE).collect();
    if redacted.chars().count() > MAX_MESSAGE {
        out.push('…');
    }
    out
}

/// Extracts `error.message` (OpenAI / Anthropic shapes) or falls back to the raw body.
pub fn error_message(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(body) {
        let err = v.get("error").unwrap_or(&v);
        if let Some(m) = err.get("message").and_then(Value::as_str) {
            return m.to_owned();
        }
        if let Some(m) = err.as_str() {
            return m.to_owned();
        }
    }
    body.trim().to_owned()
}

fn looks_like_quota(body_lower: &str) -> bool {
    [
        "insufficient_quota",
        "quota",
        "billing",
        "credit",
        "balance",
        "payment",
        "usage limit",
        "limit reached",
        "exceeded your",
    ]
    .iter()
    .any(|needle| body_lower.contains(needle))
}

/// Whether an error text says the account itself is disabled, banned, suspended or
/// deactivated (not merely a bad key). Expects lower-case input.
pub fn looks_like_account_disabled(text_lower: &str) -> bool {
    [
        "deactivated",
        "suspended",
        "banned",
        "account_disabled",
        "account disabled",
        "account is disabled",
        "account has been disabled",
        "organization has been disabled",
        "organization is disabled",
        "organization_disabled",
    ]
    .iter()
    .any(|needle| text_lower.contains(needle))
}

fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    if let Some(ms) = headers
        .get("retry-after-ms")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
    {
        return Some(Duration::from_millis(ms));
    }
    headers
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|s| s.is_finite() && *s >= 0.0)
        .map(Duration::from_secs_f64)
}

/// Default HTTP failure classification. Provider modules may refine it.
pub fn classify(status: u16, headers: &HeaderMap, body: &str) -> UpstreamFailure {
    let message = error_message(body);
    let lower = body.to_ascii_lowercase();
    let kind = match status {
        401 | 403 if looks_like_account_disabled(&lower) => FailureKind::AccountDisabled,
        401 => FailureKind::Auth,
        402 => FailureKind::QuotaExhausted { reset_after: None },
        403 if looks_like_quota(&lower) => FailureKind::QuotaExhausted { reset_after: None },
        403 => FailureKind::Auth,
        404 => FailureKind::ModelNotFound,
        408 | 504 | 524 => FailureKind::Timeout,
        429 if looks_like_quota(&lower) && !lower.contains("rate") => FailureKind::QuotaExhausted {
            reset_after: retry_after(headers),
        },
        429 => FailureKind::RateLimited {
            retry_after: retry_after(headers),
        },
        400 | 409 | 413 | 415 | 422 => FailureKind::InvalidRequest,
        500..=599 => FailureKind::Server,
        _ => FailureKind::Other,
    };
    UpstreamFailure::new(kind, Some(status), message)
}

#[cfg(test)]
mod tests {
    use reqwest::header::HeaderValue;

    use super::*;

    fn h(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut m = HeaderMap::new();
        for (k, v) in pairs {
            m.insert(*k, HeaderValue::from_str(v).expect("value"));
        }
        m
    }

    #[test]
    fn classifies_common_statuses() {
        let none = HeaderMap::new();
        assert_eq!(classify(401, &none, "").kind, FailureKind::Auth);
        assert_eq!(classify(403, &none, "{}").kind, FailureKind::Auth);
        assert_eq!(
            classify(
                403,
                &none,
                r#"{"error":{"type":"permission_error","message":"This organization has been disabled."}}"#
            )
            .kind,
            FailureKind::AccountDisabled
        );
        assert_eq!(
            classify(401, &none, r#"{"error":{"code":"account_deactivated"}}"#).kind,
            FailureKind::AccountDisabled
        );
        assert_eq!(
            classify(429, &none, "your account was suspended").kind,
            FailureKind::RateLimited { retry_after: None },
            "only 401 / 403 can be terminal"
        );
        assert_eq!(
            classify(
                429,
                &h(&[("retry-after", "7")]),
                r#"{"error":{"message":"Rate limit reached"}}"#
            )
            .kind,
            FailureKind::RateLimited {
                retry_after: Some(Duration::from_secs(7))
            }
        );
        assert_eq!(
            classify(429, &none, r#"{"error":{"type":"insufficient_quota","message":"You exceeded your current quota"}}"#).kind,
            FailureKind::QuotaExhausted { reset_after: None }
        );
        assert_eq!(
            classify(402, &none, "").kind,
            FailureKind::QuotaExhausted { reset_after: None }
        );
        assert_eq!(classify(529, &none, "overloaded").kind, FailureKind::Server);
        assert_eq!(
            classify(400, &none, "bad").kind,
            FailureKind::InvalidRequest
        );
        assert!(!FailureKind::InvalidRequest.fails_over());
        assert!(FailureKind::Server.fails_over());
    }

    #[test]
    fn retry_after_ms_takes_precedence() {
        let f = classify(
            429,
            &h(&[("retry-after", "9"), ("retry-after-ms", "1500")]),
            "slow down",
        );
        assert_eq!(
            f.kind,
            FailureKind::RateLimited {
                retry_after: Some(Duration::from_millis(1500))
            }
        );
    }

    #[test]
    fn messages_are_extracted_and_sanitized() {
        let body =
            r#"{"error":{"message":"Incorrect API key provided: sk-proj-ABCDEFGHIJKLMNOP1234"}}"#;
        let f = classify(401, &HeaderMap::new(), body);
        assert_eq!(f.message, "Incorrect API key provided: sk-[REDACTED]");
        let long = "x".repeat(2000);
        assert!(
            classify(500, &HeaderMap::new(), &long)
                .message
                .chars()
                .count()
                <= 501
        );
        assert_eq!(
            error_message(
                r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#
            ),
            "Overloaded"
        );
        assert_eq!(error_message("plain text"), "plain text");
    }
}
