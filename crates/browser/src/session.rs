// SPDX-License-Identifier: Apache-2.0

//! One account's browser session: start, state checks, and request execution
//! (ExecPlan 017 §5.1, §5.6).
//!
//! A request holds the session for its whole duration (one answer at a time). It types the
//! message like a user, then passively reads the page's own answer stream. Pre-answer
//! failures are returned so the gateway can fail over; once the answer request is under way
//! the events are streamed. Dropping the stream (client gone) presses the page's stop
//! control.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use loadout_platform::OwnedJob;
use loadout_protocol::{CanonicalEvent, StopReason};
use loadout_provider_sdk::web::{
    GateState, PageState, SessionError, SessionGate, SessionRequest, SessionStream,
    SessionUpstream, StreamParser, WebSite,
};
use loadout_provider_sdk::{
    BoxFuture, CatalogModel, FailureKind, ProviderConfig, ProviderModule, UpstreamFailure,
};
use serde_json::Value;
use tokio::sync::{OwnedMutexGuard, mpsc};

use crate::BrowserError;
use crate::cdp::Subscription;
use crate::launch::{Browser, LaunchOptions};
use crate::page::{Page, decode_b64};
use crate::plan::{self, ConversationMode};

const READY_WAIT: Duration = Duration::from_secs(20);
const BUSY_WAIT: Duration = Duration::from_secs(3);
const ANSWER_REQUEST_WAIT: Duration = Duration::from_secs(30);
const RESPONSE_WAIT: Duration = Duration::from_secs(120);
const STOP_WAIT: Duration = Duration::from_secs(30);
/// Longest silence of the page's answer stream once it started.
const ANSWER_IDLE: Duration = Duration::from_secs(600);
const MODELS_WAIT: Duration = Duration::from_secs(10);
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);
/// Consecutive failures after which the session needs the user (ExecPlan 017 §5.5).
const ATTENTION_AFTER: u32 = 3;
/// Cooldown when the site signals a limit without a reset time.
const DEFAULT_COOLDOWN: Duration = Duration::from_secs(3600);
const PUMP_BUFFER: usize = 64;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// How browsers are started (shared by all accounts).
#[derive(Clone, Debug)]
pub struct Launcher {
    pub edge: PathBuf,
    pub job: Arc<OwnedJob>,
    pub headless: bool,
}

/// Per-account settings (`accounts.web_session_json`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountSettings {
    pub mode: ConversationMode,
    /// URL of the conversation used when a request names none.
    pub default_conversation: Option<String>,
    pub min_interval_secs: u32,
    /// 0 = no cap.
    pub daily_soft_cap: u32,
}

impl Default for AccountSettings {
    fn default() -> Self {
        Self {
            mode: ConversationMode::Persistent,
            default_conversation: None,
            min_interval_secs: 10,
            daily_soft_cap: 150,
        }
    }
}

/// Notifications from a session to the daemon.
pub trait AccountEvents: Send + Sync {
    /// A new conversation became the account's default conversation (persist it).
    fn default_conversation(&self, account_id: &str, url: &str);
    /// The page needs the user: signed out or a verification page.
    fn needs_attention(&self, account_id: &str, reason: &str);
    /// The page listed its models.
    fn models(&self, account_id: &str, models: Vec<CatalogModel>);
}

/// Snapshot for status displays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebAccountStatus {
    pub state: GateState,
    pub running: bool,
    pub attention: Option<String>,
    pub next_allowed_ms: i64,
    pub today_count: u32,
}

struct Running {
    browser: Browser,
    page: Page,
    /// Model selected by the last navigation.
    model: Option<String>,
}

/// One account's browser session.
pub struct WebAccount {
    account_id: String,
    module: Arc<dyn ProviderModule>,
    origin: String,
    profile_dir: PathBuf,
    gate: Arc<SessionGate>,
    settings: Mutex<AccountSettings>,
    /// Explicit conversation key → conversation id (memory only, ExecPlan 017 §5.6).
    conversations: Mutex<HashMap<String, String>>,
    running: Arc<tokio::sync::Mutex<Option<Running>>>,
    events: Arc<dyn AccountEvents>,
    failures: AtomicU32,
    attention: Mutex<Option<String>>,
}

impl std::fmt::Debug for WebAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebAccount")
            .field("account_id", &self.account_id)
            .field("state", &self.gate.state())
            .finish_non_exhaustive()
    }
}

/// Restores the gate if a request ends before its answer started streaming (including when
/// the gateway drops the future on timeout).
struct GateGuard<'a> {
    account: &'a WebAccount,
    armed: bool,
}

impl Drop for GateGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.account.release_gate();
        }
    }
}

struct Started {
    request_id: String,
    buffered: Vec<u8>,
    net: Subscription,
}

enum Open {
    Existing(String),
    New { temporary: bool },
}

enum Key {
    Explicit(String),
    Default,
    Stateless,
}

struct Plan {
    open: Open,
    key: Key,
    text: String,
}

fn failed(kind: FailureKind, message: impl AsRef<str>) -> SessionError {
    SessionError::Failed(UpstreamFailure::new(kind, None, message))
}

impl WebAccount {
    pub fn new(
        account_id: &str,
        module: Arc<dyn ProviderModule>,
        config: &ProviderConfig,
        profile_dir: PathBuf,
        settings: AccountSettings,
        events: Arc<dyn AccountEvents>,
    ) -> Result<Arc<Self>, BrowserError> {
        let origin = module
            .web_site()
            .ok_or_else(|| BrowserError::NotWebProvider(config.slug.clone()))?
            .origin(config);
        let gate = Arc::new(SessionGate::default());
        gate.set_limits(
            i64::from(settings.min_interval_secs) * 1000,
            settings.daily_soft_cap,
        );
        Ok(Arc::new(Self {
            account_id: account_id.to_owned(),
            module,
            origin,
            profile_dir,
            gate,
            settings: Mutex::new(settings),
            conversations: Mutex::default(),
            running: Arc::default(),
            events,
            failures: AtomicU32::new(0),
            attention: Mutex::default(),
        }))
    }

    fn site(&self) -> &dyn WebSite {
        self.module.web_site().expect("checked in WebAccount::new")
    }

    pub fn account_id(&self) -> &str {
        &self.account_id
    }

    pub fn gate(&self) -> &Arc<SessionGate> {
        &self.gate
    }

    pub fn profile_dir(&self) -> &std::path::Path {
        &self.profile_dir
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }

    pub fn settings(&self) -> AccountSettings {
        self.settings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn set_settings(&self, s: AccountSettings) {
        self.gate
            .set_limits(i64::from(s.min_interval_secs) * 1000, s.daily_soft_cap);
        *self.settings.lock().unwrap_or_else(PoisonError::into_inner) = s;
    }

    /// Whether `url` is a conversation of this site (for binding a default conversation).
    pub fn is_conversation_url(&self, url: &str) -> bool {
        self.site().conversation_id(&self.origin, url).is_some()
    }

    pub fn status(&self) -> WebAccountStatus {
        let now = now_ms();
        let running = match self.running.try_lock() {
            Ok(r) => r.as_ref().is_some_and(|r| r.browser.is_alive()),
            Err(_) => true,
        };
        WebAccountStatus {
            state: self.gate.state(),
            running,
            attention: self
                .attention
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
            next_allowed_ms: self.gate.next_allowed_ms(),
            today_count: self.gate.today_count(now),
        }
    }

    fn set_attention(&self, reason: &str) {
        self.gate.set_state(GateState::NeedsAttention);
        let mut a = self
            .attention
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if a.as_deref() != Some(reason) {
            *a = Some(reason.to_owned());
            drop(a);
            tracing::warn!(account = %self.account_id, reason, "web session needs attention");
            self.events.needs_attention(&self.account_id, reason);
        }
    }

    /// Back to Ready after a request, unless a cooldown or attention state was set.
    fn release_gate(&self) {
        if self.gate.state() == GateState::Busy {
            self.gate.set_state(GateState::Ready);
        }
    }

    fn apply_page_state(&self, state: PageState) {
        match state {
            PageState::Ready => {
                *self
                    .attention
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = None;
                self.failures.store(0, Ordering::Relaxed);
                match self.gate.state() {
                    GateState::Cooling if now_ms() < self.gate.next_allowed_ms() => {}
                    _ => self.gate.set_state(GateState::Ready),
                }
            }
            PageState::Busy => self.gate.set_state(GateState::Busy),
            PageState::LoggedOut => {
                self.set_attention("signed out: sign in in the Loadout browser window")
            }
            PageState::Challenge => {
                self.set_attention("verification page: complete it in the Loadout browser window")
            }
            PageState::Unknown => {}
        }
    }

    /// Starts the browser if it is not running and reads the page state.
    pub async fn start(&self, launcher: &Launcher) -> Result<(), BrowserError> {
        let mut running = self.running.lock().await;
        if running.as_ref().is_some_and(|r| r.browser.is_alive()) {
            return Ok(());
        }
        if let Some(old) = running.take() {
            old.browser.shutdown(Duration::from_millis(500)).await;
        }
        self.gate.set_state(GateState::Starting);
        let opts = LaunchOptions {
            edge: launcher.edge.clone(),
            profile_dir: self.profile_dir.clone(),
            headless: launcher.headless,
            start_url: "about:blank".into(),
        };
        let result = async {
            let browser = Browser::launch(&opts, &launcher.job).await?;
            let page = browser.first_page("about:blank").await?;
            Ok::<_, BrowserError>((browser, page))
        }
        .await;
        let (browser, page) = match result {
            Ok(v) => v,
            Err(e) => {
                self.gate.set_state(GateState::Stopped);
                return Err(e);
            }
        };
        let url = self
            .site()
            .new_conversation_url(&self.origin, "auto", false);
        let models = self.navigate(&page, &url, MODELS_WAIT).await;
        if let Ok(Some(models)) = models {
            self.events.models(&self.account_id, models);
        }
        let state = self.site().page_state(&page).await;
        tracing::info!(account = %self.account_id, pid = browser.pid(), ?state, "web session started");
        *running = Some(Running {
            browser,
            page,
            model: None,
        });
        drop(running);
        if state == PageState::Unknown {
            self.gate.set_state(GateState::Starting);
        }
        self.apply_page_state(state);
        Ok(())
    }

    /// Re-reads the page state (background check). Skipped while a request runs.
    pub async fn check(&self) -> Option<PageState> {
        let Ok(running) = self.running.try_lock() else {
            return None;
        };
        let Some(r) = running.as_ref() else {
            self.gate.set_state(GateState::Stopped);
            return None;
        };
        if !r.browser.is_alive() {
            self.gate.set_state(GateState::Stopped);
            return None;
        }
        let state = self.site().page_state(&r.page).await;
        drop(running);
        self.apply_page_state(state);
        Some(state)
    }

    pub fn is_running(&self) -> bool {
        match self.running.try_lock() {
            Ok(r) => r.as_ref().is_some_and(|r| r.browser.is_alive()),
            Err(_) => true,
        }
    }

    /// Brings the account's window to the front.
    pub async fn show(&self) -> Result<(), BrowserError> {
        let running = self.running.lock().await;
        match running.as_ref() {
            Some(r) => r.page.show().await,
            None => Err(BrowserError::Closed),
        }
    }

    /// Closes the browser (waits for a running request).
    pub async fn stop(&self) {
        let mut running = self.running.lock().await;
        if let Some(r) = running.take() {
            r.browser.shutdown(SHUTDOWN_GRACE).await;
            tracing::info!(account = %self.account_id, "web session stopped");
        }
        self.gate.set_state(GateState::Stopped);
    }

    /// Navigates, reading the page's model list if it loads one within `models_wait`.
    async fn navigate(
        &self,
        page: &Page,
        url: &str,
        models_wait: Duration,
    ) -> Result<Option<Vec<CatalogModel>>, BrowserError> {
        let mut net = page.network();
        page.navigate(url).await?;
        let site = self.site();
        let deadline = tokio::time::Instant::now() + models_wait;
        let mut candidates: Vec<String> = Vec::new();
        loop {
            let ev = match tokio::time::timeout_at(deadline, net.next()).await {
                Ok(Some(ev)) => ev,
                Ok(None) => return Err(BrowserError::Closed),
                Err(_) => return Ok(None),
            };
            let id = ev
                .params
                .get("requestId")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match ev.method.as_str() {
                "Network.responseReceived" => {
                    let url = ev
                        .params
                        .pointer("/response/url")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let status = ev
                        .params
                        .pointer("/response/status")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    if status == 200 && site.is_models_response(url) {
                        candidates.push(id.to_owned());
                    }
                }
                "Network.loadingFinished" if candidates.iter().any(|c| c == id) => {
                    let body = page.response_body(id).await?;
                    let models = site.parse_models(&body);
                    if !models.is_empty() {
                        return Ok(Some(models));
                    }
                }
                _ => {}
            }
        }
    }

    fn plan(&self, req: &SessionRequest) -> Result<Plan, SessionError> {
        let site = self.site();
        let settings = self.settings();
        if let Some(p) = plan::unsupported_parameter(&req.request) {
            return Err(SessionError::Unsupported(format!(
                "web sessions cannot honour the {p} parameter"
            )));
        }
        let plan = match settings.mode {
            ConversationMode::Stateless => Plan {
                open: Open::New { temporary: true },
                key: Key::Stateless,
                text: plan::flatten(&req.request).map_err(SessionError::Unsupported)?,
            },
            ConversationMode::Persistent => {
                let (existing, key) = match &req.conversation {
                    Some(k) => (
                        self.conversations
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .get(k)
                            .cloned(),
                        Key::Explicit(k.clone()),
                    ),
                    None => (
                        settings
                            .default_conversation
                            .as_deref()
                            .and_then(|u| site.conversation_id(&self.origin, u)),
                        Key::Default,
                    ),
                };
                match existing {
                    Some(id) => Plan {
                        open: Open::Existing(id),
                        key,
                        text: plan::last_user_text(&req.request)
                            .map_err(SessionError::Unsupported)?,
                    },
                    // A new conversation gets the whole history, so nothing is lost when a
                    // client continues a conversation this daemon has not seen.
                    None => Plan {
                        open: Open::New { temporary: false },
                        key,
                        text: plan::flatten(&req.request).map_err(SessionError::Unsupported)?,
                    },
                }
            }
        };
        if plan.text.trim().is_empty() {
            return Err(SessionError::Unsupported("the message is empty".into()));
        }
        if plan.text.chars().count() > site.max_input_chars() {
            return Err(SessionError::Unsupported(format!(
                "the message is longer than the {} characters the page accepts",
                site.max_input_chars()
            )));
        }
        Ok(plan)
    }

    /// Opens the right conversation, waits for the page, types the message and waits for
    /// the page's answer request to start.
    async fn submit(
        &self,
        running: &mut Running,
        plan: &Plan,
        model: &str,
    ) -> Result<Started, SessionError> {
        let site = self.site();
        let page = running.page.clone();
        let current = page
            .url()
            .await
            .map_err(|e| failed(FailureKind::Connect, e.to_string()))?;
        let (navigate_to, needed) = match &plan.open {
            Open::New { temporary } => (
                site.new_conversation_url(&self.origin, model, *temporary),
                true,
            ),
            Open::Existing(id) => (
                site.conversation_url(&self.origin, id, model),
                site.conversation_id(&self.origin, &current).as_deref() != Some(id.as_str())
                    || running.model.as_deref() != Some(model),
            ),
        };
        if needed {
            match self.navigate(&page, &navigate_to, Duration::ZERO).await {
                Ok(_) => running.model = Some(model.to_owned()),
                Err(e) => return Err(failed(FailureKind::Connect, e.to_string())),
            }
        }

        // Wait for an idle, signed-in page.
        let started = tokio::time::Instant::now();
        loop {
            let state = site.page_state(&page).await;
            match state {
                PageState::Ready => break,
                PageState::LoggedOut | PageState::Challenge => {
                    self.apply_page_state(state);
                    let why = self
                        .attention
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .clone()
                        .unwrap_or_default();
                    return Err(failed(FailureKind::Auth, why));
                }
                PageState::Busy if started.elapsed() > BUSY_WAIT => {
                    return Err(SessionError::Busy(
                        "the page is in use (an answer is running or text is being typed)",
                    ));
                }
                PageState::Busy | PageState::Unknown => {}
            }
            if started.elapsed() > READY_WAIT {
                return Err(failed(FailureKind::Server, "the page did not become ready"));
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }

        let mut net = page.network();
        site.submit(&page, &plan.text).await.map_err(|e| {
            failed(
                FailureKind::Server,
                format!("could not send the message: {e}"),
            )
        })?;

        // The page's own answer request.
        let deadline = tokio::time::Instant::now() + ANSWER_REQUEST_WAIT;
        let request_id = loop {
            let ev = match tokio::time::timeout_at(deadline, net.next()).await {
                Ok(Some(ev)) => ev,
                Ok(None) => return Err(failed(FailureKind::Connect, "the browser closed")),
                Err(_) => {
                    return Err(failed(
                        FailureKind::Server,
                        "the page did not send the message (no answer request)",
                    ));
                }
            };
            if ev.method == "Network.requestWillBeSent" {
                let method = ev
                    .params
                    .pointer("/request/method")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let url = ev
                    .params
                    .pointer("/request/url")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if site.is_answer_request(method, url)
                    && let Some(id) = ev.params.get("requestId").and_then(Value::as_str)
                {
                    break id.to_owned();
                }
            }
        };

        let deadline = tokio::time::Instant::now() + RESPONSE_WAIT;
        let status = loop {
            let ev = match tokio::time::timeout_at(deadline, net.next()).await {
                Ok(Some(ev)) => ev,
                Ok(None) => return Err(failed(FailureKind::Connect, "the browser closed")),
                Err(_) => return Err(failed(FailureKind::Timeout, "no answer from the site")),
            };
            if ev.params.get("requestId").and_then(Value::as_str) != Some(request_id.as_str()) {
                continue;
            }
            match ev.method.as_str() {
                "Network.responseReceived" => {
                    break ev
                        .params
                        .pointer("/response/status")
                        .and_then(Value::as_u64)
                        .and_then(|s| u16::try_from(s).ok())
                        .unwrap_or(0);
                }
                "Network.loadingFailed" => {
                    let text = ev
                        .params
                        .get("errorText")
                        .and_then(Value::as_str)
                        .unwrap_or("request failed");
                    return Err(failed(FailureKind::Connect, text));
                }
                _ => {}
            }
        };
        if !(200..300).contains(&status) {
            // Read the error body once the response finished.
            let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
            while let Ok(Some(ev)) = tokio::time::timeout_at(deadline, net.next()).await {
                if ev.method == "Network.loadingFinished"
                    && ev.params.get("requestId").and_then(Value::as_str)
                        == Some(request_id.as_str())
                {
                    break;
                }
            }
            let body = page.response_body(&request_id).await.unwrap_or_default();
            let failure = site.classify_response(status, &body);
            self.note_failure_signal(&failure);
            return Err(SessionError::Failed(failure));
        }
        let buffered = page
            .stream_body(&request_id)
            .await
            .map_err(|e| failed(FailureKind::Server, format!("cannot read the answer: {e}")))?;
        Ok(Started {
            request_id,
            buffered,
            net,
        })
    }

    /// Quota and rate-limit signals cool the session down (ADR-014 §7).
    fn note_failure_signal(&self, f: &UpstreamFailure) {
        let wait = match f.kind {
            FailureKind::QuotaExhausted { reset_after } => {
                Some(reset_after.unwrap_or(DEFAULT_COOLDOWN))
            }
            FailureKind::RateLimited { retry_after } => {
                Some(retry_after.unwrap_or(Duration::from_secs(60)))
            }
            FailureKind::Auth | FailureKind::AccountDisabled => {
                self.set_attention(
                    "the site rejected the session: check the Loadout browser window",
                );
                None
            }
            _ => None,
        };
        if let Some(w) = wait {
            let until = now_ms() + i64::try_from(w.as_millis()).unwrap_or(i64::MAX / 2);
            self.gate.cool_until(until);
            tracing::info!(account = %self.account_id, until, "web session cooling down");
        }
    }

    /// Limit messages inside the answer stream cool the session down like a 429 does.
    fn note_stream_errors(&self, events: &[CanonicalEvent]) {
        for e in events {
            if let CanonicalEvent::Error {
                message,
                kind: Some(kind),
            } = e
                && kind.contains("rate_limit")
            {
                let m = message.to_ascii_lowercase();
                let failure_kind = if m.contains("usage") || m.contains("cap") {
                    FailureKind::QuotaExhausted { reset_after: None }
                } else {
                    FailureKind::RateLimited { retry_after: None }
                };
                self.note_failure_signal(&UpstreamFailure::new(failure_kind, None, message));
            }
        }
    }

    fn count_failure(&self) {
        if self.failures.fetch_add(1, Ordering::AcqRel) + 1 >= ATTENTION_AFTER {
            self.set_attention("repeated failures: check the Loadout browser window");
        }
    }

    async fn run(self: Arc<Self>, req: SessionRequest) -> Result<SessionStream, SessionError> {
        let plan = self.plan(&req)?;
        let mut guard: OwnedMutexGuard<Option<Running>> = self
            .running
            .clone()
            .try_lock_owned()
            .map_err(|_| SessionError::Busy("web session busy"))?;
        let alive = guard.as_ref().is_some_and(|r| r.browser.is_alive());
        if !alive {
            *guard = None;
            self.gate.set_state(GateState::Stopped);
            return Err(failed(FailureKind::Connect, "the browser is not running"));
        }
        self.gate.begin_request(now_ms());
        let mut gate_guard = GateGuard {
            account: &self,
            armed: true,
        };
        let running = guard.as_mut().expect("checked alive");
        let started = match self.submit(running, &plan, &req.model).await {
            Ok(s) => s,
            Err(e) => {
                if matches!(&e, SessionError::Failed(f) if matches!(f.kind, FailureKind::Server | FailureKind::Timeout | FailureKind::Connect))
                {
                    self.count_failure();
                }
                return Err(e);
            }
        };
        gate_guard.armed = false;
        drop(gate_guard);

        let (tx, rx) = mpsc::channel::<CanonicalEvent>(PUMP_BUFFER);
        let account = self.clone();
        tokio::spawn(async move {
            account.pump(guard, plan, started, tx).await;
        });
        Ok(Box::pin(futures_util::stream::unfold(
            rx,
            |mut rx| async move { rx.recv().await.map(|e| (e, rx)) },
        )))
    }

    async fn pump(
        self: Arc<Self>,
        mut guard: OwnedMutexGuard<Option<Running>>,
        plan: Plan,
        started: Started,
        tx: mpsc::Sender<CanonicalEvent>,
    ) {
        let site = self.site();
        let mut parser: Box<dyn StreamParser> = site.parser();
        let Started {
            request_id,
            buffered,
            mut net,
        } = started;
        let mut client_gone = false;
        let mut failed_stream = false;
        let mut stalled = false;
        let mut saw_finish = false;

        let first = parser.feed(&buffered);
        self.note_stream_errors(&first);
        if !send_all(&tx, first, &mut saw_finish).await {
            client_gone = true;
        }
        while !client_gone && !parser.is_done() {
            let ev = tokio::select! {
                ev = tokio::time::timeout(ANSWER_IDLE, net.next()) => match ev {
                    Ok(ev) => ev,
                    Err(_) => {
                        // Nothing from the page for too long: give up and stop the answer.
                        failed_stream = true;
                        stalled = true;
                        let _ = tx
                            .send(CanonicalEvent::Error {
                                message: "the answer stalled on the web page".into(),
                                kind: Some("upstream_error".into()),
                            })
                            .await;
                        break;
                    }
                },
                () = tx.closed() => { client_gone = true; break; }
            };
            let Some(ev) = ev else {
                failed_stream = true;
                let _ = tx
                    .send(CanonicalEvent::Error {
                        message: "the browser closed during the answer".into(),
                        kind: Some("upstream_error".into()),
                    })
                    .await;
                break;
            };
            if ev.params.get("requestId").and_then(Value::as_str) != Some(request_id.as_str()) {
                continue;
            }
            match ev.method.as_str() {
                "Network.dataReceived" => {
                    let Ok(bytes) = decode_b64(ev.params.get("data")) else {
                        continue;
                    };
                    if bytes.is_empty() {
                        continue;
                    }
                    let events = parser.feed(&bytes);
                    self.note_stream_errors(&events);
                    if !send_all(&tx, events, &mut saw_finish).await {
                        client_gone = true;
                    }
                }
                "Network.loadingFinished" => break,
                "Network.loadingFailed" => {
                    failed_stream = true;
                    let _ = tx
                        .send(CanonicalEvent::Error {
                            message: "the answer stream was interrupted".into(),
                            kind: Some("upstream_error".into()),
                        })
                        .await;
                    break;
                }
                _ => {}
            }
        }
        if !client_gone && !failed_stream {
            let tail = parser.finish();
            self.note_stream_errors(&tail);
            if send_all(&tx, tail, &mut saw_finish).await && !saw_finish {
                let _ = tx.send(CanonicalEvent::Finish(StopReason::EndTurn)).await;
            }
        }
        drop(tx);

        let running = guard.as_mut();
        if let Some(r) = running {
            if client_gone || stalled {
                tracing::info!(account = %self.account_id, client_gone, stalled, "stopping the answer");
                site.stop(&r.page).await;
                let deadline = tokio::time::Instant::now() + STOP_WAIT;
                while site.page_state(&r.page).await == PageState::Busy
                    && tokio::time::Instant::now() < deadline
                {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
            self.remember_conversation(&r.page, &plan, parser.conversation_id())
                .await;
        }
        if failed_stream {
            self.count_failure();
        } else {
            self.failures.store(0, Ordering::Relaxed);
        }
        if let Some(m) = parser.model() {
            tracing::debug!(account = %self.account_id, model = m, "answer model");
        }
        drop(guard);
        self.release_gate();
    }

    async fn remember_conversation(&self, page: &Page, plan: &Plan, from_stream: Option<&str>) {
        if !matches!(plan.open, Open::New { temporary: false }) {
            return;
        }
        let site = self.site();
        let id = match from_stream {
            Some(id) => Some(id.to_owned()),
            None => page
                .url()
                .await
                .ok()
                .and_then(|u| site.conversation_id(&self.origin, &u)),
        };
        let Some(id) = id else {
            tracing::warn!(account = %self.account_id, "could not tell which conversation the answer opened");
            return;
        };
        match &plan.key {
            Key::Explicit(k) => {
                self.conversations
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(k.clone(), id);
            }
            Key::Default => {
                let url = site.conversation_url(&self.origin, &id, "auto");
                let mut s = self.settings.lock().unwrap_or_else(PoisonError::into_inner);
                if s.default_conversation.is_none() {
                    s.default_conversation = Some(url.clone());
                    drop(s);
                    self.events.default_conversation(&self.account_id, &url);
                }
            }
            Key::Stateless => {}
        }
    }
}

/// Sends events in order; `false` once the receiver is gone.
async fn send_all(
    tx: &mpsc::Sender<CanonicalEvent>,
    events: Vec<CanonicalEvent>,
    saw_finish: &mut bool,
) -> bool {
    for e in events {
        *saw_finish |= matches!(e, CanonicalEvent::Finish(_));
        if tx.send(e).await.is_err() {
            return false;
        }
    }
    true
}

impl SessionUpstream for WebAccount {
    fn execute(
        self: Arc<Self>,
        request: SessionRequest,
    ) -> BoxFuture<'static, Result<SessionStream, SessionError>> {
        Box::pin(self.run(request))
    }
}
