// SPDX-License-Identifier: Apache-2.0

//! One attached tab: the [`PageOps`] a site module may use, plus navigation and passive
//! network capture for the session executor.

use std::time::Duration;

use base64::Engine;
use loadout_provider_sdk::BoxFuture;
use loadout_provider_sdk::web::{PageError, PageOps};
use serde_json::{Value, json};

use crate::BrowserError;
use crate::cdp::{Cdp, Subscription};

impl From<BrowserError> for PageError {
    fn from(e: BrowserError) -> Self {
        match e {
            BrowserError::Closed => Self::Closed,
            BrowserError::Timeout(what) => Self::Timeout(what),
            other => Self::Protocol(other.to_string()),
        }
    }
}

/// An attached page target (flattened session).
#[derive(Clone, Debug)]
pub struct Page {
    cdp: Cdp,
    session: String,
    target_id: String,
}

/// JSON string literal of a static selector, for embedding in a fixed expression.
fn js_str(s: &'static str) -> String {
    Value::String(s.to_owned()).to_string()
}

impl Page {
    pub(crate) async fn attach(cdp: Cdp, target_id: &str) -> Result<Self, BrowserError> {
        let r = cdp
            .call(
                "Target.attachToTarget",
                json!({"targetId": target_id, "flatten": true}),
                None,
            )
            .await?;
        let session = r
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| BrowserError::Protocol("attach returned no session".into()))?
            .to_owned();
        let page = Self {
            cdp,
            session,
            target_id: target_id.to_owned(),
        };
        page.call("Page.enable", json!({})).await?;
        page.call("Network.enable", json!({})).await?;
        Ok(page)
    }

    async fn call(&self, method: &'static str, params: Value) -> Result<Value, BrowserError> {
        self.cdp.call(method, params, Some(&self.session)).await
    }

    /// Network events of this page, from now on.
    pub fn network(&self) -> Subscription {
        self.cdp.subscribe(Some(&self.session), "Network.")
    }

    async fn evaluate(&self, expression: &str) -> Result<Value, BrowserError> {
        let r = self
            .call(
                "Runtime.evaluate",
                json!({"expression": expression, "returnByValue": true}),
            )
            .await?;
        if r.get("exceptionDetails").is_some() {
            return Err(BrowserError::Protocol("page script failed".into()));
        }
        Ok(r.pointer("/result/value").cloned().unwrap_or(Value::Null))
    }

    pub async fn url(&self) -> Result<String, BrowserError> {
        Ok(self
            .evaluate("location.href")
            .await?
            .as_str()
            .unwrap_or_default()
            .to_owned())
    }

    /// Navigates and waits until the document finished loading.
    pub async fn navigate(&self, url: &str) -> Result<(), BrowserError> {
        let r = self.call("Page.navigate", json!({"url": url})).await?;
        if let Some(e) = r.get("errorText").and_then(Value::as_str) {
            return Err(BrowserError::Navigation(e.to_owned()));
        }
        let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
        loop {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if let Ok(Value::String(s)) = self.evaluate("document.readyState").await
                && s == "complete"
            {
                return Ok(());
            }
            if tokio::time::Instant::now() > deadline {
                return Err(BrowserError::Timeout("page load"));
            }
        }
    }

    /// Brings the window and this tab to the front (no effect when headless).
    pub async fn show(&self) -> Result<(), BrowserError> {
        self.cdp
            .call(
                "Target.activateTarget",
                json!({"targetId": self.target_id}),
                None,
            )
            .await?;
        if let Ok(w) = self
            .cdp
            .call(
                "Browser.getWindowForTarget",
                json!({"targetId": self.target_id}),
                None,
            )
            .await
            && let Some(id) = w.get("windowId").and_then(Value::as_i64)
        {
            let _ = self
                .cdp
                .call(
                    "Browser.setWindowBounds",
                    json!({"windowId": id, "bounds": {"windowState": "normal"}}),
                    None,
                )
                .await;
        }
        self.call("Page.bringToFront", json!({})).await.map(|_| ())
    }

    /// Starts streaming a response's body: returns what is already buffered; later chunks
    /// arrive as `Network.dataReceived` events carrying `data`.
    pub async fn stream_body(&self, request_id: &str) -> Result<Vec<u8>, BrowserError> {
        let r = self
            .call(
                "Network.streamResourceContent",
                json!({"requestId": request_id}),
            )
            .await?;
        decode_b64(r.get("bufferedData"))
    }

    /// The complete body of a finished response.
    pub async fn response_body(&self, request_id: &str) -> Result<String, BrowserError> {
        let r = self
            .call("Network.getResponseBody", json!({"requestId": request_id}))
            .await?;
        let body = r.get("body").and_then(Value::as_str).unwrap_or_default();
        if r.get("base64Encoded").and_then(Value::as_bool) == Some(true) {
            let bytes = decode_b64(Some(&Value::String(body.to_owned())))?;
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        } else {
            Ok(body.to_owned())
        }
    }
}

pub(crate) fn decode_b64(v: Option<&Value>) -> Result<Vec<u8>, BrowserError> {
    match v.and_then(Value::as_str) {
        None | Some("") => Ok(Vec::new()),
        Some(s) => base64::engine::general_purpose::STANDARD
            .decode(s)
            .map_err(|_| BrowserError::Protocol("invalid base64 from the browser".into())),
    }
}

impl PageOps for Page {
    fn eval(&self, expr: &'static str) -> BoxFuture<'_, Result<Value, PageError>> {
        Box::pin(async move { Ok(self.evaluate(expr).await?) })
    }

    fn click(&self, selector: &'static str) -> BoxFuture<'_, Result<bool, PageError>> {
        Box::pin(async move {
            let expr = format!(
                "(()=>{{const e=document.querySelector({});if(!e)return null;\
                 e.scrollIntoView({{block:'center'}});const r=e.getBoundingClientRect();\
                 return {{x:r.left+r.width/2,y:r.top+r.height/2,w:r.width,h:r.height}};}})()",
                js_str(selector)
            );
            let rect = self.evaluate(&expr).await?;
            let (Some(x), Some(y), Some(w)) = (
                rect.get("x").and_then(Value::as_f64),
                rect.get("y").and_then(Value::as_f64),
                rect.get("w").and_then(Value::as_f64),
            ) else {
                return Ok(false);
            };
            if w <= 0.0 {
                return Ok(false);
            }
            for kind in ["mouseMoved", "mousePressed", "mouseReleased"] {
                self.call(
                    "Input.dispatchMouseEvent",
                    json!({"type": kind, "x": x, "y": y, "button": "left", "clickCount": 1}),
                )
                .await?;
            }
            Ok(true)
        })
    }

    fn focus(&self, selector: &'static str) -> BoxFuture<'_, Result<bool, PageError>> {
        Box::pin(async move {
            let expr = format!(
                "(()=>{{const e=document.querySelector({});if(!e)return false;e.focus();return true;}})()",
                js_str(selector)
            );
            Ok(self.evaluate(&expr).await? == Value::Bool(true))
        })
    }

    fn insert_text<'a>(&'a self, text: &'a str) -> BoxFuture<'a, Result<(), PageError>> {
        Box::pin(async move {
            self.call("Input.insertText", json!({"text": text})).await?;
            Ok(())
        })
    }

    fn press_key(&self, key: &'static str) -> BoxFuture<'_, Result<(), PageError>> {
        Box::pin(async move {
            let (code, vk, text) = match key {
                "Enter" => ("Enter", 13, Some("\r")),
                "Escape" => ("Escape", 27, None),
                _ => return Err(PageError::NotFound("key")),
            };
            let mut down = json!({"type": "keyDown", "key": key, "code": code,
                "windowsVirtualKeyCode": vk});
            if let Some(t) = text {
                down["text"] = Value::String(t.to_owned());
            }
            self.call("Input.dispatchKeyEvent", down).await?;
            self.call(
                "Input.dispatchKeyEvent",
                json!({"type": "keyUp", "key": key, "code": code, "windowsVirtualKeyCode": vk}),
            )
            .await?;
            Ok(())
        })
    }
}
