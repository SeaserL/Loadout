// SPDX-License-Identifier: Apache-2.0

//! ChatGPT web page as a browser-session transport (ADR-014, ExecPlan 017 §5.6). Opt-in per
//! account; trust `WEB_SESSION` (tier D).
//!
//! Site knowledge only: URLs, how to read the page state, how to send a message, which page
//! request carries the answer and how to read its stream. Everything is done the way a user
//! would in an unmodified browser; the page's own scripts make every request (ADR-014 §2).
//! The answer-stream format (`delta_encoding` v1 patches and the older cumulative messages)
//! is knowledge of what the page receives; no code copied — see docs/providers/web-chatgpt.md.
//!
//! The page structure was not verified against a signed-in account when this was written;
//! the owner's live test (ExecPlan 017 Stage 8) confirms or corrects the selectors and the
//! stream format.

use std::collections::HashMap;
use std::time::Duration;

use loadout_protocol::sse::SseParser;
use loadout_protocol::{CanonicalEvent, Protocol, StopReason};
use loadout_provider_sdk::web::{PageError, PageOps, PageState, StreamParser, WebSite};
use loadout_provider_sdk::{
    AuthMethod, BoxFuture, Capabilities, CatalogModel, Drivers, Endpoint, FailureKind,
    ProviderConfig, ProviderDescriptor, ProviderError, ProviderModule, TransportSpec, TrustLevel,
    UpstreamFailure, classify, is_loopback_url,
};
use loadout_secrets::Secret;
use reqwest::header::HeaderMap;
use serde_json::Value;

const DEFAULT_ORIGIN: &str = "https://chatgpt.com";
/// Model id meaning "whatever the page selects by default".
const AUTO_MODEL: &str = "auto";

static DESCRIPTOR: ProviderDescriptor = ProviderDescriptor {
    kind: "web-chatgpt",
    display_name: "ChatGPT (web session, opt-in)",
    fixed_slug: Some("web-chatgpt"),
    default_base_url: Some(DEFAULT_ORIGIN),
    auth: AuthMethod::BrowserLogin,
    transports: &[TransportSpec {
        protocol: Protocol::Chat,
        trust: TrustLevel::WebSession,
    }],
    drivers: Drivers {
        discovery: false,
        auth: true,
        usage: false,
        catalog: true,
        transport: true,
        health: true,
    },
    capabilities: Capabilities::STREAMING,
    default_model_protocols: &[Protocol::Chat],
};

const COMPOSER: &str = "#prompt-textarea";
const SEND_BUTTON: &str = "[data-testid=\"send-button\"]";
const STOP_BUTTON: &str = "[data-testid=\"stop-button\"]";

/// One read-only expression classifying the page (ExecPlan 017 §10: fixed, never built from
/// page or request data).
const PAGE_STATE_JS: &str = r#"(()=>{
const t=document.title||'';
if(/just a moment|attention required|verify you are human|one more step/i.test(t)
 ||document.querySelector('#challenge-form,#cf-challenge-running,iframe[src*="challenges.cloudflare.com"]'))return 'challenge';
if(location.pathname.startsWith('/auth')||document.querySelector('[data-testid="login-button"]'))return 'logged_out';
const c=document.querySelector('#prompt-textarea');
if(!c)return 'unknown';
if(document.querySelector('[data-testid="stop-button"]'))return 'busy';
const v=(c.value!==undefined?c.value:c.innerText)||'';
return v.trim().length>0?'busy':'ready';
})()"#;

const COMPOSER_LENGTH_JS: &str = r#"(()=>{const c=document.querySelector('#prompt-textarea');
if(!c)return -1;return ((c.value!==undefined?c.value:c.innerText)||'').trim().length;})()"#;

#[derive(Debug, Default)]
pub struct WebChatGpt;

fn path_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let path = rest.find('/').map_or("", |i| &rest[i..]);
    path.split(['?', '#']).next().unwrap_or("")
}

fn with_query(base: String, params: &[(&str, &str)]) -> String {
    let q: Vec<String> = params.iter().map(|(k, v)| format!("{k}={v}")).collect();
    if q.is_empty() {
        base
    } else {
        format!("{base}?{}", q.join("&"))
    }
}

/// Model slugs are typed into a URL query: keep them to a safe character set.
fn model_param(model: &str) -> Option<&str> {
    (model != AUTO_MODEL
        && !model.is_empty()
        && model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
    .then_some(model)
}

impl WebSite for WebChatGpt {
    fn origin(&self, config: &ProviderConfig) -> String {
        config
            .base_url
            .as_deref()
            .unwrap_or(DEFAULT_ORIGIN)
            .trim_end_matches('/')
            .to_owned()
    }

    fn new_conversation_url(&self, origin: &str, model: &str, temporary: bool) -> String {
        let mut params = Vec::new();
        if let Some(m) = model_param(model) {
            params.push(("model", m));
        }
        if temporary {
            params.push(("temporary-chat", "true"));
        }
        with_query(format!("{origin}/"), &params)
    }

    fn conversation_url(&self, origin: &str, conversation_id: &str, model: &str) -> String {
        let params: Vec<(&str, &str)> = model_param(model)
            .map(|m| ("model", m))
            .into_iter()
            .collect();
        with_query(format!("{origin}/c/{conversation_id}"), &params)
    }

    fn conversation_id(&self, origin: &str, url: &str) -> Option<String> {
        if !url.starts_with(origin) {
            return None;
        }
        let path = path_of(url);
        // `/c/<id>`, also inside projects and GPTs (`/g/<gpt>/c/<id>`).
        let id = path.split("/c/").nth(1)?.split('/').next()?;
        (id.len() >= 8 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
            .then(|| id.to_owned())
    }

    fn page_state<'a>(&'a self, page: &'a dyn PageOps) -> BoxFuture<'a, PageState> {
        Box::pin(async move {
            match page.eval(PAGE_STATE_JS).await {
                Ok(Value::String(s)) => match s.as_str() {
                    "ready" => PageState::Ready,
                    "busy" => PageState::Busy,
                    "logged_out" => PageState::LoggedOut,
                    "challenge" => PageState::Challenge,
                    _ => PageState::Unknown,
                },
                _ => PageState::Unknown,
            }
        })
    }

    fn submit<'a>(
        &'a self,
        page: &'a dyn PageOps,
        text: &'a str,
    ) -> BoxFuture<'a, Result<(), PageError>> {
        Box::pin(async move {
            if !page.focus(COMPOSER).await? {
                return Err(PageError::NotFound("composer"));
            }
            page.insert_text(text).await?;
            let typed = page.eval(COMPOSER_LENGTH_JS).await?.as_i64().unwrap_or(-1);
            if typed <= 0 {
                return Err(PageError::NotFound("text in the composer"));
            }
            // The send button appears once the composer has text.
            for _ in 0..20 {
                if page.click(SEND_BUTTON).await? {
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
            page.press_key("Enter").await
        })
    }

    fn stop<'a>(&'a self, page: &'a dyn PageOps) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !matches!(page.click(STOP_BUTTON).await, Ok(true)) {
                let _ = page.press_key("Escape").await;
            }
        })
    }

    fn is_answer_request(&self, method: &str, url: &str) -> bool {
        let path = path_of(url);
        method.eq_ignore_ascii_case("POST")
            && (path.ends_with("/backend-api/conversation")
                || path.ends_with("/backend-api/f/conversation"))
    }

    fn parser(&self) -> Box<dyn StreamParser> {
        Box::new(ChatGptStream::default())
    }

    fn classify_response(&self, status: u16, body: &str) -> UpstreamFailure {
        let detail: Option<Value> = serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| v.get("detail").cloned());
        let code = detail
            .as_ref()
            .and_then(|d| d.get("code"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let message = detail
            .as_ref()
            .and_then(|d| match d {
                Value::String(s) => Some(s.as_str()),
                _ => d.get("message").and_then(Value::as_str),
            })
            .unwrap_or(body);
        let clears_in = detail
            .as_ref()
            .and_then(|d| d.get("clears_in"))
            .and_then(Value::as_f64)
            .filter(|s| s.is_finite() && *s > 0.0)
            .map(Duration::from_secs_f64);
        match status {
            401 | 403 => UpstreamFailure::new(
                FailureKind::Auth,
                Some(status),
                format!("the site rejected the session ({status}): {message}"),
            ),
            429 if code.contains("cap") || clears_in.is_some() => UpstreamFailure::new(
                FailureKind::QuotaExhausted {
                    reset_after: clears_in,
                },
                Some(status),
                message,
            ),
            _ => {
                let mut f = classify(status, &HeaderMap::new(), body);
                f.message = UpstreamFailure::new(f.kind, None, message).message;
                f
            }
        }
    }

    fn is_models_response(&self, url: &str) -> bool {
        path_of(url).ends_with("/backend-api/models")
    }

    fn parse_models(&self, body: &str) -> Vec<CatalogModel> {
        let Ok(v) = serde_json::from_str::<Value>(body) else {
            return Vec::new();
        };
        let mut out: Vec<CatalogModel> = v
            .get("models")
            .and_then(Value::as_array)
            .map(|models| {
                models
                    .iter()
                    .filter_map(|m| {
                        let slug = m.get("slug").and_then(Value::as_str)?;
                        model_param(slug).or((slug == AUTO_MODEL).then_some(slug))?;
                        let mut c = CatalogModel::new(slug);
                        c.display_name = m.get("title").and_then(Value::as_str).map(str::to_owned);
                        c.capabilities = Capabilities::STREAMING;
                        Some(c)
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !out.is_empty() && !out.iter().any(|m| m.upstream_id == AUTO_MODEL) {
            out.insert(0, auto_model());
        }
        out
    }

    fn fallback_models(&self) -> Vec<CatalogModel> {
        vec![auto_model()]
    }

    fn max_input_chars(&self) -> usize {
        60_000
    }
}

fn auto_model() -> CatalogModel {
    let mut m = CatalogModel::new(AUTO_MODEL);
    m.display_name = Some("Page default".into());
    m.capabilities = Capabilities::STREAMING;
    m
}

impl ProviderModule for WebChatGpt {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        &DESCRIPTOR
    }

    /// The default origin, or a loopback mock (tests).
    fn validate(&self, config: &ProviderConfig) -> Result<(), ProviderError> {
        match config.base_url.as_deref() {
            None => Ok(()),
            Some(u) if u.trim_end_matches('/') == DEFAULT_ORIGIN => Ok(()),
            Some(u) if reqwest::Url::parse(u).is_ok_and(|url| is_loopback_url(&url)) => Ok(()),
            Some(_) => Err(ProviderError::Config(
                "web-chatgpt only connects to https://chatgpt.com".into(),
            )),
        }
    }

    /// Not an HTTP upstream: the endpoint only records the site origin.
    fn endpoint(
        &self,
        config: &ProviderConfig,
        _protocol: Protocol,
        _key: Option<&Secret>,
    ) -> Result<Endpoint, ProviderError> {
        Ok(Endpoint::new(self.origin(config)))
    }

    fn account_concurrency(&self) -> Option<u32> {
        Some(1)
    }

    fn web_site(&self) -> Option<&dyn WebSite> {
        Some(self)
    }

    /// Reasoning models on the web page may think for minutes before the first word.
    fn first_token_timeout(&self) -> Option<Duration> {
        Some(Duration::from_secs(600))
    }

    fn active_probe(&self) -> bool {
        false
    }
}

#[derive(Debug, Default)]
struct MessageInfo {
    answer: bool,
    /// Text already emitted (for cumulative updates).
    emitted: String,
}

/// Parser of the page's answer stream.
#[derive(Debug, Default)]
struct ChatGptStream {
    sse: SseParser,
    messages: HashMap<i64, MessageInfo>,
    /// Legacy format: message id → index.
    legacy_ids: HashMap<String, i64>,
    current: i64,
    last_path: String,
    last_op: String,
    started: bool,
    done: bool,
    finished: bool,
    errored: bool,
    conversation_id: Option<String>,
    model: Option<String>,
}

fn is_answer_message(m: &Value) -> bool {
    let role = m.pointer("/author/role").and_then(Value::as_str);
    let content_type = m.pointer("/content/content_type").and_then(Value::as_str);
    let recipient = m.get("recipient").and_then(Value::as_str);
    let channel = m.get("channel").and_then(Value::as_str);
    role == Some("assistant")
        && content_type == Some("text")
        && recipient.is_none_or(|r| r == "all")
        && !matches!(channel, Some("analysis" | "commentary"))
}

impl ChatGptStream {
    fn text(&mut self, index: i64, delta: &str, out: &mut Vec<CanonicalEvent>) {
        let Some(m) = self.messages.get_mut(&index) else {
            return;
        };
        if !m.answer || delta.is_empty() {
            return;
        }
        m.emitted.push_str(delta);
        out.push(CanonicalEvent::TextDelta(delta.to_owned()));
    }

    /// A cumulative value of the answer text: emit what is new.
    fn cumulative(&mut self, index: i64, full: &str, out: &mut Vec<CanonicalEvent>) {
        let Some(m) = self.messages.get(&index) else {
            return;
        };
        if let Some(rest) = full.strip_prefix(m.emitted.as_str()) {
            let rest = rest.to_owned();
            self.text(index, &rest, out);
        }
    }

    fn add_message(&mut self, index: i64, message: &Value, out: &mut Vec<CanonicalEvent>) {
        let answer = is_answer_message(message);
        if let Some(slug) = message
            .pointer("/metadata/model_slug")
            .and_then(Value::as_str)
        {
            self.model = Some(slug.to_owned());
        }
        if answer && !self.started {
            self.started = true;
            out.push(CanonicalEvent::Start {
                id: message.get("id").and_then(Value::as_str).map(str::to_owned),
                model: self.model.clone(),
            });
        }
        self.messages.insert(
            index,
            MessageInfo {
                answer,
                emitted: String::new(),
            },
        );
        if let Some(initial) = message.pointer("/content/parts/0").and_then(Value::as_str) {
            let initial = initial.to_owned();
            self.text(index, &initial, out);
        }
    }

    fn apply(&mut self, path: &str, op: &str, v: &Value, out: &mut Vec<CanonicalEvent>) {
        match (path, op) {
            ("", "add") => {
                if let Some(c) = v.get("conversation_id").and_then(Value::as_str) {
                    self.conversation_id = Some(c.to_owned());
                }
                if let Some(m) = v.get("message") {
                    let index = self.current;
                    self.add_message(index, m, out);
                }
            }
            ("/message/content/parts/0", "append") => {
                if let Some(s) = v.as_str() {
                    self.text(self.current, s, out);
                }
            }
            ("/message/content/parts/0", "replace") => {
                if let Some(s) = v.as_str() {
                    self.cumulative(self.current, s, out);
                }
            }
            _ => {}
        }
        self.last_path = path.to_owned();
        self.last_op = op.to_owned();
    }

    fn handle(&mut self, data: &str, out: &mut Vec<CanonicalEvent>) {
        if data.trim() == "[DONE]" {
            self.done = true;
            return;
        }
        let Ok(d) = serde_json::from_str::<Value>(data) else {
            return;
        };
        if let Some(c) = d.get("conversation_id").and_then(Value::as_str) {
            self.conversation_id = Some(c.to_owned());
        }
        if let Some(e) = d
            .get("error")
            .and_then(Value::as_str)
            .filter(|e| !e.is_empty())
        {
            let kind = if e.to_ascii_lowercase().contains("limit") {
                "rate_limit_error"
            } else {
                "upstream_error"
            };
            out.push(CanonicalEvent::Error {
                message: e.chars().take(500).collect(),
                kind: Some(kind.into()),
            });
            self.done = true;
            self.errored = true;
            return;
        }
        match d.get("type").and_then(Value::as_str) {
            Some("message_stream_complete") => {
                self.done = true;
                return;
            }
            Some(_) => return,
            None => {}
        }
        if let Some(c) = d.get("c").and_then(Value::as_i64) {
            self.current = c;
        }
        let op = d.get("o").and_then(Value::as_str);
        let path = d.get("p").and_then(Value::as_str);
        match (op, path, d.get("v")) {
            (Some("patch"), _, Some(Value::Array(ops))) => {
                for o in ops {
                    let (p, op, v) = (
                        o.get("p").and_then(Value::as_str).unwrap_or(""),
                        o.get("o").and_then(Value::as_str).unwrap_or("append"),
                        o.get("v").unwrap_or(&Value::Null),
                    );
                    self.apply(p, op, v, out);
                }
            }
            (Some(op), path, Some(v)) => self.apply(path.unwrap_or(""), op, v, out),
            (None, Some(path), Some(v)) => {
                let op = self.last_op.clone();
                self.apply(path, &op, v, out);
            }
            // Shorthand: the value continues the previous operation.
            (None, None, Some(v)) if !self.last_path.is_empty() => {
                let (p, op) = (self.last_path.clone(), self.last_op.clone());
                self.apply(&p, &op, v, out);
            }
            _ => {
                // Older format: the whole message so far.
                if let Some(m) = d.get("message").filter(|m| m.is_object()) {
                    let id = m.get("id").and_then(Value::as_str).unwrap_or("").to_owned();
                    let index = match self.legacy_ids.get(&id) {
                        Some(i) => *i,
                        None => {
                            let i = 1_000_000 + self.legacy_ids.len() as i64;
                            self.legacy_ids.insert(id, i);
                            self.add_message(i, &Value::Null, out);
                            if let Some(info) = self.messages.get_mut(&i) {
                                info.answer = is_answer_message(m);
                            }
                            if is_answer_message(m) && !self.started {
                                self.started = true;
                                out.push(CanonicalEvent::Start {
                                    id: m.get("id").and_then(Value::as_str).map(str::to_owned),
                                    model: None,
                                });
                            }
                            i
                        }
                    };
                    if let Some(slug) = m.pointer("/metadata/model_slug").and_then(Value::as_str) {
                        self.model = Some(slug.to_owned());
                    }
                    if let Some(full) = m.pointer("/content/parts/0").and_then(Value::as_str) {
                        let full = full.to_owned();
                        self.cumulative(index, &full, out);
                    }
                }
            }
        }
    }

    fn finish_event(&mut self, out: &mut Vec<CanonicalEvent>) {
        if self.done && !self.finished && !self.errored {
            self.finished = true;
            out.push(CanonicalEvent::Finish(StopReason::EndTurn));
        }
    }
}

impl StreamParser for ChatGptStream {
    fn feed(&mut self, bytes: &[u8]) -> Vec<CanonicalEvent> {
        let mut out = Vec::new();
        for event in self.sse.feed(bytes) {
            if self.done {
                break;
            }
            if event.event.as_deref() == Some("delta_encoding") {
                continue;
            }
            self.handle(&event.data, &mut out);
        }
        self.finish_event(&mut out);
        out
    }

    fn finish(&mut self) -> Vec<CanonicalEvent> {
        let mut out = Vec::new();
        if let Some(event) = self.sse.finish()
            && !self.done
        {
            self.handle(&event.data, &mut out);
        }
        self.done = true;
        self.finish_event(&mut out);
        out
    }

    fn is_done(&self) -> bool {
        self.done
    }

    fn conversation_id(&self) -> Option<&str> {
        self.conversation_id.as_deref()
    }

    fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site() -> WebChatGpt {
        WebChatGpt
    }

    fn text(events: &[CanonicalEvent]) -> String {
        events
            .iter()
            .filter_map(|e| match e {
                CanonicalEvent::TextDelta(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    fn feed_all(chunks: &[&str]) -> (Vec<CanonicalEvent>, Box<dyn StreamParser>) {
        let mut p = site().parser();
        let mut out = Vec::new();
        for c in chunks {
            out.extend(p.feed(c.as_bytes()));
        }
        out.extend(p.finish());
        (out, p)
    }

    #[test]
    fn urls_and_conversation_ids() {
        let s = site();
        let o = "https://chatgpt.com";
        assert_eq!(
            s.new_conversation_url(o, "auto", false),
            "https://chatgpt.com/"
        );
        assert_eq!(
            s.new_conversation_url(o, "gpt-5", true),
            "https://chatgpt.com/?model=gpt-5&temporary-chat=true"
        );
        assert_eq!(
            s.new_conversation_url(o, "bad model&x=1", false),
            "https://chatgpt.com/",
            "unsafe slugs are not put into the URL"
        );
        assert_eq!(
            s.conversation_url(o, "68f0c0de-1234-abcd", "gpt-5"),
            "https://chatgpt.com/c/68f0c0de-1234-abcd?model=gpt-5"
        );
        let id = Some("68f0c0de-1234-abcd".to_owned());
        assert_eq!(
            s.conversation_id(o, "https://chatgpt.com/c/68f0c0de-1234-abcd"),
            id
        );
        assert_eq!(
            s.conversation_id(
                o,
                "https://chatgpt.com/g/g-abc/c/68f0c0de-1234-abcd?model=x"
            ),
            id
        );
        assert_eq!(s.conversation_id(o, "https://chatgpt.com/"), None);
        assert_eq!(
            s.conversation_id(o, "https://evil.test/c/68f0c0de-1234-abcd"),
            None
        );
    }

    #[test]
    fn only_the_page_answer_request_is_matched() {
        let s = site();
        assert!(s.is_answer_request("POST", "https://chatgpt.com/backend-api/conversation"));
        assert!(s.is_answer_request("POST", "https://chatgpt.com/backend-api/f/conversation?x=1"));
        assert!(!s.is_answer_request("GET", "https://chatgpt.com/backend-api/conversation"));
        assert!(!s.is_answer_request("POST", "https://chatgpt.com/backend-api/conversation/init"));
        assert!(s.is_models_response("https://chatgpt.com/backend-api/models?iim=false"));
    }

    #[test]
    fn delta_encoding_stream_yields_only_the_answer_text() {
        let chunks = [
            "event: delta_encoding\ndata: \"v1\"\n\n",
            // The user's message echo is not an answer.
            "event: delta\ndata: {\"p\":\"\",\"o\":\"add\",\"v\":{\"message\":{\"id\":\"u1\",\"author\":{\"role\":\"user\"},\"content\":{\"content_type\":\"text\",\"parts\":[\"question\"]}},\"conversation_id\":\"conv-0001-abcd\"},\"c\":0}\n\n",
            // Hidden reasoning is not output (D12).
            "event: delta\ndata: {\"p\":\"\",\"o\":\"add\",\"v\":{\"message\":{\"id\":\"t1\",\"author\":{\"role\":\"assistant\"},\"content\":{\"content_type\":\"thoughts\",\"thoughts\":[]},\"metadata\":{\"model_slug\":\"gpt-5-thinking\"}}},\"c\":1}\n\n",
            "event: delta\ndata: {\"p\":\"\",\"o\":\"add\",\"v\":{\"message\":{\"id\":\"a1\",\"author\":{\"role\":\"assistant\"},\"content\":{\"content_type\":\"text\",\"parts\":[\"\"]},\"metadata\":{\"model_slug\":\"gpt-5-thinking\"},\"recipient\":\"all\"},\"conversation_id\":\"conv-0001-abcd\"},\"c\":2}\n\n",
            "event: delta\ndata: {\"p\":\"/message/content/parts/0\",\"o\":\"append\",\"v\":\"Hel\"}\n\n",
            "event: delta\ndata: {\"v\":\"lo\"}\n\n",
            "event: delta\ndata: {\"v\":\", w\"}\n\nevent: delta\ndata: {\"o\":\"patch\",\"v\":[{\"p\":\"/message/content/parts/0\",\"o\":\"append\",\"v\":\"orld\"},{\"p\":\"/message/status\",\"o\":\"replace\",\"v\":\"finished_successfully\"}]}\n\n",
            "data: {\"type\":\"title_generation\",\"title\":\"x\",\"conversation_id\":\"conv-0001-abcd\"}\n\n",
            "data: {\"type\":\"message_stream_complete\",\"conversation_id\":\"conv-0001-abcd\"}\n\n",
            "data: [DONE]\n\n",
        ];
        let (events, p) = feed_all(&chunks);
        assert_eq!(text(&events), "Hello, world");
        assert!(
            matches!(events.first(), Some(CanonicalEvent::Start { id: Some(id), .. }) if id == "a1")
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, CanonicalEvent::Finish(_)))
                .count(),
            1
        );
        assert!(p.is_done());
        assert_eq!(p.conversation_id(), Some("conv-0001-abcd"));
        assert_eq!(p.model(), Some("gpt-5-thinking"));
    }

    #[test]
    fn split_chunks_and_cumulative_legacy_messages() {
        let a = "data: {\"message\":{\"id\":\"m1\",\"author\":{\"role\":\"assistant\"},\"content\":{\"content_type\":\"text\",\"parts\":[\"Hi\"]}},\"conversation_id\":\"conv-legacy-01\"}\n\n";
        let b = "data: {\"message\":{\"id\":\"m1\",\"author\":{\"role\":\"assistant\"},\"content\":{\"content_type\":\"text\",\"parts\":[\"Hi there\"]}},\"conversation_id\":\"conv-legacy-01\"}\n\ndata: [DONE]\n\n";
        let joined = format!("{a}{b}");
        let (mid_a, mid_b) = joined.split_at(37);
        let (events, p) = feed_all(&[mid_a, mid_b]);
        assert_eq!(text(&events), "Hi there");
        assert_eq!(p.conversation_id(), Some("conv-legacy-01"));
    }

    #[test]
    fn stream_errors_become_error_events() {
        let (events, _) = feed_all(&[
            "data: {\"message\":null,\"conversation_id\":null,\"error\":\"You've reached the current usage limit.\"}\n\n",
        ]);
        assert!(matches!(
            events.as_slice(),
            [CanonicalEvent::Error { kind: Some(k), .. }] if k == "rate_limit_error"
        ));
    }

    #[test]
    fn error_responses_map_to_failure_kinds() {
        let s = site();
        let cap = s.classify_response(
            429,
            r#"{"detail":{"code":"model_cap_exceeded","message":"You have hit your limit","clears_in":1800}}"#,
        );
        assert_eq!(
            cap.kind,
            FailureKind::QuotaExhausted {
                reset_after: Some(Duration::from_secs(1800))
            }
        );
        assert_eq!(cap.message, "You have hit your limit");
        assert_eq!(
            s.classify_response(429, r#"{"detail":"Too many requests"}"#)
                .kind,
            FailureKind::RateLimited { retry_after: None }
        );
        assert_eq!(s.classify_response(401, "{}").kind, FailureKind::Auth);
        assert_eq!(
            s.classify_response(502, "bad gateway").kind,
            FailureKind::Server
        );
    }

    #[test]
    fn model_list_keeps_safe_slugs_and_offers_auto() {
        let s = site();
        let models = s.parse_models(
            r#"{"models":[{"slug":"gpt-5","title":"GPT-5"},{"slug":"gpt-5-thinking","title":"GPT-5 Thinking"},{"slug":"bad slug"}]}"#,
        );
        let ids: Vec<&str> = models.iter().map(|m| m.upstream_id.as_str()).collect();
        assert_eq!(ids, ["auto", "gpt-5", "gpt-5-thinking"]);
        assert!(!models[1].capabilities.contains(Capabilities::TOOLS));
        assert!(s.parse_models("not json").is_empty());
    }

    #[test]
    fn only_the_real_site_or_a_loopback_mock_is_accepted() {
        let cfg = |u: Option<&str>| ProviderConfig {
            slug: "web-chatgpt".into(),
            kind: "web-chatgpt".into(),
            base_url: u.map(str::to_owned),
            settings: Value::Null,
        };
        let m = WebChatGpt;
        assert!(m.validate(&cfg(None)).is_ok());
        assert!(m.validate(&cfg(Some("https://chatgpt.com/"))).is_ok());
        assert!(m.validate(&cfg(Some("http://127.0.0.1:5000"))).is_ok());
        assert!(m.validate(&cfg(Some("https://chat.example.com"))).is_err());
        assert!(!m.active_probe());
        assert_eq!(m.account_concurrency(), Some(1));
    }
}
