// SPDX-License-Identifier: Apache-2.0

//! Browser-session accounts (ADR-014, ExecPlan 017 §5.5): the registry the route builder
//! reads, the background supervisor that starts and checks the browsers, and the control
//! operations behind `account.web_*`.
//!
//! Nothing here runs on the request path: route selection only reads each session's gate
//! (Harness §2.2). Browsers start, restart and are checked in the background.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use loadout_browser::{
    AccountEvents, AccountSettings, BrowserError, ConversationMode, Launcher, WebAccount,
    find_edge, remove_profile_dir,
};
use loadout_ipc::ErrorBody;
use loadout_ipc::Event;
use loadout_ipc::api::{AccountInfo, NOTIFY_EVENT, Notification, WebSessionStatus};
use loadout_platform::OwnedJob;
use loadout_provider_sdk::web::{GateState, WebBinding};
use loadout_provider_sdk::{AuthMethod, CatalogModel, ProviderConfig, ProviderModule};
use loadout_router::{now_ms, validate_alias};
use loadout_storage::{Database, NewAccount, Provider};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use crate::control::{ControlPlane, blocking, storage};
use crate::route_build::provider_config;
use crate::settings;

/// Profile directories live under `<data dir>\browser-profiles\<account id>`.
pub(crate) const PROFILES_DIR: &str = "browser-profiles";
/// Notification kind for a session that needs the user.
pub(crate) const ATTENTION_KIND: &str = "web_session_attention";
const TICK: Duration = Duration::from_secs(5);
const CHECK_INTERVAL: Duration = Duration::from_secs(600);
const ATTENTION_CHECK_INTERVAL: Duration = Duration::from_secs(30);
const MAX_BACKOFF: Duration = Duration::from_secs(300);

/// `accounts.web_session_json` (ExecPlan 017 §9).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WebJson {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_url: Option<String>,
    /// `user` or `auto`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bound_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_interval_secs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily_soft_cap: Option<u32>,
}

impl WebJson {
    pub(crate) fn parse(json: Option<&str>) -> Self {
        json.and_then(|j| serde_json::from_str(j).ok())
            .unwrap_or_default()
    }

    pub(crate) fn settings(&self) -> AccountSettings {
        let d = AccountSettings::default();
        AccountSettings {
            mode: self
                .mode
                .as_deref()
                .and_then(ConversationMode::parse)
                .unwrap_or(d.mode),
            default_conversation: self.conversation_url.clone(),
            min_interval_secs: self.min_interval_secs.unwrap_or(d.min_interval_secs),
            daily_soft_cap: self.daily_soft_cap.unwrap_or(d.daily_soft_cap),
        }
    }
}

#[derive(Debug)]
enum WebEvent {
    DefaultConversation {
        account_id: String,
        url: String,
    },
    Attention {
        account_id: String,
        reason: String,
    },
    Models {
        account_id: String,
        models: Vec<CatalogModel>,
    },
}

#[derive(Debug)]
struct EventSink(mpsc::UnboundedSender<WebEvent>);

impl AccountEvents for EventSink {
    fn default_conversation(&self, account_id: &str, url: &str) {
        let _ = self.0.send(WebEvent::DefaultConversation {
            account_id: account_id.to_owned(),
            url: url.to_owned(),
        });
    }

    fn needs_attention(&self, account_id: &str, reason: &str) {
        let _ = self.0.send(WebEvent::Attention {
            account_id: account_id.to_owned(),
            reason: reason.to_owned(),
        });
    }

    fn models(&self, account_id: &str, models: Vec<CatalogModel>) {
        let _ = self.0.send(WebEvent::Models {
            account_id: account_id.to_owned(),
            models,
        });
    }
}

#[derive(Clone, Debug)]
struct Meta {
    provider: String,
    alias: String,
}

/// All browser-session accounts of the daemon.
pub(crate) struct WebSessions {
    root: OnceLock<PathBuf>,
    enabled: AtomicBool,
    headless: AtomicBool,
    accounts: Mutex<HashMap<String, (Arc<WebAccount>, Meta)>>,
    /// Accounts no longer routable; the supervisor closes their browsers.
    retired: Mutex<Vec<Arc<WebAccount>>>,
    seen_models: Mutex<HashMap<String, Vec<CatalogModel>>>,
    launcher: Mutex<Option<Launcher>>,
    sink: Arc<EventSink>,
    events: tokio::sync::Mutex<mpsc::UnboundedReceiver<WebEvent>>,
}

impl std::fmt::Debug for WebSessions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebSessions")
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl Default for WebSessions {
    fn default() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            root: OnceLock::new(),
            enabled: AtomicBool::new(false),
            headless: AtomicBool::new(false),
            accounts: Mutex::default(),
            retired: Mutex::default(),
            seen_models: Mutex::default(),
            launcher: Mutex::default(),
            sink: Arc::new(EventSink(tx)),
            events: tokio::sync::Mutex::new(rx),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl WebSessions {
    /// Sets the data directory (once, at daemon start).
    pub(crate) fn set_root(&self, data_dir: &Path) {
        let _ = self.root.set(data_dir.join(PROFILES_DIR));
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    pub(crate) fn headless(&self) -> bool {
        self.headless.load(Ordering::Acquire)
    }

    /// Loads `web_session.*` settings; `true` when the headless mode changed.
    pub(crate) fn load_settings(&self, db: &Database) -> bool {
        let get = |key: &str| {
            settings::value(db, key)
                .ok()
                .and_then(|v| settings::parse_bool(&v))
                .unwrap_or(false)
        };
        self.enabled
            .store(get(settings::WEB_SESSION_ENABLED), Ordering::Release);
        let headless = get(settings::WEB_SESSION_HEADLESS);
        self.headless.swap(headless, Ordering::AcqRel) != headless
    }

    /// Closes every browser gracefully, then replaces the launcher (and its job) so the
    /// supervisor restarts them in the new mode. Dropping the launcher first would end the
    /// browsers through the job without letting them flush their profiles.
    async fn restart_all(&self) {
        self.stop_all().await;
        lock(&self.launcher).take();
    }

    pub(crate) fn profile_dir(&self, account_id: &str) -> Option<PathBuf> {
        self.root.get().map(|r| r.join(account_id))
    }

    pub(crate) fn account(&self, account_id: &str) -> Option<Arc<WebAccount>> {
        lock(&self.accounts).get(account_id).map(|(a, _)| a.clone())
    }

    fn all(&self) -> Vec<(Arc<WebAccount>, Meta)> {
        lock(&self.accounts).values().cloned().collect()
    }

    /// The binding attached to an account's targets (route build, no I/O). `None` while the
    /// feature is off or no data directory is set.
    pub(crate) fn binding(
        &self,
        account_id: &str,
        provider: &str,
        alias: &str,
        module: &Arc<dyn ProviderModule>,
        config: &ProviderConfig,
        json: Option<&str>,
    ) -> Result<Option<Arc<WebBinding>>, BrowserError> {
        if !self.enabled() {
            return Ok(None);
        }
        let Some(profile) = self.profile_dir(account_id) else {
            return Ok(None);
        };
        let wanted = WebJson::parse(json).settings();
        let mut accounts = lock(&self.accounts);
        let account = match accounts.get(account_id) {
            Some((a, _)) => {
                // The in-memory default conversation is authoritative (it may be newer than
                // the stored one); `web_bind` updates both.
                let mut s = wanted;
                s.default_conversation = a.settings().default_conversation;
                if s != a.settings() {
                    a.set_settings(s);
                }
                a.clone()
            }
            None => {
                let a = WebAccount::new(
                    account_id,
                    module.clone(),
                    config,
                    profile,
                    wanted,
                    self.sink.clone(),
                )?;
                accounts.insert(
                    account_id.to_owned(),
                    (
                        a.clone(),
                        Meta {
                            provider: provider.to_owned(),
                            alias: alias.to_owned(),
                        },
                    ),
                );
                a
            }
        };
        Ok(Some(Arc::new(WebBinding {
            gate: account.gate().clone(),
            upstream: account,
        })))
    }

    /// Keeps only the accounts of the new route table.
    pub(crate) fn retain(&self, ids: &HashSet<String>) {
        let mut accounts = lock(&self.accounts);
        let gone: Vec<String> = accounts
            .keys()
            .filter(|k| !ids.contains(*k))
            .cloned()
            .collect();
        let mut retired = lock(&self.retired);
        for id in gone {
            if let Some((a, _)) = accounts.remove(&id) {
                retired.push(a);
            }
        }
    }

    pub(crate) fn seen_models(&self, account_id: &str) -> Option<Vec<CatalogModel>> {
        lock(&self.seen_models).get(account_id).cloned()
    }

    fn launcher(&self) -> Result<Launcher, BrowserError> {
        let mut l = lock(&self.launcher);
        if let Some(l) = l.as_ref() {
            return Ok(l.clone());
        }
        let edge = find_edge()?;
        let job = OwnedJob::new().map_err(|e| BrowserError::Launch(e.to_string()))?;
        let launcher = Launcher {
            edge: edge.path,
            job: Arc::new(job),
            headless: self.headless(),
        };
        *l = Some(launcher.clone());
        Ok(launcher)
    }

    /// Closes every browser (daemon shutdown).
    pub(crate) async fn stop_all(&self) {
        for (a, _) in self.all() {
            a.stop().await;
        }
        let retired = std::mem::take(&mut *lock(&self.retired));
        for a in retired {
            a.stop().await;
        }
    }
}

fn status_of(
    web: &WebSessions,
    provider: &str,
    alias: &str,
    account_id: &str,
    json: Option<&str>,
) -> WebSessionStatus {
    let stored = WebJson::parse(json);
    let settings = stored.settings();
    let (state, running, attention, next_allowed_ms, today_count, conversation) =
        match web.account(account_id) {
            Some(a) => {
                let s = a.status();
                (
                    s.state.as_str().to_owned(),
                    s.running,
                    s.attention,
                    s.next_allowed_ms,
                    s.today_count,
                    a.settings().default_conversation,
                )
            }
            None => (
                GateState::Stopped.as_str().to_owned(),
                false,
                None,
                0,
                0,
                settings.default_conversation.clone(),
            ),
        };
    WebSessionStatus {
        provider: provider.to_owned(),
        alias: alias.to_owned(),
        state,
        running,
        attention,
        next_allowed_ms,
        today_count,
        mode: settings.mode.as_str().to_owned(),
        default_conversation: conversation,
        bound_by: stored.bound_by,
        min_interval_secs: settings.min_interval_secs,
        daily_soft_cap: settings.daily_soft_cap,
        enabled_globally: web.enabled(),
        headless: web.headless(),
    }
}

fn web_module(
    control: &ControlPlane,
    provider: &Provider,
) -> Result<Arc<dyn ProviderModule>, ErrorBody> {
    let module = control
        .registry
        .get(&provider.kind)
        .ok_or_else(|| ErrorBody::internal("unknown provider kind"))?
        .clone();
    if module.web_site().is_none() {
        return Err(ErrorBody::invalid_params(format!(
            "{} is not a browser-session provider",
            provider.slug
        )));
    }
    Ok(module)
}

impl ControlPlane {
    fn require_web_enabled(&self) -> Result<(), ErrorBody> {
        if self.web.enabled() {
            Ok(())
        } else {
            Err(ErrorBody::invalid_params(
                "browser sessions are off; enable them first with `loadout config set \
                 web_session.enabled true` (read the risks in docs/providers/web-chatgpt.md)",
            ))
        }
    }

    /// Reloads `web_session.*` settings and applies them.
    pub(crate) async fn apply_web_settings(&self) -> Result<(), ErrorBody> {
        let (db, web) = (self.db.clone(), self.web.clone());
        let headless_changed = blocking(move || Ok(web.load_settings(&db))).await?;
        if headless_changed {
            self.web.restart_all().await;
        }
        tracing::warn!(
            enabled = self.web.enabled(),
            headless = self.web.headless(),
            "browser-session settings changed"
        );
        self.rebuild_routes().await.map(|_| ())
    }

    /// Creates a browser-session account with its two-step confirmation (`confirm` must
    /// equal the alias, ADR-014 §8) and opens the sign-in window.
    pub async fn web_add(
        &self,
        provider: String,
        alias: String,
        confirm: String,
        display_name: Option<String>,
    ) -> Result<AccountInfo, ErrorBody> {
        self.require_web_enabled()?;
        if confirm.trim() != alias {
            return Err(ErrorBody::invalid_params(format!(
                "to add it, confirm the risks by passing the account alias {alias:?}"
            )));
        }
        // Built-in browser-session providers are added on first use (`web-chatgpt`).
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
            && m.descriptor().auth == AuthMethod::BrowserLogin
        {
            let kind = m.descriptor().kind.to_owned();
            self.add_provider(provider.clone(), kind, None, serde_json::Value::Null, None)
                .await?;
        }
        let (db, registry) = (self.db.clone(), self.registry.clone());
        let (slug, a) = (provider.clone(), alias.clone());
        let web = self.web.clone();
        blocking(move || {
            let p = Self::load_provider(&db, &slug)?;
            let module = registry
                .get(&p.kind)
                .ok_or_else(|| ErrorBody::internal("unknown provider kind"))?;
            if module.web_site().is_none() {
                return Err(ErrorBody::invalid_params(format!(
                    "{slug} is not a browser-session provider"
                )));
            }
            let ids = Self::provider_model_ids(&db, &p)?;
            validate_alias(&a, ids.iter().map(String::as_str))
                .map_err(|e| ErrorBody::invalid_params(e.to_string()))?;
            let account = db
                .accounts()
                .create(&NewAccount {
                    provider_id: &p.id,
                    alias: &a,
                    display_name: display_name.as_deref(),
                    account_type: "subscription",
                })
                .map_err(storage)?;
            let json = serde_json::to_string(&WebJson::default())
                .map_err(|e| ErrorBody::internal(e.to_string()))?;
            db.accounts()
                .set_web_session(&account.id, Some(&json))
                .map_err(storage)?;
            db.accounts()
                .set_transport_opt_in(&account.id, Some(now_ms()))
                .map_err(storage)?;
            if let Some(dir) = web.profile_dir(&account.id) {
                std::fs::create_dir_all(&dir)
                    .map_err(|e| ErrorBody::internal(format!("cannot create the profile: {e}")))?;
                loadout_platform::restrict_dir_to_current_user(&dir).map_err(|e| {
                    ErrorBody::internal(format!("cannot protect the profile directory: {e}"))
                })?;
            }
            Ok(())
        })
        .await?;
        // Audit trail (ADR-014 §8); no secrets.
        tracing::warn!(provider = %provider, alias = %alias, "browser-session account added and opted in");
        // Offer the page default model until the page lists its own.
        self.sync_models(provider.clone(), alias.clone()).await?;
        self.account(&provider, &alias).await
    }

    async fn web_target(
        &self,
        provider: &str,
        alias: &str,
    ) -> Result<(Provider, loadout_storage::Account), ErrorBody> {
        let db = self.db.clone();
        let (slug, a) = (provider.to_owned(), alias.to_owned());
        let (p, account) = blocking(move || {
            let p = Self::load_provider(&db, &slug)?;
            let account = Self::load_account(&db, &p, &a)?;
            Ok((p, account))
        })
        .await?;
        web_module(self, &p)?;
        Ok((p, account))
    }

    pub async fn web_status(
        &self,
        provider: String,
        alias: String,
    ) -> Result<WebSessionStatus, ErrorBody> {
        let (p, a) = self.web_target(&provider, &alias).await?;
        Ok(status_of(
            &self.web,
            &p.slug,
            &a.alias,
            &a.id,
            a.web_session_json.as_deref(),
        ))
    }

    fn running_account(&self, account_id: &str) -> Result<Arc<WebAccount>, ErrorBody> {
        self.web.account(account_id).ok_or_else(|| {
            ErrorBody::invalid_params(
                "this account has no browser session: browser sessions must be enabled and the \
                 account enabled and opted in",
            )
        })
    }

    /// Brings the account's browser window to the front (starting it if needed).
    pub async fn web_show(
        &self,
        provider: String,
        alias: String,
    ) -> Result<WebSessionStatus, ErrorBody> {
        self.require_web_enabled()?;
        let (_, a) = self.web_target(&provider, &alias).await?;
        let account = self.running_account(&a.id)?;
        if !account.is_running() {
            let launcher = self.web.launcher().map_err(browser_error)?;
            account.start(&launcher).await.map_err(browser_error)?;
        }
        account.show().await.map_err(browser_error)?;
        self.web_status(provider, alias).await
    }

    /// Re-reads the page state now ("I have dealt with it").
    pub async fn web_recheck(
        &self,
        provider: String,
        alias: String,
    ) -> Result<WebSessionStatus, ErrorBody> {
        let (_, a) = self.web_target(&provider, &alias).await?;
        let account = self.running_account(&a.id)?;
        account.check().await;
        self.web_status(provider, alias).await
    }

    /// Binds (or unbinds, with `None`) the conversation used when a request names none.
    pub async fn web_bind(
        &self,
        provider: String,
        alias: String,
        conversation_url: Option<String>,
    ) -> Result<WebSessionStatus, ErrorBody> {
        let (p, a) = self.web_target(&provider, &alias).await?;
        let url = conversation_url
            .map(|u| u.trim().to_owned())
            .filter(|u| !u.is_empty());
        if let Some(u) = &url {
            let module = web_module(self, &p)?;
            let site = module.web_site().expect("checked");
            let origin = site.origin(&provider_config(&p));
            if site.conversation_id(&origin, u).is_none() {
                return Err(ErrorBody::invalid_params(format!(
                    "{u:?} is not a conversation URL of {origin}"
                )));
            }
        }
        let mut json = WebJson::parse(a.web_session_json.as_deref());
        json.conversation_url.clone_from(&url);
        json.bound_by = url.as_ref().map(|_| "user".to_owned());
        self.store_web_json(&a.id, &json).await?;
        if let Some(account) = self.web.account(&a.id) {
            let mut s = account.settings();
            s.default_conversation = url;
            account.set_settings(s);
        }
        self.web_status(provider, alias).await
    }

    pub async fn web_config(
        &self,
        provider: String,
        alias: String,
        mode: Option<String>,
        min_interval_secs: Option<u32>,
        daily_soft_cap: Option<u32>,
    ) -> Result<WebSessionStatus, ErrorBody> {
        let (_, a) = self.web_target(&provider, &alias).await?;
        let mut json = WebJson::parse(a.web_session_json.as_deref());
        if let Some(m) = mode {
            let parsed = ConversationMode::parse(m.trim()).ok_or_else(|| {
                ErrorBody::invalid_params("mode must be `persistent` or `stateless`")
            })?;
            json.mode = Some(parsed.as_str().to_owned());
        }
        if let Some(s) = min_interval_secs {
            if s > 3600 {
                return Err(ErrorBody::invalid_params(
                    "min_interval_secs must be at most 3600",
                ));
            }
            json.min_interval_secs = Some(s);
        }
        if let Some(c) = daily_soft_cap {
            json.daily_soft_cap = Some(c);
        }
        self.store_web_json(&a.id, &json).await?;
        if let Some(account) = self.web.account(&a.id) {
            let mut s = json.settings();
            s.default_conversation = account.settings().default_conversation;
            account.set_settings(s);
        }
        self.web_status(provider, alias).await
    }

    async fn store_web_json(&self, account_id: &str, json: &WebJson) -> Result<(), ErrorBody> {
        let text = serde_json::to_string(json).map_err(|e| ErrorBody::internal(e.to_string()))?;
        let (db, id) = (self.db.clone(), account_id.to_owned());
        blocking(move || {
            db.accounts()
                .set_web_session(&id, Some(&text))
                .map_err(storage)?;
            Ok(())
        })
        .await
    }

    /// Deletes a removed account's browser profile (after its browser closed).
    pub(crate) async fn remove_web_profile(&self, account_id: &str) {
        let Some(dir) = self.web.profile_dir(account_id) else {
            return;
        };
        if let Some(a) = self.web.account(account_id) {
            a.stop().await;
        }
        let shown = dir.display().to_string();
        match tokio::task::spawn_blocking(move || remove_profile_dir(&dir)).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::warn!(dir = %shown, error = %e, "could not delete a browser profile")
            }
            Err(_) => tracing::warn!("browser profile deletion task failed"),
        }
    }

    async fn handle_web_event(&self, event: WebEvent, events: &broadcast::Sender<Event>) {
        match event {
            WebEvent::DefaultConversation { account_id, url } => {
                let db = self.db.clone();
                let id = account_id.clone();
                let result = blocking(move || {
                    let Some(a) = db.accounts().get_by_id(&id).map_err(storage)? else {
                        return Ok(None);
                    };
                    Ok(Some(a))
                })
                .await;
                if let Ok(Some(a)) = result {
                    let mut json = WebJson::parse(a.web_session_json.as_deref());
                    if json.conversation_url.is_none() {
                        json.conversation_url = Some(url);
                        json.bound_by = Some("auto".into());
                        if let Err(e) = self.store_web_json(&account_id, &json).await {
                            tracing::warn!(error = %e.message, "could not store the default conversation");
                        }
                    }
                }
            }
            WebEvent::Attention { account_id, reason } => {
                let subject = lock(&self.web.accounts)
                    .get(&account_id)
                    .map(|(_, m)| format!("{}/{}", m.provider, m.alias))
                    .unwrap_or(account_id);
                let n = Notification {
                    kind: ATTENTION_KIND.into(),
                    key: subject.clone(),
                    title: format!("{subject}: the web session needs you"),
                    body: reason,
                    subject,
                    detail: None,
                    value: None,
                };
                if let Ok(data) = serde_json::to_value(&n) {
                    let _ = events.send(Event::new(NOTIFY_EVENT, data));
                }
            }
            WebEvent::Models { account_id, models } => {
                let changed = {
                    let mut seen = lock(&self.web.seen_models);
                    let changed = seen.get(&account_id) != Some(&models);
                    seen.insert(account_id.clone(), models);
                    changed
                };
                if changed {
                    let meta = lock(&self.web.accounts)
                        .get(&account_id)
                        .map(|(_, m)| m.clone());
                    if let Some(m) = meta
                        && let Err(e) = self.sync_models(m.provider, m.alias).await
                    {
                        tracing::warn!(error = %e.message, "could not store the page's model list");
                    }
                }
            }
        }
    }

    /// Seeds today's request count of a session from the request log (once per start).
    async fn seed_today(&self, account: &WebAccount, meta: &Meta) {
        let now = now_ms();
        let day_start = now - now.rem_euclid(24 * 3600 * 1000);
        let (db, p, a) = (self.db.clone(), meta.provider.clone(), meta.alias.clone());
        if let Ok(Ok(n)) =
            tokio::task::spawn_blocking(move || db.count_account_requests(&p, &a, day_start)).await
        {
            account.gate().seed_count(now, n);
        }
    }
}

fn browser_error(e: BrowserError) -> ErrorBody {
    match e {
        BrowserError::EdgeNotFound | BrowserError::EdgeTooOld(_) | BrowserError::ProfileInUse => {
            ErrorBody::invalid_params(e.to_string())
        }
        other => ErrorBody::internal(other.to_string()),
    }
}

#[derive(Debug)]
struct Schedule {
    next_start: Instant,
    backoff: Duration,
    next_check: Instant,
    seeded: bool,
}

/// Background supervisor (restartable): starts browsers of opted-in accounts, restarts
/// crashed ones with backoff, checks page states, applies session events.
pub(crate) async fn run(
    control: Arc<ControlPlane>,
    events: broadcast::Sender<Event>,
    shutdown: CancellationToken,
) -> Result<(), String> {
    let web = control.web.clone();
    let mut rx = web.events.lock().await;
    let mut schedule: HashMap<String, Schedule> = HashMap::new();
    let mut launcher_error: Option<String> = None;
    loop {
        tokio::select! {
            () = tokio::time::sleep(TICK) => {}
            ev = rx.recv() => {
                if let Some(ev) = ev {
                    control.handle_web_event(ev, &events).await;
                }
                continue;
            }
            () = shutdown.cancelled() => {
                web.stop_all().await;
                return Ok(());
            }
        }
        let retired = std::mem::take(&mut *lock(&web.retired));
        for a in retired {
            a.stop().await;
            schedule.remove(a.account_id());
        }
        let accounts = web.all();
        if !web.enabled() {
            for (a, _) in &accounts {
                if a.is_running() {
                    a.stop().await;
                }
            }
            continue;
        }
        if accounts.is_empty() {
            continue;
        }
        let launcher = match web.launcher() {
            Ok(l) => {
                launcher_error = None;
                l
            }
            Err(e) => {
                let message = e.to_string();
                if launcher_error.as_deref() != Some(message.as_str()) {
                    tracing::warn!(error = %message, "browser sessions cannot start");
                    launcher_error = Some(message);
                }
                continue;
            }
        };
        let now = Instant::now();
        for (account, meta) in accounts {
            let s = schedule
                .entry(account.account_id().to_owned())
                .or_insert(Schedule {
                    next_start: now,
                    backoff: Duration::from_secs(1),
                    next_check: now + CHECK_INTERVAL,
                    seeded: false,
                });
            if !s.seeded {
                control.seed_today(&account, &meta).await;
                s.seeded = true;
            }
            if !account.is_running() {
                if now < s.next_start {
                    continue;
                }
                match account.start(&launcher).await {
                    Ok(()) => {
                        s.backoff = Duration::from_secs(1);
                        s.next_check = Instant::now() + CHECK_INTERVAL;
                    }
                    Err(e) => {
                        tracing::warn!(
                            provider = %meta.provider,
                            alias = %meta.alias,
                            error = %e,
                            retry_in_secs = s.backoff.as_secs(),
                            "browser session failed to start"
                        );
                        s.next_start = Instant::now() + s.backoff;
                        s.backoff = (s.backoff * 2).min(MAX_BACKOFF);
                    }
                }
                continue;
            }
            let interval = match account.gate().state() {
                GateState::NeedsAttention | GateState::Starting | GateState::Busy => {
                    ATTENTION_CHECK_INTERVAL
                }
                _ => CHECK_INTERVAL,
            };
            if now >= s.next_check {
                account.check().await;
                s.next_check = Instant::now() + interval;
            } else if s.next_check > now + interval {
                s.next_check = now + interval;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_json_round_trips_with_defaults() {
        let empty = WebJson::parse(None);
        assert_eq!(empty.settings(), AccountSettings::default());
        let j = WebJson {
            mode: Some("stateless".into()),
            conversation_url: Some("https://chatgpt.com/c/abc-12345".into()),
            bound_by: Some("user".into()),
            min_interval_secs: Some(30),
            daily_soft_cap: Some(0),
        };
        let text = serde_json::to_string(&j).expect("json");
        assert!(!text.contains("cookie") && !text.contains("token"));
        let back = WebJson::parse(Some(&text));
        assert_eq!(back, j);
        let s = back.settings();
        assert_eq!(s.mode, ConversationMode::Stateless);
        assert_eq!(s.min_interval_secs, 30);
        assert_eq!(s.daily_soft_cap, 0);
        assert_eq!(WebJson::parse(Some("not json")), WebJson::default());
    }
}
