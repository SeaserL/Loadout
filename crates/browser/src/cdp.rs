// SPDX-License-Identifier: Apache-2.0

//! Minimal Chrome DevTools Protocol client over the `--remote-debugging-pipe` transport
//! (ExecPlan 017 §5.1, D1–D2): JSON messages separated by NUL bytes on two anonymous pipes.
//!
//! Only methods on [`ALLOWED_METHODS`] can be sent; cookie and storage access and request
//! interception are refused before anything reaches the browser (ExecPlan 017 §10). Message
//! payloads carry prompts and answers, so they are never logged: only method names.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

use crate::BrowserError;

/// Every method Loadout may call. Anything else is a programming error and is refused.
pub const ALLOWED_METHODS: &[&str] = &[
    "Browser.getVersion",
    "Browser.close",
    "Browser.getWindowForTarget",
    "Browser.setWindowBounds",
    "Target.getTargets",
    "Target.attachToTarget",
    "Target.activateTarget",
    "Page.enable",
    "Page.navigate",
    "Page.bringToFront",
    "Runtime.evaluate",
    "Input.insertText",
    "Input.dispatchKeyEvent",
    "Input.dispatchMouseEvent",
    "Network.enable",
    "Network.streamResourceContent",
    "Network.getResponseBody",
];

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

pub fn is_allowed(method: &str) -> bool {
    ALLOWED_METHODS.contains(&method)
}

/// A protocol event.
#[derive(Clone, Debug)]
pub struct Event {
    pub method: String,
    pub session_id: Option<String>,
    pub params: Value,
}

struct Sub {
    id: u64,
    session: Option<String>,
    prefix: &'static str,
    tx: mpsc::UnboundedSender<Event>,
}

type Reply = oneshot::Sender<Result<Value, BrowserError>>;

struct Inner {
    writer: Mutex<Option<std::sync::mpsc::Sender<Vec<u8>>>>,
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, Reply>>,
    subs: Mutex<Vec<Sub>>,
    closed: AtomicBool,
    closed_tx: tokio::sync::watch::Sender<bool>,
}

impl Inner {
    fn close(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        self.writer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        for (_, reply) in self
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain()
        {
            let _ = reply.send(Err(BrowserError::Closed));
        }
        // Dropping the senders ends every subscription stream.
        self.subs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        let _ = self.closed_tx.send(true);
    }

    fn dispatch(&self, raw: &[u8]) {
        let Ok(msg) = serde_json::from_slice::<Value>(raw) else {
            tracing::warn!("unparseable message from the browser");
            return;
        };
        if let Some(id) = msg.get("id").and_then(Value::as_u64) {
            let reply = self
                .pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&id);
            if let Some(reply) = reply {
                let result = match msg.get("error") {
                    Some(e) => Err(BrowserError::Protocol(
                        e.get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("error")
                            .chars()
                            .take(200)
                            .collect(),
                    )),
                    None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = reply.send(result);
            }
            return;
        }
        let Some(method) = msg.get("method").and_then(Value::as_str) else {
            return;
        };
        let session = msg.get("sessionId").and_then(Value::as_str);
        let mut subs = self.subs.lock().unwrap_or_else(PoisonError::into_inner);
        if !subs.iter().any(|s| {
            method.starts_with(s.prefix) && (s.session.is_none() || s.session.as_deref() == session)
        }) {
            return;
        }
        let event = Event {
            method: method.to_owned(),
            session_id: session.map(str::to_owned),
            params: msg.get("params").cloned().unwrap_or(Value::Null),
        };
        subs.retain(|s| {
            if !method.starts_with(s.prefix)
                || (s.session.is_some() && s.session.as_deref() != session)
            {
                return true;
            }
            s.tx.send(event.clone()).is_ok()
        });
    }
}

/// Events matching one filter, in arrival order. Unsubscribes on drop.
#[derive(Debug)]
pub struct Subscription {
    id: u64,
    rx: mpsc::UnboundedReceiver<Event>,
    inner: Weak<Inner>,
}

impl Subscription {
    /// The next event; `None` once the browser connection closed.
    pub async fn next(&mut self) -> Option<Event> {
        self.rx.recv().await
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            inner
                .subs
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .retain(|s| s.id != self.id);
        }
    }
}

/// A CDP connection. Cheap to clone.
#[derive(Clone)]
pub struct Cdp {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Cdp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cdp")
            .field("closed", &self.inner.closed.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl Cdp {
    /// Starts reader and writer threads on the pipe pair.
    pub fn new(
        to_browser: impl Write + Send + 'static,
        from_browser: impl Read + Send + 'static,
    ) -> Self {
        let (wtx, wrx) = std::sync::mpsc::channel::<Vec<u8>>();
        let (closed_tx, _) = tokio::sync::watch::channel(false);
        let inner = Arc::new(Inner {
            writer: Mutex::new(Some(wtx)),
            next_id: AtomicU64::new(1),
            pending: Mutex::default(),
            subs: Mutex::default(),
            closed: AtomicBool::new(false),
            closed_tx,
        });

        let weak = Arc::downgrade(&inner);
        let mut to_browser = to_browser;
        std::thread::Builder::new()
            .name("cdp-writer".into())
            .spawn(move || {
                while let Ok(frame) = wrx.recv() {
                    if to_browser.write_all(&frame).is_err() {
                        break;
                    }
                }
                if let Some(inner) = weak.upgrade() {
                    inner.close();
                }
            })
            .ok();

        let weak = Arc::downgrade(&inner);
        let mut from_browser = from_browser;
        std::thread::Builder::new()
            .name("cdp-reader".into())
            .spawn(move || {
                let mut buf: Vec<u8> = Vec::with_capacity(1 << 16);
                let mut chunk = vec![0u8; 1 << 16];
                loop {
                    let n = match from_browser.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    buf.extend_from_slice(&chunk[..n]);
                    let Some(inner) = weak.upgrade() else { return };
                    let mut start = 0;
                    while let Some(pos) = buf[start..].iter().position(|&b| b == 0) {
                        inner.dispatch(&buf[start..start + pos]);
                        start += pos + 1;
                    }
                    buf.drain(..start);
                }
                if let Some(inner) = weak.upgrade() {
                    inner.close();
                }
            })
            .ok();
        Self { inner }
    }

    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }

    /// Resolves once the connection closed.
    pub async fn closed(&self) {
        let mut rx = self.inner.closed_tx.subscribe();
        let _ = rx.wait_for(|closed| *closed).await;
    }

    /// Events whose method starts with `prefix` (e.g. `"Network."`), from `session` or from
    /// any session.
    pub fn subscribe(&self, session: Option<&str>, prefix: &'static str) -> Subscription {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::unbounded_channel();
        if !self.is_closed() {
            self.inner
                .subs
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(Sub {
                    id,
                    session: session.map(str::to_owned),
                    prefix,
                    tx,
                });
        }
        Subscription {
            id,
            rx,
            inner: Arc::downgrade(&self.inner),
        }
    }

    pub async fn call(
        &self,
        method: &'static str,
        params: Value,
        session: Option<&str>,
    ) -> Result<Value, BrowserError> {
        self.call_with_timeout(method, params, session, DEFAULT_TIMEOUT)
            .await
    }

    pub async fn call_with_timeout(
        &self,
        method: &'static str,
        params: Value,
        session: Option<&str>,
        timeout: Duration,
    ) -> Result<Value, BrowserError> {
        if !is_allowed(method) {
            tracing::error!(method, "refused a browser method outside the allowlist");
            return Err(BrowserError::NotAllowed(method));
        }
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let mut msg = json!({"id": id, "method": method, "params": params});
        if let Some(s) = session {
            msg["sessionId"] = Value::String(s.to_owned());
        }
        let mut frame =
            serde_json::to_vec(&msg).map_err(|e| BrowserError::Protocol(e.to_string()))?;
        frame.push(0);
        let (tx, rx) = oneshot::channel();
        self.inner
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, tx);
        let sent = self
            .inner
            .writer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|w| w.send(frame).is_ok());
        if !sent {
            self.inner
                .pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&id);
            return Err(BrowserError::Closed);
        }
        tracing::trace!(method, "cdp call");
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(BrowserError::Closed),
            Err(_) => {
                self.inner
                    .pending
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&id);
                Err(BrowserError::Timeout(method))
            }
        }
    }

    /// Drops the connection (the browser sees its pipe close).
    pub fn close(&self) {
        self.inner.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_storage_and_interception_methods_are_refused() {
        for m in [
            "Network.getCookies",
            "Network.getAllCookies",
            "Network.setCookie",
            "Network.setUserAgentOverride",
            "Storage.getCookies",
            "Fetch.enable",
            "Emulation.setUserAgentOverride",
            "Page.addScriptToEvaluateOnNewDocument",
            "Runtime.callFunctionOn",
        ] {
            assert!(!is_allowed(m), "{m} must not be allowed");
        }
        assert!(ALLOWED_METHODS.iter().all(|m| !m.contains("Cookie")
            && !m.starts_with("Storage.")
            && !m.starts_with("Fetch.")));
    }

    /// A fake browser: echoes each command's id with a result, then emits an event.
    #[tokio::test]
    async fn calls_and_events_round_trip_over_nul_framed_pipes() {
        let (from_browser, server_write) = std::io::pipe().expect("pipe");
        let (server_read, to_browser) = std::io::pipe().expect("pipe");
        std::thread::spawn(move || {
            let mut r = server_read;
            let mut w = server_write;
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            while let Ok(n) = r.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                while let Some(pos) = buf.iter().position(|&b| b == 0) {
                    let msg: Value = serde_json::from_slice(&buf[..pos]).expect("json");
                    buf.drain(..=pos);
                    let id = msg["id"].as_u64().expect("id");
                    let event = json!({"method": "Network.dataReceived", "sessionId": "S1",
                        "params": {"requestId": "r1"}});
                    let other =
                        json!({"method": "Page.loadEventFired", "sessionId": "S1", "params": {}});
                    let reply = json!({"id": id, "result": {"echo": msg["method"]}});
                    for m in [other, event, reply] {
                        let mut f = serde_json::to_vec(&m).expect("encode");
                        f.push(0);
                        w.write_all(&f).expect("write");
                    }
                }
            }
        });
        let cdp = Cdp::new(to_browser, from_browser);
        let mut net = cdp.subscribe(Some("S1"), "Network.");
        let r = cdp
            .call("Browser.getVersion", json!({}), None)
            .await
            .expect("call");
        assert_eq!(r["echo"], "Browser.getVersion");
        let e = net.next().await.expect("event");
        assert_eq!(e.method, "Network.dataReceived");
        assert!(matches!(
            cdp.call("Network.getCookies", json!({}), None).await,
            Err(BrowserError::NotAllowed(_))
        ));
        cdp.close();
        assert!(net.next().await.is_none(), "closing ends subscriptions");
        assert!(matches!(
            cdp.call("Browser.getVersion", json!({}), None).await,
            Err(BrowserError::Closed)
        ));
    }
}
