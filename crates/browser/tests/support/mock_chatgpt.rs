// SPDX-License-Identifier: Apache-2.0

//! A local stand-in for the ChatGPT web page (ExecPlan 017 Stage 2): the composer, send and
//! stop buttons the `web-chatgpt` module looks for, a model list request on load, and an
//! answer stream in the `delta_encoding` v1 format. Every reply echoes what it received, so
//! tests can check exactly what was typed into the page.
//!
//! Shared by the browser crate's tests and the daemon e2e test (`#[path]` include).

#![allow(dead_code)]

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use bytes::Bytes;
use serde_json::{Value, json};

/// How the mock behaves for the next page loads and answers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Normal,
    /// Pages show a signed-out view.
    LoggedOut,
    /// Pages show a verification interstitial.
    Challenge,
    /// The answer request fails with a usage cap (429, `clears_in`).
    Capped,
    /// Answers stream slowly (for cancellation).
    Slow,
}

/// One answer request as the page sent it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Received {
    pub text: String,
    pub conversation: String,
    pub model: String,
    pub temporary: bool,
    pub turn: usize,
}

#[derive(Debug, Default)]
pub struct MockState {
    pub mode: Mutex<Mode>,
    pub received: Mutex<Vec<Received>>,
    conversations: Mutex<Vec<(String, usize)>>,
    next_id: AtomicUsize,
    pub aborted: AtomicBool,
    pub model_list_requests: AtomicUsize,
}

impl MockState {
    pub fn set_mode(&self, m: Mode) {
        *self.mode.lock().expect("lock") = m;
    }

    fn mode(&self) -> Mode {
        *self.mode.lock().expect("lock")
    }

    pub fn received(&self) -> Vec<Received> {
        self.received.lock().expect("lock").clone()
    }
}

const PAGE: &str = r#"<!doctype html><html><head><meta charset="utf-8"><title>MockGPT</title></head>
<body>
<div id="prompt-textarea" contenteditable="true" style="min-height:40px;border:1px solid #888"></div>
<div id="buttons"></div>
<script>
const composer = document.getElementById('prompt-textarea');
const buttons = document.getElementById('buttons');
let streaming = false, controller = null;
function render() {
  buttons.innerHTML = '';
  if (streaming) {
    const b = document.createElement('button'); b.dataset.testid = 'stop-button'; b.textContent = 'Stop';
    b.onclick = () => controller && controller.abort(); buttons.appendChild(b);
  } else if (composer.innerText.trim().length > 0) {
    const b = document.createElement('button'); b.dataset.testid = 'send-button'; b.textContent = 'Send';
    b.onclick = send; buttons.appendChild(b);
  }
}
composer.addEventListener('input', render);
composer.addEventListener('keydown', e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(); } });
fetch('/backend-api/models').then(r => r.json()).catch(() => {});
async function send() {
  const text = composer.innerText;
  if (!text.trim()) return;
  composer.innerHTML = ''; streaming = true; render();
  const params = new URLSearchParams(location.search);
  const m = location.pathname.match(/\/c\/([\w-]+)/);
  const temporary = params.get('temporary-chat') === 'true';
  controller = new AbortController();
  let convo = null;
  try {
    const r = await fetch('/backend-api/f/conversation', {method: 'POST',
      headers: {'content-type': 'application/json'},
      body: JSON.stringify({text, conversation_id: m ? m[1] : null,
        model: params.get('model') || 'auto', temporary}), signal: controller.signal});
    const reader = r.body.getReader(); const dec = new TextDecoder();
    while (true) {
      const {value, done} = await reader.read(); if (done) break;
      const mm = dec.decode(value).match(/"conversation_id":"([\w-]+)"/); if (mm) convo = mm[1];
    }
  } catch (e) {}
  if (convo && !m && !temporary) history.pushState({}, '', '/c/' + convo);
  streaming = false; controller = null; render();
}
render();
</script></body></html>"#;

const LOGGED_OUT: &str = r#"<!doctype html><html><head><title>MockGPT</title></head><body>
<button data-testid="login-button">Log in</button></body></html>"#;

const CHALLENGE: &str = r#"<!doctype html><html><head><title>Just a moment...</title></head><body>
<div id="challenge-form">Verify you are human</div></body></html>"#;

async fn page(State(state): State<Arc<MockState>>) -> Html<&'static str> {
    Html(match state.mode() {
        Mode::LoggedOut => LOGGED_OUT,
        Mode::Challenge => CHALLENGE,
        _ => PAGE,
    })
}

async fn models(State(state): State<Arc<MockState>>) -> impl IntoResponse {
    state.model_list_requests.fetch_add(1, Ordering::Relaxed);
    axum::Json(json!({"models": [
        {"slug": "gpt-5", "title": "GPT-5"},
        {"slug": "gpt-5-thinking", "title": "GPT-5 Thinking"}
    ]}))
}

/// Sets `aborted` if the body is dropped before the last chunk.
struct AbortFlag {
    state: Arc<MockState>,
    finished: bool,
}

impl AbortFlag {
    fn finish(mut self) {
        self.finished = true;
    }
}

impl Drop for AbortFlag {
    fn drop(&mut self) {
        if !self.finished {
            self.state.aborted.store(true, Ordering::Release);
        }
    }
}

fn frame(event: Option<&str>, data: &Value) -> String {
    match event {
        Some(e) => format!("event: {e}\ndata: {data}\n\n"),
        None => format!("data: {data}\n\n"),
    }
}

async fn conversation(State(state): State<Arc<MockState>>, body: Bytes) -> Response {
    let req: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let mode = state.mode();
    if mode == Mode::Capped {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            axum::Json(json!({"detail": {"code": "model_cap_exceeded",
                "message": "You have hit the usage limit for this model.", "clears_in": 1800}})),
        )
            .into_response();
    }
    let text = req["text"].as_str().unwrap_or_default().to_owned();
    let model = req["model"].as_str().unwrap_or("auto").to_owned();
    let temporary = req["temporary"].as_bool().unwrap_or(false);
    let (conversation, turn) = {
        let mut convs = state.conversations.lock().expect("lock");
        match req["conversation_id"].as_str() {
            Some(id) => match convs.iter_mut().find(|(c, _)| c == id) {
                Some((c, n)) => {
                    *n += 1;
                    (c.clone(), *n)
                }
                None => {
                    convs.push((id.to_owned(), 1));
                    (id.to_owned(), 1)
                }
            },
            None => {
                let n = state.next_id.fetch_add(1, Ordering::Relaxed) + 1;
                let id = format!("conv-{n:04}-mock");
                convs.push((id.clone(), 1));
                (id, 1)
            }
        }
    };
    state.received.lock().expect("lock").push(Received {
        text: text.clone(),
        conversation: conversation.clone(),
        model: model.clone(),
        temporary,
        turn,
    });

    let reply = format!("turn {turn} of {conversation}: {text}");
    let mut frames = vec![
        frame(Some("delta_encoding"), &json!("v1")),
        frame(
            Some("delta"),
            &json!({"p": "", "o": "add", "c": 0, "v": {"message": {"id": "msg-user",
                "author": {"role": "user"}, "content": {"content_type": "text", "parts": [text]}},
                "conversation_id": conversation}}),
        ),
        frame(
            Some("delta"),
            &json!({"p": "", "o": "add", "c": 1, "v": {"message": {"id": "msg-answer",
                "author": {"role": "assistant"}, "content": {"content_type": "text", "parts": [""]},
                "metadata": {"model_slug": if model == "auto" { "gpt-5" } else { model.as_str() }},
                "recipient": "all"}, "conversation_id": conversation}}),
        ),
    ];
    let chars: Vec<char> = reply.chars().collect();
    let pieces: Vec<String> = chars.chunks(7).map(|c| c.iter().collect()).collect();
    for (i, piece) in pieces.iter().enumerate() {
        let v = if i == 0 {
            json!({"p": "/message/content/parts/0", "o": "append", "v": piece})
        } else {
            json!({"v": piece})
        };
        frames.push(frame(Some("delta"), &v));
    }
    frames.push(frame(
        None,
        &json!({"type": "message_stream_complete", "conversation_id": conversation}),
    ));
    frames.push("data: [DONE]\n\n".to_owned());
    let delay = if mode == Mode::Slow {
        Duration::from_millis(400)
    } else {
        Duration::from_millis(15)
    };
    let guard = AbortFlag {
        state: state.clone(),
        finished: false,
    };
    let stream = futures_util::stream::unfold(
        (frames.into_iter(), guard, true),
        move |(mut it, guard, first)| async move {
            if !first {
                tokio::time::sleep(delay).await;
            }
            match it.next() {
                Some(f) => Some((Ok::<_, Infallible>(Bytes::from(f)), (it, guard, false))),
                None => {
                    guard.finish();
                    None
                }
            }
        },
    );
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from_stream(stream))
        .expect("response")
}

/// Starts the mock on a background runtime; returns its origin (`http://127.0.0.1:<port>`).
pub fn start() -> (String, Arc<MockState>) {
    let state = Arc::new(MockState::default());
    let shared = state.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime");
        rt.block_on(async move {
            let app = Router::new()
                .route("/", get(page))
                .route("/c/{id}", get(page))
                .route("/backend-api/models", get(models))
                .route("/backend-api/f/conversation", post(conversation))
                .with_state(shared);
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            tx.send(listener.local_addr().expect("addr"))
                .expect("send addr");
            let _ = axum::serve(listener, app).await;
        });
    });
    let addr: SocketAddr = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("mock started");
    (format!("http://{addr}"), state)
}
