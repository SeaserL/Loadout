// SPDX-License-Identifier: Apache-2.0

//! Loadout-managed OAuth logins (ADR-012 §1, ExecPlan 010 §2.1): device-code login and
//! token refresh. Tokens are owned by Loadout (Credential Manager), never copied from an
//! official CLI.

use std::time::Duration;

use loadout_secrets::Secret;

use crate::{BoxFuture, HttpClient, ProviderConfig, ProviderError};

/// Tokens returned by a login or refresh.
pub struct TokenSet {
    pub access: Secret,
    /// A refresh may or may not rotate the refresh token.
    pub refresh: Option<Secret>,
    pub expires_at_ms: Option<i64>,
    /// Non-secret identity for the UI (e.g. an email).
    pub account_label: Option<String>,
}

impl std::fmt::Debug for TokenSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenSet")
            .field("expires_at_ms", &self.expires_at_ms)
            .field("account_label", &self.account_label)
            .finish_non_exhaustive()
    }
}

/// What the user must do to finish a device-code login.
#[derive(Debug)]
pub struct DeviceStart {
    pub verification_url: String,
    pub user_code: String,
    pub interval: Duration,
    pub expires_in: Duration,
    /// Opaque state passed back to [`OAuthFlow::poll_device`].
    pub poll: DevicePoll,
}

/// Opaque per-login state (not secret on its own: it only completes together with the
/// user's approval in the browser).
#[derive(Clone, Debug)]
pub struct DevicePoll {
    pub device_auth_id: String,
    pub user_code: String,
}

#[derive(Debug)]
pub enum PollOutcome {
    /// The user has not approved yet.
    Pending,
    Done(TokenSet),
}

/// Why a refresh failed: a dead login needs the user, anything else is retried.
#[derive(Debug, thiserror::Error)]
pub enum RefreshError {
    #[error("the login is no longer valid; sign in again ({0})")]
    LoginInvalid(String),
    #[error(transparent)]
    Other(#[from] ProviderError),
}

pub trait OAuthFlow: Send + Sync {
    fn start_device<'a>(
        &'a self,
        http: &'a HttpClient,
        config: &'a ProviderConfig,
    ) -> BoxFuture<'a, Result<DeviceStart, ProviderError>>;

    fn poll_device<'a>(
        &'a self,
        http: &'a HttpClient,
        config: &'a ProviderConfig,
        poll: &'a DevicePoll,
    ) -> BoxFuture<'a, Result<PollOutcome, ProviderError>>;

    fn refresh<'a>(
        &'a self,
        http: &'a HttpClient,
        config: &'a ProviderConfig,
        refresh_token: &'a Secret,
    ) -> BoxFuture<'a, Result<TokenSet, RefreshError>>;

    /// Refresh this long before expiry.
    fn refresh_lead(&self) -> Duration {
        Duration::from_secs(24 * 3600)
    }
}

/// `application/x-www-form-urlencoded` body from pairs.
pub fn form_encode(pairs: &[(&str, &str)]) -> String {
    fn enc(s: &str, out: &mut String) {
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(b as char);
                }
                b' ' => out.push('+'),
                _ => out.push_str(&format!("%{b:02X}")),
            }
        }
    }
    let mut out = String::new();
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        enc(k, &mut out);
        out.push('=');
        enc(v, &mut out);
    }
    out
}

/// Decodes the (unverified) claims of a JWT. Only used to read non-secret facts (expiry,
/// account id) from a token Loadout itself obtained over TLS.
pub fn jwt_claims(token: &str) -> Option<serde_json::Value> {
    let payload = token.split('.').nth(1)?;
    serde_json::from_slice(&base64url_decode(payload)?).ok()
}

fn base64url_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            b'=' => break,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_encoding_escapes_reserved_characters() {
        assert_eq!(
            form_encode(&[
                ("grant_type", "refresh_token"),
                ("scope", "openid email"),
                ("x", "a&b=c/")
            ]),
            "grant_type=refresh_token&scope=openid+email&x=a%26b%3Dc%2F"
        );
    }

    #[test]
    fn decodes_jwt_claims() {
        // {"exp":1790000000,"sub":"u"} base64url, no padding.
        let token = "eyJhbGciOiJub25lIn0.eyJleHAiOjE3OTAwMDAwMDAsInN1YiI6InUifQ.sig";
        let c = jwt_claims(token).expect("claims");
        assert_eq!(c["exp"], 1_790_000_000);
        assert_eq!(c["sub"], "u");
        assert!(jwt_claims("not-a-jwt").is_none());
        assert!(jwt_claims("a.!!!.b").is_none());
    }

    #[test]
    fn token_set_debug_hides_tokens() {
        let t = TokenSet {
            access: Secret::new("access-secret"),
            refresh: Some(Secret::new("refresh-secret")),
            expires_at_ms: Some(1),
            account_label: None,
        };
        assert!(!format!("{t:?}").contains("secret"));
    }
}
