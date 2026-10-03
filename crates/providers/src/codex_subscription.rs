// SPDX-License-Identifier: Apache-2.0

//! Codex through a ChatGPT subscription (ADR-012 §1, ExecPlan 010). Opt-in per account;
//! trust `PRIVATE_ENDPOINT`.
//!
//! Loadout signs in on its own (device code against `auth.openai.com`, Codex CLI client id)
//! and owns the tokens; it never reads or refreshes `~/.codex/auth.json`. Inference goes to
//! `chatgpt.com/backend-api/codex/responses` (streaming Responses only). Protocol knowledge
//! from openai/codex (Apache-2.0) and CLIProxyAPI (MIT, d7914af); no code copied — see
//! docs/providers/codex-subscription.md.
//!
//! Settings (`providers.config_json`):
//! - `auth_base_url`: OAuth server (default `https://auth.openai.com`; tests point it at a
//!   mock).
//! - `client_identity`: `loadout` (default, honest `User-Agent`) or `codex` (also sends
//!   `Originator: codex_cli_rs`, only after the user chose it).

use std::time::Duration;

use loadout_protocol::Protocol;
use loadout_provider_sdk::oauth::{
    DevicePoll, DeviceStart, OAuthFlow, PollOutcome, RefreshError, TokenSet, form_encode,
    jwt_claims,
};
use loadout_provider_sdk::time::now_ms;
use loadout_provider_sdk::{
    AuthMethod, BoxFuture, Capabilities, CatalogModel, DriverContext, Drivers, Endpoint,
    FailureKind, HttpClient, ProviderConfig, ProviderDescriptor, ProviderError, ProviderModule,
    TransportSpec, TrustLevel, UpstreamFailure, classify, error_message, join_url,
};
use loadout_secrets::Secret;
use reqwest::header::HeaderMap;
use serde_json::{Value, json};

static DESCRIPTOR: ProviderDescriptor = ProviderDescriptor {
    kind: "codex-subscription",
    display_name: "Codex (ChatGPT subscription, opt-in)",
    fixed_slug: Some("codex-sub"),
    default_base_url: Some("https://chatgpt.com/backend-api/codex"),
    auth: AuthMethod::OAuth,
    transports: &[TransportSpec {
        protocol: Protocol::Responses,
        trust: TrustLevel::PrivateEndpoint,
    }],
    drivers: Drivers {
        discovery: false,
        auth: true,
        usage: false,
        catalog: true,
        transport: true,
        health: true,
    },
    capabilities: Capabilities::STREAMING
        .union(Capabilities::TOOLS)
        .union(Capabilities::REASONING)
        .union(Capabilities::MODEL_DISCOVERY),
    default_model_protocols: &[Protocol::Responses],
};

const AUTH_BASE: &str = "https://auth.openai.com";
/// The Codex CLI's public OAuth client.
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
/// `client_version` the models endpoint expects.
const CLIENT_VERSION: &str = "0.154.0";
const OAUTH_TIMEOUT: Duration = Duration::from_secs(30);
const AUTH_CLAIM: &str = "https://api.openai.com/auth";

fn auth_base(config: &ProviderConfig) -> String {
    config
        .setting_str("auth_base_url")
        .unwrap_or(AUTH_BASE)
        .trim_end_matches('/')
        .to_owned()
}

fn user_agent() -> String {
    format!("loadout/{}", env!("CARGO_PKG_VERSION"))
}

/// Non-secret facts from an access token.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct TokenFacts {
    pub account_id: Option<String>,
    pub plan: Option<String>,
    pub expires_at_ms: Option<i64>,
    pub email: Option<String>,
}

pub(crate) fn token_facts(token: &str) -> TokenFacts {
    let Some(c) = jwt_claims(token) else {
        return TokenFacts::default();
    };
    let auth = c.get(AUTH_CLAIM);
    let s = |v: Option<&Value>| v.and_then(Value::as_str).map(str::to_owned);
    TokenFacts {
        account_id: s(auth.and_then(|a| a.get("chatgpt_account_id"))),
        plan: s(auth.and_then(|a| a.get("chatgpt_plan_type"))),
        expires_at_ms: c.get("exp").and_then(Value::as_i64).map(|e| e * 1000),
        email: s(c.get("email")).or_else(|| {
            s(c.get("https://api.openai.com/profile")
                .and_then(|p| p.get("email")))
        }),
    }
}

/// Rewrites a Responses body into what the Codex backend accepts.
pub(crate) fn normalize_body(body: &mut Value) {
    let Some(obj) = body.as_object_mut() else {
        return;
    };
    for key in [
        "previous_response_id",
        "max_output_tokens",
        "max_tokens",
        "temperature",
        "top_p",
        "user",
        "truncation",
        "prompt_cache_retention",
        "safety_identifier",
        "stream_options",
        "service_tier",
    ] {
        obj.remove(key);
    }
    obj.insert("stream".into(), Value::Bool(true));
    obj.insert("store".into(), Value::Bool(false));
    if !obj.get("instructions").is_some_and(Value::is_string) {
        obj.insert("instructions".into(), Value::String(String::new()));
    }
    if let Some(text) = obj.get("input").and_then(Value::as_str).map(str::to_owned) {
        obj.insert(
            "input".into(),
            json!([{"type": "message", "role": "user", "content": [{"type": "input_text", "text": text}]}]),
        );
    }
    if let Some(items) = obj.get_mut("input").and_then(Value::as_array_mut) {
        for item in items {
            if item.get("role").and_then(Value::as_str) == Some("system") {
                item["role"] = Value::String("developer".into());
            }
        }
    }
}

/// Seconds until a reported reset (`resets_in_seconds`, or `resets_at` in Unix seconds).
fn reset_after(err: &Value) -> Option<Duration> {
    if let Some(s) = err.get("resets_in_seconds").and_then(Value::as_u64) {
        return Some(Duration::from_secs(s));
    }
    let at = err.get("resets_at").and_then(Value::as_i64)?;
    let ms = at * 1000 - now_ms();
    u64::try_from(ms).ok().map(Duration::from_millis)
}

pub(crate) fn classify_codex(status: u16, headers: &HeaderMap, body: &str) -> UpstreamFailure {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let err = v.get("error").unwrap_or(&v);
    let kind = err
        .get("type")
        .or_else(|| err.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let message = error_message(body);
    let lower = body.to_ascii_lowercase();
    let failure = |k| UpstreamFailure::new(k, Some(status), &message);
    match (status, kind) {
        (_, "usage_limit_reached") => failure(FailureKind::QuotaExhausted {
            reset_after: reset_after(err),
        }),
        (_, "refresh_token_reused" | "authentication_error") | (401, _) => {
            failure(FailureKind::Auth)
        }
        (_, "context_length_exceeded") => failure(FailureKind::InvalidRequest),
        _ if lower.contains("at capacity") || lower.contains("server_is_overloaded") => {
            failure(FailureKind::Server)
        }
        _ => classify(status, headers, body),
    }
}

pub(crate) fn parse_models(v: &Value) -> Result<Vec<CatalogModel>, ProviderError> {
    let list = v
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| ProviderError::InvalidResponse("expected a models list".into()))?;
    Ok(list
        .iter()
        .filter(|m| {
            m.get("visibility")
                .and_then(Value::as_str)
                .unwrap_or("list")
                == "list"
        })
        .filter(|m| m.get("supported_in_api").and_then(Value::as_bool) != Some(false))
        .filter_map(|m| {
            let mut c = CatalogModel::new(m.get("slug").and_then(Value::as_str)?);
            c.display_name = m
                .get("display_name")
                .and_then(Value::as_str)
                .map(str::to_owned);
            c.context_window = m
                .get("context_window")
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok());
            c.protocols = vec![Protocol::Responses];
            c.capabilities = Capabilities::STREAMING
                .union(Capabilities::TOOLS)
                .union(Capabilities::REASONING);
            let images = m
                .get("input_modalities")
                .and_then(Value::as_array)
                .is_some_and(|a| a.iter().any(|x| x == "image"));
            if images {
                c.capabilities = c.capabilities.union(Capabilities::VISION);
            }
            Some(c)
        })
        .collect())
}

async fn post(
    http: &HttpClient,
    url: &str,
    content_type: &'static str,
    body: String,
) -> Result<(u16, String), ProviderError> {
    let resp = http
        .for_url(url)
        .post(url)
        .header("content-type", content_type)
        .header("accept", "application/json")
        .header("user-agent", user_agent())
        .body(body)
        .timeout(OAUTH_TIMEOUT)
        .send()
        .await
        .map_err(|e| ProviderError::Upstream(UpstreamFailure::from_reqwest(&e)))?;
    let status = resp.status().as_u16();
    let text = resp
        .text()
        .await
        .map_err(|e| ProviderError::Upstream(UpstreamFailure::from_reqwest(&e)))?;
    Ok((status, text))
}

fn json_body(text: &str) -> Result<Value, ProviderError> {
    serde_json::from_str(text)
        .map_err(|_| ProviderError::InvalidResponse("expected JSON from the login server".into()))
}

/// Turns a token endpoint response into a [`TokenSet`].
pub(crate) fn token_set(v: &Value) -> Result<TokenSet, ProviderError> {
    let access = v
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            ProviderError::InvalidResponse("the login returned no access token".into())
        })?;
    let facts = token_facts(access);
    let id_email = v
        .get("id_token")
        .and_then(Value::as_str)
        .map(token_facts)
        .and_then(|f| f.email);
    let expires_at_ms = facts.expires_at_ms.or_else(|| {
        v.get("expires_in")
            .and_then(Value::as_i64)
            .map(|s| now_ms() + s * 1000)
    });
    Ok(TokenSet {
        refresh: v
            .get("refresh_token")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .map(Secret::new),
        access: Secret::new(access),
        expires_at_ms,
        account_label: id_email.or(facts.email),
    })
}

#[derive(Debug, Default)]
pub struct CodexOAuth;

impl OAuthFlow for CodexOAuth {
    fn start_device<'a>(
        &'a self,
        http: &'a HttpClient,
        config: &'a ProviderConfig,
    ) -> BoxFuture<'a, Result<DeviceStart, ProviderError>> {
        Box::pin(async move {
            let base = auth_base(config);
            let (status, text) = post(
                http,
                &format!("{base}/api/accounts/deviceauth/usercode"),
                "application/json",
                json!({ "client_id": CLIENT_ID }).to_string(),
            )
            .await?;
            if !(200..300).contains(&status) {
                return Err(ProviderError::Upstream(classify(
                    status,
                    &HeaderMap::new(),
                    &text,
                )));
            }
            let v = json_body(&text)?;
            let s = |k: &str| {
                v.get(k)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|x| !x.is_empty())
            };
            let device_auth_id = s("device_auth_id");
            let user_code = s("user_code").or_else(|| s("usercode"));
            let (Some(device_auth_id), Some(user_code)) = (device_auth_id, user_code) else {
                return Err(ProviderError::InvalidResponse(
                    "the login server returned no device code".into(),
                ));
            };
            let interval = v
                .get("interval")
                .and_then(|i| i.as_u64().or_else(|| i.as_str()?.trim().parse().ok()))
                .filter(|&i| i > 0)
                .unwrap_or(5);
            Ok(DeviceStart {
                verification_url: format!("{base}/codex/device"),
                user_code: user_code.to_owned(),
                interval: Duration::from_secs(interval),
                expires_in: Duration::from_secs(15 * 60),
                poll: DevicePoll {
                    device_auth_id: device_auth_id.to_owned(),
                    user_code: user_code.to_owned(),
                },
            })
        })
    }

    fn poll_device<'a>(
        &'a self,
        http: &'a HttpClient,
        config: &'a ProviderConfig,
        poll: &'a DevicePoll,
    ) -> BoxFuture<'a, Result<PollOutcome, ProviderError>> {
        Box::pin(async move {
            let base = auth_base(config);
            let (status, text) = post(
                http,
                &format!("{base}/api/accounts/deviceauth/token"),
                "application/json",
                json!({ "device_auth_id": poll.device_auth_id, "user_code": poll.user_code })
                    .to_string(),
            )
            .await?;
            if status == 403 || status == 404 {
                return Ok(PollOutcome::Pending);
            }
            if !(200..300).contains(&status) {
                return Err(ProviderError::Upstream(classify(
                    status,
                    &HeaderMap::new(),
                    &text,
                )));
            }
            let v = json_body(&text)?;
            let s = |k: &str| v.get(k).and_then(Value::as_str).filter(|x| !x.is_empty());
            let (Some(code), Some(verifier)) = (s("authorization_code"), s("code_verifier")) else {
                return Err(ProviderError::InvalidResponse(
                    "the login server returned no authorization code".into(),
                ));
            };
            let redirect = format!("{base}/deviceauth/callback");
            let (status, text) = post(
                http,
                &format!("{base}/oauth/token"),
                "application/x-www-form-urlencoded",
                form_encode(&[
                    ("grant_type", "authorization_code"),
                    ("client_id", CLIENT_ID),
                    ("code", code),
                    ("code_verifier", verifier),
                    ("redirect_uri", &redirect),
                ]),
            )
            .await?;
            if !(200..300).contains(&status) {
                return Err(ProviderError::Upstream(classify(
                    status,
                    &HeaderMap::new(),
                    &text,
                )));
            }
            Ok(PollOutcome::Done(token_set(&json_body(&text)?)?))
        })
    }

    fn refresh<'a>(
        &'a self,
        http: &'a HttpClient,
        config: &'a ProviderConfig,
        refresh_token: &'a Secret,
    ) -> BoxFuture<'a, Result<TokenSet, RefreshError>> {
        Box::pin(async move {
            let base = auth_base(config);
            let (status, text) = post(
                http,
                &format!("{base}/oauth/token"),
                "application/x-www-form-urlencoded",
                form_encode(&[
                    ("client_id", CLIENT_ID),
                    ("grant_type", "refresh_token"),
                    ("refresh_token", refresh_token.expose()),
                    ("scope", "openid profile email"),
                ]),
            )
            .await?;
            if !(200..300).contains(&status) {
                let lower = text.to_ascii_lowercase();
                if matches!(status, 400 | 401)
                    && (lower.contains("invalid_grant")
                        || lower.contains("refresh_token_reused")
                        || lower.contains("refresh_token_expired")
                        || lower.contains("refresh_token_invalidated"))
                {
                    return Err(RefreshError::LoginInvalid(error_message(&text)));
                }
                return Err(
                    ProviderError::Upstream(classify(status, &HeaderMap::new(), &text)).into(),
                );
            }
            Ok(token_set(&json_body(&text)?)?)
        })
    }
}

#[derive(Debug, Default)]
pub struct CodexSubscription {
    oauth: CodexOAuth,
}

impl ProviderModule for CodexSubscription {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        &DESCRIPTOR
    }

    fn validate(&self, config: &ProviderConfig) -> Result<(), ProviderError> {
        crate::common::validate(&DESCRIPTOR, config)?;
        match config.setting_str("client_identity") {
            None | Some("loadout" | "codex") => Ok(()),
            Some(other) => Err(ProviderError::Config(format!(
                "client_identity must be loadout or codex, not {other:?}"
            ))),
        }
    }

    fn endpoint(
        &self,
        config: &ProviderConfig,
        protocol: Protocol,
        key: Option<&Secret>,
    ) -> Result<Endpoint, ProviderError> {
        if protocol != Protocol::Responses {
            return Err(ProviderError::NotSupported(
                "protocols other than Responses",
            ));
        }
        let key =
            key.ok_or_else(|| ProviderError::Config("sign in to this account first".into()))?;
        let facts = token_facts(key.expose());
        let account_id = facts.account_id.ok_or_else(|| {
            ProviderError::Config(
                "the stored login has no ChatGPT account id; sign in again".into(),
            )
        })?;
        let base = config.base_url(&DESCRIPTOR)?;
        let mut e = Endpoint::new(join_url(&base, "/responses"))
            .secret_header("authorization", "Bearer ", key)?
            .header("chatgpt-account-id", &account_id)?
            .header("user-agent", &user_agent())?;
        if config.setting_str("client_identity") == Some("codex") {
            e = e.header("originator", "codex_cli_rs")?;
        }
        Ok(e)
    }

    fn prepare_request(&self, _config: &ProviderConfig, protocol: Protocol, body: &mut Value) {
        if protocol == Protocol::Responses {
            normalize_body(body);
        }
    }

    fn upstream_stream_only(&self) -> bool {
        true
    }

    fn account_concurrency(&self) -> Option<u32> {
        Some(2)
    }

    fn classify(&self, status: u16, headers: &HeaderMap, body: &str) -> UpstreamFailure {
        classify_codex(status, headers, body)
    }

    fn oauth(&self) -> Option<&dyn OAuthFlow> {
        Some(&self.oauth)
    }

    fn list_models<'a>(
        &'a self,
        ctx: DriverContext<'a>,
    ) -> BoxFuture<'a, Result<Vec<CatalogModel>, ProviderError>> {
        Box::pin(async move {
            let e = self.endpoint(ctx.config, Protocol::Responses, ctx.key)?;
            let base = ctx.config.base_url(&DESCRIPTOR)?;
            let url = join_url(&base, &format!("/models?client_version={CLIENT_VERSION}"));
            let v = ctx
                .http
                .get_json(&url, e.headers, crate::common::CATALOG_TIMEOUT)
                .await?;
            parse_models(&v)
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use axum::Router;
    use axum::routing::post as axum_post;

    use super::*;

    /// An unsigned JWT with the given claims (tests only).
    pub(crate) fn fake_jwt(claims: &Value) -> String {
        fn b64(bytes: &[u8]) -> String {
            const A: &[u8; 64] =
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
            let mut out = String::new();
            for chunk in bytes.chunks(3) {
                let n = chunk.iter().fold(0u32, |a, &b| (a << 8) | u32::from(b))
                    << (8 * (3 - chunk.len()));
                for i in 0..=chunk.len() {
                    out.push(A[((n >> (18 - 6 * i)) & 63) as usize] as char);
                }
            }
            out
        }
        format!(
            "{}.{}.sig",
            b64(br#"{"alg":"none"}"#),
            b64(claims.to_string().as_bytes())
        )
    }

    pub(crate) fn access_token(exp_secs: i64) -> String {
        fake_jwt(&json!({
            "exp": exp_secs,
            AUTH_CLAIM: {"chatgpt_account_id": "acct-1", "chatgpt_plan_type": "plus"},
            "https://api.openai.com/profile": {"email": "user@example.test"}
        }))
    }

    #[test]
    fn reads_token_facts() {
        let f = token_facts(&access_token(1_900_000_000));
        assert_eq!(f.account_id.as_deref(), Some("acct-1"));
        assert_eq!(f.plan.as_deref(), Some("plus"));
        assert_eq!(f.expires_at_ms, Some(1_900_000_000_000));
        assert_eq!(f.email.as_deref(), Some("user@example.test"));
        assert_eq!(token_facts("garbage"), TokenFacts::default());
    }

    #[test]
    fn endpoint_headers_are_honest_by_default() {
        let m = CodexSubscription::default();
        let mut cfg =
            crate::testutil::config("codex-sub", "codex-subscription", "https://up.test/codex");
        let key = Secret::new(access_token(1_900_000_000));
        let e = m
            .endpoint(&cfg, Protocol::Responses, Some(&key))
            .expect("endpoint");
        assert_eq!(e.url, "https://up.test/codex/responses");
        assert_eq!(e.headers["chatgpt-account-id"], "acct-1");
        assert!(
            e.headers["user-agent"]
                .to_str()
                .expect("ua")
                .starts_with("loadout/")
        );
        assert!(e.headers.get("originator").is_none());
        assert!(e.headers["authorization"].is_sensitive());
        assert!(
            !format!("{e:?}").contains("eyJ"),
            "token must not appear in Debug"
        );

        cfg.settings = json!({"client_identity": "codex"});
        let e = m
            .endpoint(&cfg, Protocol::Responses, Some(&key))
            .expect("endpoint");
        assert_eq!(e.headers["originator"], "codex_cli_rs");
        assert!(m.endpoint(&cfg, Protocol::Chat, Some(&key)).is_err());
        assert!(m.endpoint(&cfg, Protocol::Responses, None).is_err());
        cfg.settings = json!({"client_identity": "browser"});
        assert!(m.validate(&cfg).is_err());
    }

    #[test]
    fn normalizes_request_bodies() {
        let mut body = json!({
            "model": "gpt-x", "input": "hi", "stream": false, "store": true,
            "temperature": 0.2, "max_output_tokens": 100, "previous_response_id": "r1",
            "tools": [{"type": "function", "name": "f"}]
        });
        normalize_body(&mut body);
        assert_eq!(body["stream"], true);
        assert_eq!(body["store"], false);
        assert_eq!(body["instructions"], "");
        assert!(body.get("temperature").is_none() && body.get("max_output_tokens").is_none());
        assert!(body.get("previous_response_id").is_none());
        assert_eq!(body["input"][0]["content"][0]["text"], "hi");
        assert_eq!(body["tools"][0]["name"], "f", "other fields are kept");

        let mut body = json!({"instructions": "be brief", "input": [
            {"type": "message", "role": "system", "content": "sys"},
            {"type": "message", "role": "user", "content": "q"}
        ]});
        normalize_body(&mut body);
        assert_eq!(body["instructions"], "be brief");
        assert_eq!(body["input"][0]["role"], "developer");
        assert_eq!(body["input"][1]["role"], "user");
    }

    #[test]
    fn classifies_codex_errors() {
        let h = HeaderMap::new();
        let f = classify_codex(
            429,
            &h,
            r#"{"error":{"type":"usage_limit_reached","message":"limit","resets_in_seconds":120}}"#,
        );
        assert_eq!(
            f.kind,
            FailureKind::QuotaExhausted {
                reset_after: Some(Duration::from_secs(120))
            }
        );
        let f = classify_codex(
            429,
            &h,
            r#"{"error":{"type":"rate_limit_exceeded","message":"slow"}}"#,
        );
        assert!(
            matches!(f.kind, FailureKind::RateLimited { .. }),
            "{:?}",
            f.kind
        );
        assert_eq!(classify_codex(401, &h, "{}").kind, FailureKind::Auth);
        assert_eq!(
            classify_codex(400, &h, r#"{"error":{"code":"refresh_token_reused"}}"#).kind,
            FailureKind::Auth
        );
        assert_eq!(
            classify_codex(503, &h, r#"{"detail":"Model is at capacity"}"#).kind,
            FailureKind::Server
        );
        assert_eq!(
            classify_codex(
                400,
                &h,
                r#"{"error":{"code":"context_length_exceeded","message":"too long"}}"#
            )
            .kind,
            FailureKind::InvalidRequest
        );
    }

    #[test]
    fn parses_the_model_catalog() {
        let v = json!({"models": [
            {"slug": "gpt-a", "display_name": "GPT A", "context_window": 272000, "visibility": "list",
             "supported_in_api": true, "input_modalities": ["text", "image"]},
            {"slug": "hidden", "visibility": "hide", "supported_in_api": true},
            {"slug": "no-api", "visibility": "list", "supported_in_api": false},
        ]});
        let models = parse_models(&v).expect("models");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].upstream_id, "gpt-a");
        assert_eq!(models[0].context_window, Some(272_000));
        assert!(models[0].capabilities.contains(Capabilities::VISION));
        assert!(parse_models(&json!({})).is_err());
    }

    /// Mock auth server: device code → pending once → approved → token exchange; refresh.
    async fn mock_auth() -> String {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};
        let polls = Arc::new(AtomicU32::new(0));
        let app = Router::new()
            .route(
                "/api/accounts/deviceauth/usercode",
                axum_post(|body: String| async move {
                    assert!(body.contains(CLIENT_ID));
                    axum::Json(json!({"device_auth_id": "dev-1", "user_code": "ABCD-1234", "interval": "1"}))
                }),
            )
            .route(
                "/api/accounts/deviceauth/token",
                axum_post(move || {
                    let polls = polls.clone();
                    async move {
                        if polls.fetch_add(1, Ordering::SeqCst) == 0 {
                            return (axum::http::StatusCode::FORBIDDEN, "pending".to_owned());
                        }
                        (
                            axum::http::StatusCode::OK,
                            json!({"authorization_code": "code-1", "code_verifier": "ver-1", "code_challenge": "ch"})
                                .to_string(),
                        )
                    }
                }),
            )
            .route(
                "/oauth/token",
                axum_post(|body: String| async move {
                    if body.contains("grant_type=refresh_token") {
                        if body.contains("refresh_token=dead") {
                            return (
                                axum::http::StatusCode::BAD_REQUEST,
                                json!({"error": "invalid_grant", "error_description": "reused"}).to_string(),
                            );
                        }
                        return (
                            axum::http::StatusCode::OK,
                            json!({"access_token": access_token(1_900_000_100), "refresh_token": "rt-2"}).to_string(),
                        );
                    }
                    assert!(body.contains("code=code-1") && body.contains("code_verifier=ver-1"));
                    (
                        axum::http::StatusCode::OK,
                        json!({
                            "access_token": access_token(1_900_000_000),
                            "refresh_token": "rt-1",
                            "id_token": fake_jwt(&json!({"email": "id@example.test"}))
                        })
                        .to_string(),
                    )
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn device_login_and_refresh_against_a_mock_server() {
        let base = mock_auth().await;
        let mut cfg = crate::testutil::config("codex-sub", "codex-subscription", "https://up.test");
        cfg.settings = json!({"auth_base_url": base});
        let http = crate::testutil::http();
        let flow = CodexOAuth;

        let start = flow.start_device(&http, &cfg).await.expect("start");
        assert_eq!(start.user_code, "ABCD-1234");
        assert_eq!(start.interval, Duration::from_secs(1));
        assert!(start.verification_url.ends_with("/codex/device"));
        assert!(matches!(
            flow.poll_device(&http, &cfg, &start.poll)
                .await
                .expect("poll"),
            PollOutcome::Pending
        ));
        let PollOutcome::Done(tokens) = flow
            .poll_device(&http, &cfg, &start.poll)
            .await
            .expect("poll")
        else {
            panic!("expected tokens");
        };
        assert_eq!(tokens.account_label.as_deref(), Some("id@example.test"));
        assert_eq!(tokens.expires_at_ms, Some(1_900_000_000_000));
        assert_eq!(tokens.refresh.as_ref().map(Secret::expose), Some("rt-1"));

        let refreshed = flow
            .refresh(&http, &cfg, &Secret::new("rt-1"))
            .await
            .expect("refresh");
        assert_eq!(refreshed.expires_at_ms, Some(1_900_000_100_000));
        assert_eq!(refreshed.refresh.as_ref().map(Secret::expose), Some("rt-2"));
        assert!(matches!(
            flow.refresh(&http, &cfg, &Secret::new("dead")).await,
            Err(RefreshError::LoginInvalid(_))
        ));
    }
}
