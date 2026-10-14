// SPDX-License-Identifier: Apache-2.0

//! Browser-session transport contract (ADR-014, ExecPlan 017 §5.2).
//!
//! A `WEB_SESSION` provider drives a site's own web page in a dedicated, unmodified browser.
//! The split:
//!
//! - [`WebSite`] (implemented by the provider module) holds everything site specific: URLs,
//!   how to read the page state, how to submit, which page request carries the answer and how
//!   to parse it;
//! - [`PageOps`] (implemented by the browser crate) is the narrow set of page operations a
//!   site may use;
//! - [`SessionUpstream`] (implemented by the browser crate) executes one request after route
//!   selection; [`SessionGate`] is the lock-free state the router reads before selecting.

use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU8, AtomicU32, Ordering};

use futures_util::Stream;
use loadout_protocol::{CanonicalEvent, CanonicalRequest};
use serde_json::Value;

use crate::{BoxFuture, CatalogModel, ProviderConfig, UpstreamFailure};

/// What the page currently shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageState {
    /// Signed in, composer empty, nothing generating.
    Ready,
    /// The sign-in page or a signed-out view.
    LoggedOut,
    /// An anti-bot or verification page; only the user may act on it (ADR-014 §2).
    Challenge,
    /// An answer is being generated, or the user is typing in the composer.
    Busy,
    /// Still loading or not recognised.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PageError {
    #[error("the browser connection is closed")]
    Closed,
    #[error("browser protocol error: {0}")]
    Protocol(String),
    #[error("timed out: {0}")]
    Timeout(&'static str),
    #[error("page element not found: {0}")]
    NotFound(&'static str),
}

/// Page operations available to a site module. Expressions and selectors are `'static`:
/// they come from the module's source, never from page content or request data.
pub trait PageOps: Send + Sync {
    /// Evaluates a fixed expression and returns its JSON value.
    fn eval(&self, expr: &'static str) -> BoxFuture<'_, Result<Value, PageError>>;

    /// Clicks the centre of the first visible element matching `selector`; `Ok(false)` if
    /// there is none.
    fn click(&self, selector: &'static str) -> BoxFuture<'_, Result<bool, PageError>>;

    /// Focuses the first element matching `selector`; `Ok(false)` if there is none.
    fn focus(&self, selector: &'static str) -> BoxFuture<'_, Result<bool, PageError>>;

    /// Inserts text at the caret as one input event, like an IME commit.
    fn insert_text<'a>(&'a self, text: &'a str) -> BoxFuture<'a, Result<(), PageError>>;

    /// Presses and releases a key: `Enter`, `Escape`.
    fn press_key(&self, key: &'static str) -> BoxFuture<'_, Result<(), PageError>>;
}

/// Turns the bytes of the page's answer stream into canonical events.
pub trait StreamParser: Send {
    fn feed(&mut self, bytes: &[u8]) -> Vec<CanonicalEvent>;

    /// Flushes buffered input after the stream ended.
    fn finish(&mut self) -> Vec<CanonicalEvent>;

    /// The site signalled the end of the answer.
    fn is_done(&self) -> bool;

    /// The conversation the answer belongs to, once known.
    fn conversation_id(&self) -> Option<&str>;

    /// The model that actually answered, once known.
    fn model(&self) -> Option<&str>;
}

/// Site knowledge of a `WEB_SESSION` provider module.
pub trait WebSite: Send + Sync {
    /// Site origin such as `https://chatgpt.com`; tests point it at a loopback mock.
    fn origin(&self, config: &ProviderConfig) -> String;

    /// URL that opens a new conversation with `model` (`temporary`: not saved in history).
    fn new_conversation_url(&self, origin: &str, model: &str, temporary: bool) -> String;

    /// URL of an existing conversation, selecting `model` for the next message.
    fn conversation_url(&self, origin: &str, conversation_id: &str, model: &str) -> String;

    /// The conversation id in a conversation URL of this site, if it is one.
    fn conversation_id(&self, origin: &str, url: &str) -> Option<String>;

    fn page_state<'a>(&'a self, page: &'a dyn PageOps) -> BoxFuture<'a, PageState>;

    /// Puts `text` into the composer and sends it.
    fn submit<'a>(
        &'a self,
        page: &'a dyn PageOps,
        text: &'a str,
    ) -> BoxFuture<'a, Result<(), PageError>>;

    /// Stops a running answer (the page's own stop control).
    fn stop<'a>(&'a self, page: &'a dyn PageOps) -> BoxFuture<'a, ()>;

    /// Whether a request the page sent carries the answer stream.
    fn is_answer_request(&self, method: &str, url: &str) -> bool;

    fn parser(&self) -> Box<dyn StreamParser>;

    /// Maps a non-2xx answer response.
    fn classify_response(&self, status: u16, body: &str) -> UpstreamFailure;

    /// Whether a page response lists the models (read passively while the page loads).
    fn is_models_response(&self, url: &str) -> bool;

    fn parse_models(&self, body: &str) -> Vec<CatalogModel>;

    /// Models to offer when the page's list has not been seen yet.
    fn fallback_models(&self) -> Vec<CatalogModel>;

    /// Longest text one message may carry.
    fn max_input_chars(&self) -> usize;
}

/// Lock-free gate state of a browser-session target (ExecPlan 017 §5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GateState {
    Stopped = 0,
    Starting = 1,
    Ready = 2,
    Busy = 3,
    NeedsAttention = 4,
    Cooling = 5,
}

impl GateState {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Starting,
            2 => Self::Ready,
            3 => Self::Busy,
            4 => Self::NeedsAttention,
            5 => Self::Cooling,
            _ => Self::Stopped,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Busy => "busy",
            Self::NeedsAttention => "needs_attention",
            Self::Cooling => "cooling",
        }
    }
}

const DAY_MS: i64 = 24 * 3600 * 1000;

/// Read by the router before selection (atomics only, Harness §2.2); written by the browser
/// session and its background checks.
#[derive(Debug)]
pub struct SessionGate {
    state: AtomicU8,
    /// Earliest time of the next request (minimum interval or cooldown), unix ms.
    next_allowed_ms: AtomicI64,
    /// UTC day number of `today_count`.
    day: AtomicI64,
    today_count: AtomicU32,
    min_interval_ms: AtomicI64,
    /// 0 = no cap.
    daily_cap: AtomicU32,
}

impl Default for SessionGate {
    fn default() -> Self {
        Self {
            state: AtomicU8::new(GateState::Stopped as u8),
            next_allowed_ms: AtomicI64::new(0),
            day: AtomicI64::new(0),
            today_count: AtomicU32::new(0),
            min_interval_ms: AtomicI64::new(0),
            daily_cap: AtomicU32::new(0),
        }
    }
}

impl SessionGate {
    pub fn state(&self) -> GateState {
        GateState::from_u8(self.state.load(Ordering::Acquire))
    }

    pub fn set_state(&self, s: GateState) {
        self.state.store(s as u8, Ordering::Release);
    }

    pub fn set_limits(&self, min_interval_ms: i64, daily_cap: u32) {
        self.min_interval_ms
            .store(min_interval_ms.max(0), Ordering::Relaxed);
        self.daily_cap.store(daily_cap, Ordering::Relaxed);
    }

    pub fn next_allowed_ms(&self) -> i64 {
        self.next_allowed_ms.load(Ordering::Acquire)
    }

    /// Requests counted today (UTC).
    pub fn today_count(&self, now_ms: i64) -> u32 {
        if self.day.load(Ordering::Acquire) == now_ms.div_euclid(DAY_MS) {
            self.today_count.load(Ordering::Acquire)
        } else {
            0
        }
    }

    /// Seeds today's count (daemon start).
    pub fn seed_count(&self, now_ms: i64, count: u32) {
        self.day.store(now_ms.div_euclid(DAY_MS), Ordering::Release);
        self.today_count.store(count, Ordering::Release);
    }

    /// Why the target cannot take a request now; `None` when it can.
    pub fn unavailable_reason(&self, now_ms: i64) -> Option<&'static str> {
        match self.state() {
            GateState::Ready => {}
            GateState::Cooling if now_ms >= self.next_allowed_ms() => {}
            GateState::Cooling => return Some("web session cooling down"),
            GateState::Busy => return Some("web session busy"),
            GateState::NeedsAttention => return Some("web session needs attention"),
            GateState::Starting => return Some("web session starting"),
            GateState::Stopped => return Some("web session not running"),
        }
        if now_ms < self.next_allowed_ms() {
            return Some("web session minimum interval");
        }
        let cap = self.daily_cap.load(Ordering::Relaxed);
        if cap > 0 && self.today_count(now_ms) >= cap {
            return Some("web session daily soft cap");
        }
        None
    }

    /// Counts a request and starts its minimum interval.
    pub fn begin_request(&self, now_ms: i64) {
        let day = now_ms.div_euclid(DAY_MS);
        if self.day.swap(day, Ordering::AcqRel) != day {
            self.today_count.store(0, Ordering::Release);
        }
        self.today_count.fetch_add(1, Ordering::AcqRel);
        let next = now_ms + self.min_interval_ms.load(Ordering::Relaxed);
        self.next_allowed_ms.fetch_max(next, Ordering::AcqRel);
        self.set_state(GateState::Busy);
    }

    /// Cooldown until `until_ms` (quota or rate-limit signal).
    pub fn cool_until(&self, until_ms: i64) {
        self.next_allowed_ms.fetch_max(until_ms, Ordering::AcqRel);
        self.set_state(GateState::Cooling);
    }
}

/// The answer as canonical events. Dropping it stops the answer on the page.
pub type SessionStream = Pin<Box<dyn Stream<Item = CanonicalEvent> + Send>>;

/// One request for a browser session.
#[derive(Debug)]
pub struct SessionRequest {
    pub request: CanonicalRequest,
    /// Upstream model id (an entry of the page's model list).
    pub model: String,
    /// Explicit conversation key (ADR-013 sources 1–2, already hashed per client and
    /// account); `None` uses the account's default conversation.
    pub conversation: Option<String>,
}

/// Why a browser session did not start an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    /// The page is in use (the user is typing, or an answer is running): try another target.
    Busy(&'static str),
    /// The request cannot be expressed on this site.
    Unsupported(String),
    Failed(UpstreamFailure),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy(why) => f.write_str(why),
            Self::Unsupported(why) => f.write_str(why),
            Self::Failed(e) => e.fmt(f),
        }
    }
}

/// Executes requests on one account's browser session (after route selection).
pub trait SessionUpstream: Send + Sync {
    fn execute(
        self: Arc<Self>,
        request: SessionRequest,
    ) -> BoxFuture<'static, Result<SessionStream, SessionError>>;
}

/// Attached to every route target of a browser-session account.
pub struct WebBinding {
    pub gate: Arc<SessionGate>,
    pub upstream: Arc<dyn SessionUpstream>,
}

impl fmt::Debug for WebBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebBinding")
            .field("gate", &self.gate.state())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_reasons_follow_state_interval_and_cap() {
        let g = SessionGate::default();
        assert_eq!(g.unavailable_reason(0), Some("web session not running"));
        g.set_state(GateState::Ready);
        g.set_limits(10_000, 2);
        let t0 = 5 * DAY_MS + 1000;
        assert_eq!(g.unavailable_reason(t0), None);

        g.begin_request(t0);
        assert_eq!(g.state(), GateState::Busy);
        g.set_state(GateState::Ready);
        assert_eq!(
            g.unavailable_reason(t0 + 5_000),
            Some("web session minimum interval")
        );
        assert_eq!(g.unavailable_reason(t0 + 10_000), None);

        g.begin_request(t0 + 10_000);
        g.set_state(GateState::Ready);
        assert_eq!(g.today_count(t0 + 20_000), 2);
        assert_eq!(
            g.unavailable_reason(t0 + 20_000),
            Some("web session daily soft cap")
        );
        // A new UTC day resets the count.
        assert_eq!(g.unavailable_reason(t0 + DAY_MS), None);
        g.begin_request(t0 + DAY_MS);
        assert_eq!(g.today_count(t0 + DAY_MS), 1);

        g.cool_until(t0 + 2 * DAY_MS);
        assert_eq!(
            g.unavailable_reason(t0 + DAY_MS + 60_000),
            Some("web session cooling down")
        );
        assert_eq!(g.unavailable_reason(t0 + 2 * DAY_MS), None);
    }
}
