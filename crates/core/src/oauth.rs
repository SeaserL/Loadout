// SPDX-License-Identifier: Apache-2.0

//! Loadout-managed OAuth logins and lower-trust transport opt-in (ADR-012, ExecPlan 010
//! §2.2–2.5): device-code login sessions, background token refresh, and the per-account
//! confirmation that makes a `PRIVATE_ENDPOINT` transport routable.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use loadout_ipc::ErrorBody;
use loadout_ipc::api::{AccountInfo, OAuthStart, OAuthStatus};
use loadout_provider_sdk::oauth::{PollOutcome, RefreshError, TokenSet};
use loadout_provider_sdk::{AuthMethod, FailureKind, ProviderModule};
use loadout_router::{now_ms, validate_alias};
use loadout_secrets::{SecretRef, SecretStore};
use loadout_storage::{Database, NewAccount, Provider};
use tokio_util::sync::CancellationToken;

use crate::control::{ControlPlane, blocking, provider_error, storage};
use crate::route_build::{provider_config, read_secret};

const OAUTH_SOURCE: &str = "loadout-oauth";
const REFRESH_TICK: Duration = Duration::from_secs(300);

#[derive(Clone, Debug)]
enum LoginState {
    Pending,
    Done(Box<AccountInfo>),
    Failed(String),
}

/// In-memory OAuth state of the control plane.
#[derive(Debug, Default)]
pub(crate) struct OAuthState {
    logins: Mutex<HashMap<String, LoginState>>,
    /// Accounts whose refresh token was rejected; not retried until the user signs in again.
    dead: Mutex<HashSet<String>>,
    refresh: tokio::sync::Mutex<()>,
}

impl OAuthState {
    fn set(&self, id: &str, state: LoginState) {
        self.logins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id.to_owned(), state);
    }
}

fn oauth_module(
    control: &ControlPlane,
    provider: &Provider,
) -> Result<Arc<dyn ProviderModule>, ErrorBody> {
    let module = control
        .registry
        .get(&provider.kind)
        .ok_or_else(|| ErrorBody::internal("unknown provider kind"))?
        .clone();
    if module.descriptor().auth != AuthMethod::OAuth || module.oauth().is_none() {
        return Err(ErrorBody::invalid_params(format!(
            "{} does not use a Loadout sign-in",
            provider.slug
        )));
    }
    Ok(module)
}

/// Stores freshly issued tokens: creates the account if needed, writes the new secrets,
/// swaps the references, then deletes the secrets no longer referenced.
fn store_tokens(
    db: &Database,
    secrets: &dyn SecretStore,
    provider: &Provider,
    alias: &str,
    tokens: &TokenSet,
) -> Result<(), ErrorBody> {
    let account = match db.accounts().get(&provider.id, alias).map_err(storage)? {
        Some(a) => a,
        None => db
            .accounts()
            .create(&NewAccount {
                provider_id: &provider.id,
                alias,
                display_name: tokens.account_label.as_deref(),
                account_type: "subscription",
            })
            .map_err(storage)?,
    };
    let put = |secret| -> Result<String, ErrorBody> {
        let r = SecretRef::generate();
        secrets.put(&r, secret).map_err(|e| {
            tracing::error!(error = %e, "cannot store a login token");
            ErrorBody::internal("cannot store the login in Credential Manager")
        })?;
        Ok(r.to_string())
    };
    let access_ref = put(&tokens.access)?;
    let refresh_ref = match &tokens.refresh {
        Some(r) => Some(put(r)?),
        // Not rotated: keep the current refresh token.
        None => db
            .accounts()
            .credentials(&account.id)
            .map_err(storage)?
            .into_iter()
            .find(|c| c.credential_type == "oauth" && c.scope.as_deref() == Some("refresh"))
            .and_then(|c| c.secret_ref),
    };
    let old = db
        .accounts()
        .set_oauth_tokens(
            &account.id,
            OAUTH_SOURCE,
            &access_ref,
            refresh_ref.as_deref(),
            tokens.expires_at_ms,
        )
        .map_err(storage)?;
    for r in old {
        if let Ok(parsed) = r.parse::<SecretRef>() {
            let _ = secrets.delete(&parsed);
        }
    }
    Ok(())
}

impl ControlPlane {
    /// Starts a device-code login; the daemon polls in the background.
    pub async fn oauth_start(
        self: &Arc<Self>,
        provider: String,
        alias: String,
    ) -> Result<OAuthStart, ErrorBody> {
        // Built-in sign-in providers are added on first use (`codex-sub`).
        let db = self.db.clone();
        let slug = provider.clone();
        let exists = blocking(move || {
            Ok(db
                .providers()
                .get_by_slug(&slug)
                .map_err(storage)?
                .is_some())
        })
        .await?;
        if !exists
            && let Some(m) = self.registry.by_fixed_slug(&provider)
            && m.descriptor().auth == AuthMethod::OAuth
        {
            let kind = m.descriptor().kind.to_owned();
            self.add_provider(provider.clone(), kind, None, serde_json::Value::Null, None)
                .await?;
        }
        let db = self.db.clone();
        let (slug, a) = (provider.clone(), alias.clone());
        let provider_row = blocking(move || {
            let p = Self::load_provider(&db, &slug)?;
            if db.accounts().get(&p.id, &a).map_err(storage)?.is_none() {
                let ids = Self::provider_model_ids(&db, &p)?;
                validate_alias(&a, ids.iter().map(String::as_str))
                    .map_err(|e| ErrorBody::invalid_params(e.to_string()))?;
            }
            Ok(p)
        })
        .await?;
        let module = oauth_module(self, &provider_row)?;
        let config = provider_config(&provider_row);
        let flow = module.oauth().expect("checked");
        let start = flow
            .start_device(&self.http, &config)
            .await
            .map_err(provider_error)?;
        let login_id = uuid::Uuid::new_v4().to_string();
        self.oauth.set(&login_id, LoginState::Pending);
        tracing::info!(provider = %provider, alias = %alias, "OAuth login started");

        let (control, id, poll) = (self.clone(), login_id.clone(), start.poll.clone());
        let (interval, expires_in) = (start.interval, start.expires_in);
        tokio::spawn(async move {
            let deadline = tokio::time::Instant::now() + expires_in;
            let module = module.clone();
            let flow = module.oauth().expect("checked");
            let result = loop {
                tokio::time::sleep(interval).await;
                if tokio::time::Instant::now() > deadline {
                    break Err("the sign-in was not completed in time".to_owned());
                }
                match flow.poll_device(&control.http, &config, &poll).await {
                    Ok(PollOutcome::Pending) => continue,
                    Ok(PollOutcome::Done(tokens)) => break Ok(tokens),
                    Err(e) => break Err(e.to_string()),
                }
            };
            let state = match result {
                Ok(tokens) => match control.finish_login(&provider_row, &alias, tokens).await {
                    Ok(info) => {
                        tracing::info!(provider = %provider_row.slug, alias = %alias, "OAuth login completed");
                        LoginState::Done(Box::new(info))
                    }
                    Err(e) => LoginState::Failed(e.message),
                },
                Err(message) => {
                    tracing::info!(provider = %provider_row.slug, alias = %alias, %message, "OAuth login failed");
                    LoginState::Failed(message)
                }
            };
            control.oauth.set(&id, state);
        });

        Ok(OAuthStart {
            login_id,
            verification_url: start.verification_url,
            user_code: start.user_code,
            expires_in_secs: start.expires_in.as_secs(),
        })
    }

    async fn finish_login(
        &self,
        provider: &Provider,
        alias: &str,
        tokens: TokenSet,
    ) -> Result<AccountInfo, ErrorBody> {
        let (db, secrets, p, a) = (
            self.db.clone(),
            self.secrets.clone(),
            provider.clone(),
            alias.to_owned(),
        );
        let account_id = blocking(move || {
            store_tokens(&db, secrets.as_ref(), &p, &a, &tokens)?;
            Ok(Self::load_account(&db, &p, &a)?.id)
        })
        .await?;
        self.oauth
            .dead
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&account_id);
        // The model list needs the new token; a failure leaves the account usable for a
        // later `model sync`.
        if let Err(e) = self
            .sync_models(provider.slug.clone(), alias.to_owned())
            .await
        {
            tracing::warn!(provider = %provider.slug, alias, error = %e.message, "model sync after sign-in failed");
            self.rebuild_routes().await?;
        }
        self.account(&provider.slug, alias).await
    }

    pub fn oauth_status(&self, login_id: &str) -> Result<OAuthStatus, ErrorBody> {
        let state = self
            .oauth
            .logins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(login_id)
            .cloned()
            .ok_or_else(|| ErrorBody::not_found(format!("no sign-in {login_id:?}")))?;
        Ok(match state {
            LoginState::Pending => OAuthStatus {
                state: "pending".into(),
                account: None,
                message: None,
            },
            LoginState::Done(info) => OAuthStatus {
                state: "done".into(),
                account: Some(*info),
                message: None,
            },
            LoginState::Failed(m) => OAuthStatus {
                state: "failed".into(),
                account: None,
                message: Some(m),
            },
        })
    }

    /// Turns a lower-trust transport on (`confirm` must equal the alias) or off.
    pub async fn set_transport(
        &self,
        provider: String,
        alias: String,
        enabled: bool,
        confirm: Option<String>,
    ) -> Result<AccountInfo, ErrorBody> {
        if enabled && confirm.as_deref().map(str::trim) != Some(alias.as_str()) {
            return Err(ErrorBody::invalid_params(format!(
                "to enable it, confirm by passing the account alias {alias:?}"
            )));
        }
        let (db, registry) = (self.db.clone(), self.registry.clone());
        let (slug, a) = (provider.clone(), alias.clone());
        blocking(move || {
            let p = Self::load_provider(&db, &slug)?;
            let module = registry
                .get(&p.kind)
                .ok_or_else(|| ErrorBody::internal("unknown provider kind"))?;
            let low_trust = module
                .descriptor()
                .transports
                .iter()
                .any(|t| !t.trust.allowed_for_inference());
            if !low_trust {
                return Err(ErrorBody::invalid_params(format!(
                    "{slug} needs no transport opt-in"
                )));
            }
            let account = Self::load_account(&db, &p, &a)?;
            db.accounts()
                .set_transport_opt_in(&account.id, enabled.then(now_ms))
                .map_err(storage)?;
            Ok(())
        })
        .await?;
        // Audit trail (ADR-012 §1); no secrets.
        tracing::warn!(provider = %provider, alias = %alias, enabled, "lower-trust transport opt-in changed");
        self.rebuild_routes().await?;
        self.account(&provider, &alias).await
    }

    /// Refreshes every OAuth login that expires within its module's lead time.
    pub(crate) async fn refresh_logins(&self) {
        let _serial = self.oauth.refresh.lock().await;
        let lead = Duration::from_secs(24 * 3600);
        let before = now_ms() + i64::try_from(lead.as_millis()).unwrap_or(i64::MAX);
        let db = self.db.clone();
        let Ok(due) = blocking(move || db.accounts().oauth_expiring(before).map_err(storage)).await
        else {
            return;
        };
        let mut changed = false;
        for cred in due {
            let dead = self
                .oauth
                .dead
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .contains(&cred.account_id);
            if dead {
                continue;
            }
            match self.refresh_account(&cred.account_id).await {
                Ok(()) => changed = true,
                Err(RefreshError::LoginInvalid(message)) => {
                    tracing::warn!(account = %cred.account_id, %message, "OAuth login is no longer valid");
                    self.oauth
                        .dead
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(cred.account_id.clone());
                    for t in self.routes.load().targets() {
                        if *t.account_id == *cred.account_id {
                            t.health
                                .record_failure(FailureKind::Auth, &message, now_ms());
                        }
                    }
                }
                Err(RefreshError::Other(e)) => {
                    tracing::info!(account = %cred.account_id, error = %e, "OAuth refresh failed; will retry");
                }
            }
        }
        if changed && let Err(e) = self.rebuild_routes().await {
            tracing::warn!(error = %e.message, "route rebuild after token refresh failed");
        }
    }

    async fn refresh_account(&self, account_id: &str) -> Result<(), RefreshError> {
        let (db, secrets, id) = (self.db.clone(), self.secrets.clone(), account_id.to_owned());
        let loaded = blocking(move || {
            let account = db
                .accounts()
                .list(None)
                .map_err(storage)?
                .into_iter()
                .find(|a| a.id == id)
                .ok_or_else(|| ErrorBody::not_found("account"))?;
            let provider = db
                .providers()
                .list()
                .map_err(storage)?
                .into_iter()
                .find(|p| p.id == account.provider_id)
                .ok_or_else(|| ErrorBody::not_found("provider"))?;
            let refresh = db
                .accounts()
                .credentials(&account.id)
                .map_err(storage)?
                .into_iter()
                .find(|c| c.credential_type == "oauth" && c.scope.as_deref() == Some("refresh"))
                .and_then(|c| c.secret_ref)
                .and_then(|r| read_secret(secrets.as_ref(), &r));
            Ok((account, provider, refresh))
        })
        .await;
        let (account, provider, refresh) = loaded.map_err(|e| {
            RefreshError::Other(loadout_provider_sdk::ProviderError::Config(e.message))
        })?;
        let Some(refresh) = refresh else {
            return Err(RefreshError::LoginInvalid("no refresh token stored".into()));
        };
        let module = oauth_module(self, &provider).map_err(|e| {
            RefreshError::Other(loadout_provider_sdk::ProviderError::Config(e.message))
        })?;
        let config = provider_config(&provider);
        let tokens = module
            .oauth()
            .expect("checked")
            .refresh(&self.http, &config, &refresh)
            .await?;
        let (db, secrets) = (self.db.clone(), self.secrets.clone());
        blocking(move || store_tokens(&db, secrets.as_ref(), &provider, &account.alias, &tokens))
            .await
            .map_err(|e| {
                RefreshError::Other(loadout_provider_sdk::ProviderError::Config(e.message))
            })?;
        tracing::info!(account = %account_id, "OAuth login refreshed");
        Ok(())
    }
}

/// Background task: refresh OAuth logins before they expire.
pub(crate) async fn refresh_loop(
    control: Arc<ControlPlane>,
    shutdown: CancellationToken,
) -> Result<(), std::convert::Infallible> {
    loop {
        control.refresh_logins().await;
        tokio::select! {
            () = shutdown.cancelled() => return Ok(()),
            () = tokio::time::sleep(REFRESH_TICK) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use axum::Router;
    use axum::http::StatusCode;
    use axum::routing::{get, post};
    use loadout_providers::Registry;
    use loadout_secrets::InMemorySecretStore;
    use serde_json::json;

    use super::*;

    fn b64(bytes: &[u8]) -> String {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n =
                chunk.iter().fold(0u32, |a, &b| (a << 8) | u32::from(b)) << (8 * (3 - chunk.len()));
            for i in 0..=chunk.len() {
                out.push(A[((n >> (18 - 6 * i)) & 63) as usize] as char);
            }
        }
        out
    }

    fn jwt(exp_secs: i64) -> String {
        let claims = json!({
            "exp": exp_secs,
            "https://api.openai.com/auth": {"chatgpt_account_id": "acct-1"},
            "email": "owner@example.test"
        });
        format!("{}.{}.sig", b64(b"{}"), b64(claims.to_string().as_bytes()))
    }

    /// Mock auth server and Codex backend (models list only).
    async fn mock(refreshes: Arc<AtomicU32>, access_exp: i64) -> String {
        let app = Router::new()
            .route(
                "/api/accounts/deviceauth/usercode",
                post(|| async {
                    axum::Json(json!({"device_auth_id": "d", "user_code": "C-1", "interval": 1}))
                }),
            )
            .route(
                "/api/accounts/deviceauth/token",
                post(|| async {
                    axum::Json(
                        json!({"authorization_code": "c", "code_verifier": "v", "code_challenge": "x"}),
                    )
                }),
            )
            .route(
                "/oauth/token",
                post(move |body: String| {
                    let refreshes = refreshes.clone();
                    async move {
                        if body.contains("grant_type=refresh_token") {
                            let n = refreshes.fetch_add(1, Ordering::SeqCst);
                            if body.contains("refresh_token=rt-dead") {
                                return (
                                    StatusCode::BAD_REQUEST,
                                    json!({"error": "invalid_grant"}).to_string(),
                                );
                            }
                            return (
                                StatusCode::OK,
                                json!({
                                    "access_token": jwt(4_000_000_000),
                                    "refresh_token": format!("rt-{}", n + 2)
                                })
                                .to_string(),
                            );
                        }
                        (
                            StatusCode::OK,
                            json!({"access_token": jwt(access_exp), "refresh_token": "rt-1"})
                                .to_string(),
                        )
                    }
                }),
            )
            .route(
                "/codex/models",
                get(|| async {
                    axum::Json(json!({"models": [
                        {"slug": "gpt-test", "visibility": "list", "supported_in_api": true}
                    ]}))
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

    async fn setup(
        access_exp: i64,
    ) -> (Arc<ControlPlane>, Arc<InMemorySecretStore>, Arc<AtomicU32>) {
        let refreshes = Arc::new(AtomicU32::new(0));
        let base = mock(refreshes.clone(), access_exp).await;
        let secrets = Arc::new(InMemorySecretStore::new());
        let control = Arc::new(ControlPlane::new(
            Arc::new(Database::open_in_memory().expect("db")),
            secrets.clone(),
            Registry::builtin(),
            Arc::default(),
            loadout_provider_sdk::HttpClient::new(&Default::default()).expect("http"),
            std::env::temp_dir(),
        ));
        control
            .add_provider(
                "codex-sub".into(),
                "codex-subscription".into(),
                Some(format!("{base}/codex")),
                json!({"auth_base_url": base, "allow_insecure_http": true}),
                None,
            )
            .await
            .expect("provider");
        (control, secrets, refreshes)
    }

    async fn login(control: &Arc<ControlPlane>, alias: &str) -> AccountInfo {
        let start = control
            .oauth_start("codex-sub".into(), alias.into())
            .await
            .expect("start");
        assert_eq!(start.user_code, "C-1");
        for _ in 0..50 {
            let s = control.oauth_status(&start.login_id).expect("status");
            match s.state.as_str() {
                "done" => return s.account.expect("account"),
                "failed" => panic!("login failed: {:?}", s.message),
                _ => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
        panic!("login did not finish");
    }

    fn codex_targets(control: &ControlPlane) -> Vec<Arc<loadout_router::RouteTarget>> {
        control
            .routes
            .load()
            .targets()
            .iter()
            .filter(|t| &*t.provider_slug == "codex-sub")
            .cloned()
            .collect()
    }

    #[tokio::test]
    async fn login_opt_in_and_routing() {
        let (control, secrets, _) = setup(4_000_000_000).await;
        assert!(
            control
                .add_account("codex-sub".into(), "main".into(), None, None, false)
                .await
                .is_err(),
            "OAuth providers cannot be added with a key"
        );
        let info = login(&control, "main").await;
        assert_eq!(info.account_type, "subscription");
        assert_eq!(info.display_name.as_deref(), Some("owner@example.test"));
        assert_eq!(info.models, 1);
        assert!(!info.transport_opt_in);
        assert_eq!(info.login_expires_at_ms, Some(4_000_000_000_000));
        assert_eq!(secrets.len(), 2, "access + refresh");
        assert!(
            codex_targets(&control).is_empty(),
            "not routable before opt-in"
        );

        assert!(
            control
                .set_transport("codex-sub".into(), "main".into(), true, None)
                .await
                .is_err()
        );
        assert!(
            control
                .set_transport(
                    "codex-sub".into(),
                    "main".into(),
                    true,
                    Some("other".into())
                )
                .await
                .is_err()
        );
        let info = control
            .set_transport("codex-sub".into(), "main".into(), true, Some("main".into()))
            .await
            .expect("opt in");
        assert!(info.transport_opt_in);
        let targets = codex_targets(&control);
        assert_eq!(targets.len(), 1);
        assert!(targets[0].opted_in);
        assert_eq!(targets[0].endpoint.headers["chatgpt-account-id"], "acct-1");
        let table = control.routes.load();
        for (name, members) in table.virtuals() {
            assert!(
                members.iter().all(|t| &*t.provider_slug != "codex-sub"),
                "{name} must not include opted-in subscription targets"
            );
        }
        assert!(
            table.resolve("codex-sub/main/gpt-test").is_ok(),
            "reachable by exact name"
        );

        control
            .set_transport("codex-sub".into(), "main".into(), false, None)
            .await
            .expect("opt out");
        assert!(codex_targets(&control).is_empty());

        // Signing in again replaces the tokens and deletes the old secrets.
        login(&control, "main").await;
        assert_eq!(secrets.len(), 2);
    }

    #[tokio::test]
    async fn refresh_rotates_tokens_and_stops_on_dead_logins() {
        // Expires within the 24 h lead: due for refresh.
        let soon = now_ms() / 1000 + 3600;
        let (control, secrets, refreshes) = setup(soon).await;
        login(&control, "main").await;
        control.refresh_logins().await;
        assert_eq!(refreshes.load(Ordering::SeqCst), 1);
        let info = control.account("codex-sub", "main").await.expect("account");
        assert_eq!(info.login_expires_at_ms, Some(4_000_000_000_000));
        assert_eq!(secrets.len(), 2, "old access and refresh tokens deleted");
        control.refresh_logins().await;
        assert_eq!(refreshes.load(Ordering::SeqCst), 1, "not due any more");

        // A rejected refresh token marks the login dead; it is not retried.
        let (control, _, refreshes) = setup(soon).await;
        login(&control, "main").await;
        let (db, store) = (control.db.clone(), control.secrets.clone());
        blocking(move || {
            let p = ControlPlane::load_provider(&db, "codex-sub")?;
            let a = ControlPlane::load_account(&db, &p, "main")?;
            let refresh = db
                .accounts()
                .credentials(&a.id)
                .map_err(storage)?
                .into_iter()
                .find(|c| c.scope.as_deref() == Some("refresh"))
                .and_then(|c| c.secret_ref)
                .expect("refresh ref");
            store
                .put(
                    &refresh.parse().expect("ref"),
                    &loadout_secrets::Secret::new("rt-dead"),
                )
                .expect("put");
            Ok(())
        })
        .await
        .expect("swap");
        control.refresh_logins().await;
        control.refresh_logins().await;
        assert_eq!(
            refreshes.load(Ordering::SeqCst),
            1,
            "a dead login is not retried"
        );
    }
}
