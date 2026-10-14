// SPDX-License-Identifier: Apache-2.0

//! Proxy behaviour against scripted mock upstreams (ExecPlan 002 §7).

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use loadout_gateway::{
    ClientId, ClientKeyIndex, ClientPolicies, ClientPolicy, DataPlane, Gateway, GatewayConfig,
    RequestRecord, ResponseRouteStore, TelemetrySink, UpstreamLimits, generate_client_key,
};
use loadout_protocol::Protocol;
use loadout_protocol::sse::SseParser;
use loadout_provider_sdk::usage::{Confidence, UsageReport, UsageSource};
use loadout_provider_sdk::{
    BoxFuture, Capabilities, HttpClient, HttpConfig, ProviderConfig, ProviderModule, TrustLevel,
};
use loadout_providers::{AnthropicCompatible, OpenAiCompatible};
use loadout_router::{
    HealthRegistry, HealthState, Prices, RouteTable, RouteTableHandle, RouteTarget, TargetId,
};
use loadout_secrets::Secret;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

// ---- mock upstream ------------------------------------------------------------------------

#[derive(Clone)]
enum Reply {
    Status(u16, String),
    Json(Value),
    Sse(Vec<String>),
    /// Sends these events, then drops the connection.
    SseThenDrop(Vec<String>),
}

#[derive(Clone, Debug)]
struct Seen {
    path: String,
    headers: HeaderMap,
    body: Value,
}

#[derive(Clone, Default)]
struct Mock {
    replies: Arc<Mutex<HashMap<String, VecDeque<Reply>>>>,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Mock {
    fn push(&self, path: &str, reply: Reply) {
        self.replies
            .lock()
            .expect("lock")
            .entry(path.to_owned())
            .or_default()
            .push_back(reply);
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().expect("lock").clone()
    }
}

async fn mock_handler(State(mock): State<Mock>, req: Request) -> Response {
    let path = req.uri().path().to_owned();
    let headers = req.headers().clone();
    let bytes = axum::body::to_bytes(req.into_body(), 1 << 20)
        .await
        .expect("body");
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    mock.seen.lock().expect("lock").push(Seen {
        path: path.clone(),
        headers,
        body,
    });
    let reply = mock
        .replies
        .lock()
        .expect("lock")
        .get_mut(&path)
        .and_then(VecDeque::pop_front)
        .unwrap_or(Reply::Status(500, "no scripted reply".into()));
    match reply {
        Reply::Status(code, body) => {
            (StatusCode::from_u16(code).expect("status"), body).into_response()
        }
        Reply::Json(v) => axum::Json(v).into_response(),
        Reply::Sse(events) => sse_response(events, false),
        Reply::SseThenDrop(events) => sse_response(events, true),
    }
}

fn sse_response(events: Vec<String>, drop_after: bool) -> Response {
    let items: Vec<Result<Bytes, std::io::Error>> =
        events.into_iter().map(|e| Ok(Bytes::from(e))).collect();
    // Resetting the connection immediately can make Windows discard data the client has not
    // read yet; wait briefly so the drop happens after the events were delivered.
    let tail = futures_util::stream::once(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        Err(std::io::Error::other("connection reset"))
    });
    let stream = futures_util::stream::iter(items);
    use futures_util::StreamExt;
    let body = if drop_after {
        Body::from_stream(stream.chain(tail))
    } else {
        Body::from_stream(stream)
    };
    let mut resp = Response::new(body);
    resp.headers_mut()
        .insert("content-type", "text/event-stream".parse().expect("ct"));
    resp
}

async fn start_mock() -> (Mock, String) {
    let mock = Mock::default();
    let app = Router::new()
        .route("/v1/chat/completions", post(mock_handler))
        .route("/v1/messages", post(mock_handler))
        .route("/v1/responses", post(mock_handler))
        .with_state(mock.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (mock, format!("http://{addr}/v1"))
}

// ---- upstream stream fixtures --------------------------------------------------------------

fn chat_chunk(delta: Value, finish: Option<&str>) -> String {
    format!(
        "data: {}\n\n",
        json!({"id": "up1", "model": "up-model", "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]})
    )
}

fn chat_stream_text(text: &str) -> Vec<String> {
    vec![
        chat_chunk(json!({"role": "assistant", "content": ""}), None),
        chat_chunk(json!({"content": text}), None),
        chat_chunk(json!({}), Some("stop")),
        format!(
            "data: {}\n\n",
            json!({"id": "up1", "choices": [], "usage": {"prompt_tokens": 11, "completion_tokens": 7}})
        ),
        "data: [DONE]\n\n".into(),
    ]
}

fn chat_stream_tool() -> Vec<String> {
    vec![
        chat_chunk(json!({"role": "assistant"}), None),
        chat_chunk(json!({"content": "Let me check."}), None),
        chat_chunk(
            json!({"tool_calls": [{"index": 0, "id": "call_9", "type": "function", "function": {"name": "weather", "arguments": ""}}]}),
            None,
        ),
        chat_chunk(
            json!({"tool_calls": [{"index": 0, "function": {"arguments": "{\"city\":\"Paris\"}"}}]}),
            None,
        ),
        chat_chunk(json!({}), Some("tool_calls")),
        format!(
            "data: {}\n\n",
            json!({"choices": [], "usage": {"prompt_tokens": 20, "completion_tokens": 9}})
        ),
        "data: [DONE]\n\n".into(),
    ]
}

fn anthropic_stream_text(text: &str) -> Vec<String> {
    let ev = |name: &str, data: Value| format!("event: {name}\ndata: {data}\n\n");
    vec![
        ev(
            "message_start",
            json!({"type": "message_start", "message": {"id": "msg_up", "model": "claude-up", "usage": {"input_tokens": 12, "output_tokens": 1}}}),
        ),
        ev(
            "content_block_start",
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        ),
        ev(
            "content_block_delta",
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": text}}),
        ),
        ev(
            "content_block_stop",
            json!({"type": "content_block_stop", "index": 0}),
        ),
        ev(
            "message_delta",
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 5}}),
        ),
        ev("message_stop", json!({"type": "message_stop"})),
    ]
}

fn responses_stream_text(id: &str, text: &str) -> Vec<String> {
    let ev = |name: &str, data: Value| format!("event: {name}\ndata: {data}\n\n");
    vec![
        ev(
            "response.created",
            json!({"type": "response.created", "response": {"id": id, "model": "gpt-up"}}),
        ),
        ev(
            "response.output_text.delta",
            json!({"type": "response.output_text.delta", "item_id": "m", "delta": text}),
        ),
        ev(
            "response.completed",
            json!({"type": "response.completed", "response": {"id": id, "status": "completed", "usage": {"input_tokens": 3, "output_tokens": 2}}}),
        ),
    ]
}

// ---- gateway harness ------------------------------------------------------------------------

#[derive(Default)]
struct Records(Mutex<Vec<RequestRecord>>, Mutex<Vec<(String, UsageReport)>>);

impl TelemetrySink for Records {
    fn record(&self, r: RequestRecord) {
        self.0.lock().expect("lock").push(r);
    }
    fn observe_usage(&self, account_id: &str, report: UsageReport) {
        self.1
            .lock()
            .expect("lock")
            .push((account_id.to_owned(), report));
    }
}

#[derive(Default)]
struct Routes(Mutex<HashMap<String, String>>);

impl ResponseRouteStore for Routes {
    fn get<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Option<String>> {
        let v = self.0.lock().expect("lock").get(id).cloned();
        Box::pin(async move { v })
    }
    fn put(&self, id: String, target: String) {
        self.0.lock().expect("lock").insert(id, target);
    }
}

struct TargetSpec {
    provider: &'static str,
    alias: &'static str,
    position: i64,
    model: &'static str,
    protocol: Protocol,
    base: String,
}

fn build_target(reg: &HealthRegistry, s: &TargetSpec) -> Arc<RouteTarget> {
    let module: Arc<dyn ProviderModule> = match s.protocol {
        Protocol::Messages => Arc::new(AnthropicCompatible),
        _ => Arc::new(OpenAiCompatible),
    };
    let config = ProviderConfig {
        slug: s.provider.into(),
        kind: module.descriptor().kind.into(),
        base_url: Some(s.base.clone()),
        settings: Value::Object(Default::default()),
    };
    let key = Secret::new(format!("sk-upstream-{}-{}", s.provider, s.alias));
    let endpoint = module
        .endpoint(&config, s.protocol, Some(&key))
        .expect("endpoint");
    let id: TargetId = Arc::from(format!(
        "{}-{}-{}:{}",
        s.provider,
        s.alias,
        s.model,
        s.protocol.as_str()
    ));
    Arc::new(RouteTarget {
        health: reg.get_or_create(&id),
        id,
        provider_slug: Arc::from(s.provider),
        account_id: Arc::from(format!("{}-{}", s.provider, s.alias)),
        account_alias: Arc::from(s.alias),
        account_position: s.position,
        upstream_model: Arc::from(s.model),
        protocol: s.protocol,
        trust: TrustLevel::Local,
        opted_in: false,
        context_window: None,
        max_output_tokens: None,
        capabilities: Capabilities::DEFAULT_MODEL,
        prices: Prices {
            input: Some(1.0),
            output: Some(2.0),
            cache_read: None,
            cache_write: None,
        },
        endpoint,
        web: None,
        module,
        config: Arc::new(config),
    })
}

struct Harness {
    addr: SocketAddr,
    key: String,
    http: reqwest::Client,
    records: Arc<Records>,
    health: Arc<HealthRegistry>,
    routes_store: Arc<Routes>,
    policies: Arc<ClientPolicies>,
    _shutdown: CancellationToken,
}

impl Harness {
    async fn new(specs: Vec<TargetSpec>) -> Self {
        let health = Arc::new(HealthRegistry::default());
        let targets = specs.iter().map(|s| build_target(&health, s)).collect();
        Self::with_targets(health, targets).await
    }

    async fn with_targets(health: Arc<HealthRegistry>, targets: Vec<Arc<RouteTarget>>) -> Self {
        Self::with_table(health, RouteTable::new(1, targets)).await
    }

    async fn with_table(health: Arc<HealthRegistry>, table: RouteTable) -> Self {
        let routes = Arc::new(RouteTableHandle::new(table));
        let records = Arc::new(Records::default());
        let routes_store = Arc::new(Routes::default());
        let policies = Arc::new(ClientPolicies::default());
        let plane = DataPlane {
            routes,
            http: HttpClient::new(&HttpConfig::default()).expect("http"),
            telemetry: records.clone(),
            response_routes: routes_store.clone(),
            limits: UpstreamLimits {
                first_token_timeout: Duration::from_secs(10),
                ..UpstreamLimits::default()
            },
            sessions: Arc::default(),
            policies: policies.clone(),
            account_slots: Arc::default(),
        };
        let key = generate_client_key();
        let keys = Arc::new(ClientKeyIndex::new([(
            key.hash,
            ClientId::from("test-client"),
        )]));
        let gateway = Gateway::bind(
            GatewayConfig {
                bind: SocketAddr::from(([127, 0, 0, 1], 0)),
                ..GatewayConfig::default()
            },
            keys,
            Arc::new(plane),
        )
        .await
        .expect("bind");
        let addr = gateway.local_addr();
        let shutdown = CancellationToken::new();
        tokio::spawn(gateway.serve(shutdown.clone()));
        Self {
            addr,
            key: key.key,
            http: reqwest::Client::new(),
            records,
            health,
            routes_store,
            policies,
            _shutdown: shutdown,
        }
    }

    async fn post(&self, path: &str, body: Value) -> reqwest::Response {
        self.post_with(path, body, &[]).await
    }

    async fn post_with(
        &self,
        path: &str,
        body: Value,
        headers: &[(&str, &str)],
    ) -> reqwest::Response {
        let mut req = self
            .http
            .post(format!("http://{}{path}", self.addr))
            .bearer_auth(&self.key)
            .json(&body);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        req.send().await.expect("send")
    }

    /// Waits for the background pump to record the request.
    async fn records(&self, n: usize) -> Vec<RequestRecord> {
        for _ in 0..200 {
            let r = self.records.0.lock().expect("lock").clone();
            if r.len() >= n {
                return r;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("expected {n} telemetry records");
    }
}

fn spec(
    provider: &'static str,
    alias: &'static str,
    pos: i64,
    model: &'static str,
    protocol: Protocol,
    base: &str,
) -> TargetSpec {
    TargetSpec {
        provider,
        alias,
        position: pos,
        model,
        protocol,
        base: base.to_owned(),
    }
}

fn sse_json(text: &str) -> Vec<(Option<String>, Value)> {
    let mut p = SseParser::new();
    let mut events = p.feed(text.as_bytes());
    events.extend(p.finish());
    events
        .into_iter()
        .filter(|e| e.data != "[DONE]")
        .map(|e| (e.event, serde_json::from_str(&e.data).expect("event json")))
        .collect()
}

// ---- tests ------------------------------------------------------------------------------------

#[tokio::test]
async fn chat_passthrough_stream_is_byte_faithful_and_hides_injected_usage() {
    let (mock, base) = start_mock().await;
    let upstream = chat_stream_text("Hello");
    mock.push("/v1/chat/completions", Reply::Sse(upstream.clone()));
    let h = Harness::new(vec![spec(
        "acme",
        "main",
        0,
        "up-model",
        Protocol::Chat,
        &base,
    )])
    .await;

    let resp = h
        .post("/v1/chat/completions", json!({"model": "acme/main/up-model", "stream": true, "messages": [{"role": "user", "content": "hi"}], "custom_field": 1}))
        .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["x-loadout-route"], "acme/main/up-model");
    let text = resp.text().await.expect("text");
    let expected: String = upstream
        .iter()
        .filter(|e| !e.contains("\"usage\""))
        .cloned()
        .collect();
    assert_eq!(
        text, expected,
        "bytes unchanged except the injected usage chunk"
    );

    let seen = mock.seen();
    assert_eq!(seen[0].body["model"], "up-model");
    assert_eq!(seen[0].body["stream_options"]["include_usage"], true);
    assert_eq!(
        seen[0].body["custom_field"], 1,
        "unknown fields pass through"
    );
    assert_eq!(
        seen[0].headers["authorization"],
        "Bearer sk-upstream-acme-main"
    );

    let r = &h.records(1).await[0];
    assert_eq!(r.status, "ok");
    assert_eq!(r.mode, Some("passthrough"));
    assert_eq!(r.usage.input_tokens, Some(11));
    assert_eq!(r.usage_confidence, "EXACT");
    assert!(r.ttft_ms.is_some());
    assert!(r.estimated_cost_usd.is_some());
    assert_eq!(r.client_id.as_deref(), Some("test-client"));
}

#[tokio::test]
async fn chat_passthrough_keeps_usage_chunk_when_client_asked() {
    let (mock, base) = start_mock().await;
    mock.push("/v1/chat/completions", Reply::Sse(chat_stream_text("x")));
    let h = Harness::new(vec![spec("acme", "main", 0, "m", Protocol::Chat, &base)]).await;
    let text = h
        .post("/v1/chat/completions", json!({"model": "acme/main/m", "stream": true, "stream_options": {"include_usage": true}, "messages": []}))
        .await
        .text()
        .await
        .expect("text");
    assert!(text.contains("\"usage\""));
}

#[tokio::test]
async fn non_stream_passthrough_returns_upstream_json() {
    let (mock, base) = start_mock().await;
    let body = json!({"id": "c1", "object": "chat.completion", "choices": [{"index": 0, "message": {"role": "assistant", "content": "hey"}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 1, "completion_tokens": 2}});
    mock.push("/v1/chat/completions", Reply::Json(body.clone()));
    let h = Harness::new(vec![spec("acme", "main", 0, "m", Protocol::Chat, &base)]).await;
    let resp = h
        .post(
            "/v1/chat/completions",
            json!({"model": "acme/main/m", "messages": []}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.json::<Value>().await.expect("json"), body);
    assert_eq!(h.records(1).await[0].usage.output_tokens, Some(2));
}

#[tokio::test]
async fn anthropic_client_streams_through_chat_upstream() {
    let (mock, base) = start_mock().await;
    mock.push("/v1/chat/completions", Reply::Sse(chat_stream_tool()));
    let h = Harness::new(vec![spec("cc", "main", 0, "kimi", Protocol::Chat, &base)]).await;

    let resp = h
        .post(
            "/v1/messages",
            json!({"model": "cc/main/kimi", "max_tokens": 100, "stream": true,
                   "system": "be terse",
                   "messages": [{"role": "user", "content": "weather in Paris?"}],
                   "tools": [{"name": "weather", "input_schema": {"type": "object"}}]}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    let events = sse_json(&resp.text().await.expect("text"));
    let names: Vec<&str> = events.iter().filter_map(|(n, _)| n.as_deref()).collect();
    assert_eq!(names.first(), Some(&"message_start"));
    assert_eq!(names.last(), Some(&"message_stop"));
    let tool = events
        .iter()
        .find(|(_, v)| v["content_block"]["type"] == "tool_use")
        .expect("tool_use block");
    assert_eq!(tool.1["content_block"]["name"], "weather");
    let delta = events
        .iter()
        .find(|(n, _)| n.as_deref() == Some("message_delta"))
        .expect("delta");
    assert_eq!(delta.1["delta"]["stop_reason"], "tool_use");
    assert_eq!(delta.1["usage"]["output_tokens"], 9);

    let upstream = &mock.seen()[0];
    assert_eq!(upstream.path, "/v1/chat/completions");
    assert_eq!(upstream.body["messages"][0]["role"], "system");
    assert_eq!(upstream.body["tools"][0]["function"]["name"], "weather");
    assert_eq!(upstream.body["max_tokens"], 100);
    assert_eq!(h.records(1).await[0].mode, Some("converted"));
}

#[tokio::test]
async fn chat_client_gets_aggregated_json_from_anthropic_upstream() {
    let (mock, base) = start_mock().await;
    mock.push("/v1/messages", Reply::Sse(anthropic_stream_text("Bonjour")));
    let h = Harness::new(vec![spec(
        "claude",
        "main",
        0,
        "claude-up",
        Protocol::Messages,
        &base,
    )])
    .await;
    let resp = h
        .post("/v1/chat/completions", json!({"model": "claude/main/claude-up", "messages": [{"role": "user", "content": "hi"}]}))
        .await;
    assert_eq!(resp.status(), 200);
    let v: Value = resp.json().await.expect("json");
    assert_eq!(v["object"], "chat.completion");
    assert_eq!(v["model"], "claude/main/claude-up");
    assert_eq!(v["choices"][0]["message"]["content"], "Bonjour");
    assert_eq!(v["usage"]["prompt_tokens"], 12);
    assert_eq!(v["usage"]["completion_tokens"], 5);
    let upstream = &mock.seen()[0];
    assert_eq!(
        upstream.body["stream"], true,
        "conversion always streams upstream"
    );
    assert_eq!(upstream.headers["x-api-key"], "sk-upstream-claude-main");
    assert!(
        upstream.headers.get("authorization").is_none(),
        "client key never forwarded"
    );
}

#[tokio::test]
async fn responses_client_through_chat_upstream() {
    let (mock, base) = start_mock().await;
    mock.push(
        "/v1/chat/completions",
        Reply::Sse(chat_stream_text("Hi there")),
    );
    let h = Harness::new(vec![spec("acme", "main", 0, "m", Protocol::Chat, &base)]).await;
    let text = h
        .post(
            "/v1/responses",
            json!({"model": "acme/main/m", "input": "hello", "stream": true}),
        )
        .await
        .text()
        .await
        .expect("text");
    let events = sse_json(&text);
    assert_eq!(events[0].1["type"], "response.created");
    let done = events.last().expect("last");
    assert_eq!(done.1["type"], "response.completed");
    assert_eq!(
        done.1["response"]["output"][0]["content"][0]["text"],
        "Hi there"
    );
}

#[tokio::test]
async fn rate_limited_account_fails_over_to_next_account() {
    let (mock_a, base_a) = start_mock().await;
    let (mock_b, base_b) = start_mock().await;
    mock_a.push(
        "/v1/chat/completions",
        Reply::Status(429, r#"{"error":{"message":"Rate limit reached"}}"#.into()),
    );
    mock_b.push(
        "/v1/chat/completions",
        Reply::Sse(chat_stream_text("from b")),
    );
    let h = Harness::new(vec![
        spec("acme", "a", 0, "m", Protocol::Chat, &base_a),
        spec("acme", "b", 1, "m", Protocol::Chat, &base_b),
    ])
    .await;

    let resp = h
        .post(
            "/v1/chat/completions",
            json!({"model": "acme/m", "stream": true, "messages": []}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["x-loadout-route"], "acme/b/m");
    assert!(resp.text().await.expect("text").contains("from b"));
    let r = &h.records(1).await[0];
    assert_eq!(r.attempts, 2);
    assert_eq!(r.account.as_deref(), Some("b"));
    assert_eq!(
        r.decision.as_deref(),
        Some("acme/a/m: 429 rate_limit → acme/b/m: ok")
    );
    assert_eq!(
        h.health.get("acme-a-m:chat").expect("health").state(),
        HealthState::RateLimited
    );

    // The rate-limited account is skipped on the next request without being called.
    mock_b.push(
        "/v1/chat/completions",
        Reply::Sse(chat_stream_text("again")),
    );
    let resp = h
        .post(
            "/v1/chat/completions",
            json!({"model": "acme/m", "stream": true, "messages": []}),
        )
        .await;
    assert_eq!(resp.headers()["x-loadout-route"], "acme/b/m");
    assert_eq!(mock_a.seen().len(), 1);
    resp.text().await.expect("text");
    assert_eq!(
        h.records(2).await[1].decision.as_deref(),
        Some("acme/a/m: skipped (rate limited) → acme/b/m: ok"),
        "metadata only: no upstream error text"
    );
}

#[tokio::test]
async fn quota_exhausted_exact_account_fails_over_within_provider() {
    let (mock_a, base_a) = start_mock().await;
    let (mock_b, base_b) = start_mock().await;
    mock_a.push(
        "/v1/chat/completions",
        Reply::Status(
            402,
            r#"{"error":{"message":"Insufficient balance"}}"#.into(),
        ),
    );
    mock_b.push(
        "/v1/chat/completions",
        Reply::Json(json!({"id": "x", "choices": []})),
    );
    let h = Harness::new(vec![
        spec("acme", "a", 0, "m", Protocol::Chat, &base_a),
        spec("acme", "b", 1, "m", Protocol::Chat, &base_b),
    ])
    .await;
    let resp = h
        .post(
            "/v1/chat/completions",
            json!({"model": "acme/a/m", "messages": []}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["x-loadout-route"], "acme/b/m");
    assert_eq!(
        h.health.get("acme-a-m:chat").expect("h").state(),
        HealthState::QuotaExhausted
    );
}

#[tokio::test]
async fn disabled_or_repeatedly_rejected_accounts_become_terminal() {
    let (mock_a, base_a) = start_mock().await;
    let (mock_b, base_b) = start_mock().await;
    let (mock_c, base_c) = start_mock().await;
    mock_a.push(
        "/v1/chat/completions",
        Reply::Status(
            403,
            r#"{"error":{"type":"permission_error","message":"This organization has been disabled."}}"#
                .into(),
        ),
    );
    for _ in 0..2 {
        mock_b.push(
            "/v1/chat/completions",
            Reply::Status(401, r#"{"error":{"message":"invalid x-api-key"}}"#.into()),
        );
    }
    for _ in 0..3 {
        mock_c.push(
            "/v1/chat/completions",
            Reply::Json(json!({"id": "x", "choices": []})),
        );
    }
    let h = Harness::new(vec![
        spec("acme", "a", 0, "m", Protocol::Chat, &base_a),
        spec("acme", "b", 1, "m", Protocol::Chat, &base_b),
        spec("acme", "c", 2, "m", Protocol::Chat, &base_c),
    ])
    .await;
    let send = || {
        h.post(
            "/v1/chat/completions",
            json!({"model": "acme/m", "messages": []}),
        )
    };

    // a: disabled at once; b: a first 401 retries after an hour.
    assert_eq!(send().await.headers()["x-loadout-route"], "acme/c/m");
    let a = h.health.get("acme-a-m:chat").expect("a");
    let b = h.health.get("acme-b-m:chat").expect("b");
    assert!(a.is_terminal());
    assert_eq!(b.state(), HealthState::AuthFailed);
    assert!(!b.is_terminal());

    // The half-open retry of b is rejected again: terminal.
    let later = loadout_router::now_ms() + 3_600_001;
    assert!(b.is_available(later) && !a.is_available(later));
    b.record_failure(
        loadout_provider_sdk::FailureKind::Auth,
        "invalid x-api-key",
        later,
    );
    assert!(b.is_terminal());
    let snap = b.snapshot();
    assert_eq!((snap.terminal, snap.until_ms), (true, None));

    // Neither is called again; c serves.
    assert_eq!(send().await.headers()["x-loadout-route"], "acme/c/m");
    assert_eq!(mock_a.seen().len(), 1);
    assert_eq!(mock_b.seen().len(), 1);

    // Until the user acts (new key, re-enabled account or a successful check reset it).
    b.reset();
    assert!(b.is_available(loadout_router::now_ms()));
    assert!(a.is_terminal());
}

#[tokio::test]
async fn bad_request_is_returned_without_failover() {
    let (mock_a, base_a) = start_mock().await;
    let (mock_b, base_b) = start_mock().await;
    mock_a.push(
        "/v1/chat/completions",
        Reply::Status(
            400,
            r#"{"error":{"message":"messages must not be empty"}}"#.into(),
        ),
    );
    let h = Harness::new(vec![
        spec("acme", "a", 0, "m", Protocol::Chat, &base_a),
        spec("acme", "b", 1, "m", Protocol::Chat, &base_b),
    ])
    .await;
    let resp = h
        .post(
            "/v1/chat/completions",
            json!({"model": "acme/m", "messages": []}),
        )
        .await;
    assert_eq!(resp.status(), 400);
    let v: Value = resp.json().await.expect("json");
    assert!(
        v["error"]["message"]
            .as_str()
            .expect("msg")
            .contains("messages must not be empty")
    );
    assert!(mock_b.seen().is_empty());
    assert_ne!(
        h.health.get("acme-a-m:chat").expect("h").state(),
        HealthState::Cooldown
    );
}

#[tokio::test]
async fn error_event_before_output_fails_over() {
    let (mock_a, base_a) = start_mock().await;
    let (mock_b, base_b) = start_mock().await;
    mock_a.push(
        "/v1/messages",
        Reply::Sse(vec![
            "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n".into(),
        ]),
    );
    mock_b.push("/v1/messages", Reply::Sse(anthropic_stream_text("ok")));
    let h = Harness::new(vec![
        spec("claude", "a", 0, "c", Protocol::Messages, &base_a),
        spec("claude", "b", 1, "c", Protocol::Messages, &base_b),
    ])
    .await;
    let resp = h
        .post("/v1/messages", json!({"model": "claude/c", "max_tokens": 10, "stream": true, "messages": [{"role": "user", "content": "x"}]}))
        .await;
    assert_eq!(resp.headers()["x-loadout-route"], "claude/b/c");
    let text = resp.text().await.expect("text");
    assert!(
        !text.contains("Overloaded"),
        "pre-commit error never reaches the client"
    );
    assert!(text.contains("\"text\":\"ok\""));
}

#[tokio::test]
async fn upstream_drop_after_commit_ends_stream_with_error_event() {
    let (mock, base) = start_mock().await;
    mock.push(
        "/v1/chat/completions",
        Reply::SseThenDrop(vec![
            chat_chunk(json!({"role": "assistant"}), None),
            chat_chunk(json!({"content": "partial"}), None),
        ]),
    );
    let h = Harness::new(vec![spec("acme", "main", 0, "m", Protocol::Chat, &base)]).await;
    let text = h
        .post(
            "/v1/chat/completions",
            json!({"model": "acme/main/m", "stream": true, "messages": []}),
        )
        .await
        .text()
        .await
        .expect("text");
    assert!(text.contains("partial"));
    assert!(
        text.contains("\"error\""),
        "client sees an error event: {text}"
    );
    assert!(text.trim_end().ends_with("data: [DONE]"));
    let r = &h.records(1).await[0];
    assert_eq!(r.status, "error");
    assert_eq!(r.error_category, Some("upstream_interrupted"));
}

#[tokio::test]
async fn no_failover_header_limits_to_one_attempt() {
    let (mock_a, base_a) = start_mock().await;
    let (mock_b, base_b) = start_mock().await;
    mock_a.push("/v1/chat/completions", Reply::Status(503, "busy".into()));
    let h = Harness::new(vec![
        spec("acme", "a", 0, "m", Protocol::Chat, &base_a),
        spec("acme", "b", 1, "m", Protocol::Chat, &base_b),
    ])
    .await;
    let resp = h
        .post_with(
            "/v1/chat/completions",
            json!({"model": "acme/m", "messages": []}),
            &[("x-loadout-no-failover", "true")],
        )
        .await;
    assert_eq!(resp.status(), 502);
    assert_eq!(
        resp.json::<Value>().await.expect("json")["error"]["code"],
        "upstream_error"
    );
    assert!(mock_b.seen().is_empty());
}

#[tokio::test]
async fn stateful_responses_are_pinned_and_require_native_upstream() {
    let (mock_a, base_a) = start_mock().await;
    let (mock_b, base_b) = start_mock().await;
    mock_b.push(
        "/v1/responses",
        Reply::Sse(responses_stream_text("resp_first", "one")),
    );
    let h = Harness::new(vec![
        spec("oa", "a", 0, "gpt", Protocol::Responses, &base_a),
        spec("oa", "b", 1, "gpt", Protocol::Responses, &base_b),
    ])
    .await;
    // Make account a unavailable so the first response is created on b.
    h.health
        .get("oa-a-gpt:responses")
        .expect("h")
        .record_failure(
            loadout_provider_sdk::FailureKind::Server,
            "down",
            loadout_router::now_ms(),
        );
    let first = h
        .post(
            "/v1/responses",
            json!({"model": "oa/gpt", "input": "a", "stream": true}),
        )
        .await;
    assert_eq!(first.headers()["x-loadout-route"], "oa/b/gpt");
    let _ = first.text().await;
    assert_eq!(
        h.routes_store
            .0
            .lock()
            .expect("lock")
            .get("resp_first")
            .map(String::as_str),
        Some("oa-b-gpt:responses")
    );

    // a is healthy again, but the follow-up must go to b where the state lives.
    h.health.get("oa-a-gpt:responses").expect("h").reset();
    mock_b.push(
        "/v1/responses",
        Reply::Sse(responses_stream_text("resp_second", "two")),
    );
    let second = h
        .post("/v1/responses", json!({"model": "oa/gpt", "input": "b", "previous_response_id": "resp_first", "stream": true}))
        .await;
    assert_eq!(second.headers()["x-loadout-route"], "oa/b/gpt");
    assert_eq!(mock_b.seen()[1].body["previous_response_id"], "resp_first");
    assert!(mock_a.seen().is_empty());
}

#[tokio::test]
async fn stateful_request_without_responses_upstream_is_rejected() {
    let (_mock, base) = start_mock().await;
    let h = Harness::new(vec![spec("acme", "main", 0, "m", Protocol::Chat, &base)]).await;
    let resp = h
        .post(
            "/v1/responses",
            json!({"model": "acme/main/m", "input": "x", "previous_response_id": "resp_1"}),
        )
        .await;
    assert_eq!(resp.status(), 400);
    assert_eq!(
        resp.json::<Value>().await.expect("json")["error"]["code"],
        "unsupported_capability"
    );
}

#[tokio::test]
async fn unknown_model_and_models_listing() {
    let (_mock, base) = start_mock().await;
    let h = Harness::new(vec![
        spec("acme", "main", 0, "m1", Protocol::Chat, &base),
        spec("acme", "main", 0, "m1", Protocol::Responses, &base),
    ])
    .await;
    let resp = h
        .post(
            "/v1/chat/completions",
            json!({"model": "gpt-4o", "messages": []}),
        )
        .await;
    assert_eq!(resp.status(), 404);
    let v: Value = resp.json().await.expect("json");
    assert!(
        v["error"]["message"]
            .as_str()
            .expect("m")
            .contains("provider/account/model")
    );

    let list: Value = h
        .http
        .get(format!("http://{}/v1/models", h.addr))
        .bearer_auth(&h.key)
        .send()
        .await
        .expect("send")
        .json()
        .await
        .expect("json");
    assert_eq!(
        list["data"].as_array().map(Vec::len),
        Some(1),
        "one name per account model"
    );
    assert_eq!(list["data"][0]["id"], "acme/main/m1");
    let extra = &list["data"][0]["loadout"];
    assert_eq!(extra["kind"], "exact");
    assert_eq!(extra["tier"], "UNMEASURED");
    assert_eq!(extra["state"], "UNKNOWN");
    assert_eq!(extra["input_price"], 1.0);
    assert_eq!(extra["output_price"], 2.0);

    let anthropic: Value = h
        .http
        .get(format!("http://{}/v1/models", h.addr))
        .header("x-api-key", &h.key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
        .expect("send")
        .json()
        .await
        .expect("json");
    assert_eq!(anthropic["data"][0]["type"], "model");
}

#[tokio::test]
async fn anthropic_passthrough_forwards_beta_headers() {
    let (mock, base) = start_mock().await;
    mock.push("/v1/messages", Reply::Sse(anthropic_stream_text("x")));
    let h = Harness::new(vec![spec(
        "claude",
        "main",
        0,
        "c",
        Protocol::Messages,
        &base,
    )])
    .await;
    let _ = h
        .post_with(
            "/v1/messages",
            json!({"model": "claude/main/c", "max_tokens": 5, "stream": true, "messages": [{"role": "user", "content": "x"}]}),
            &[("anthropic-beta", "interleaved-thinking-2025-05-14"), ("anthropic-version", "2023-06-01"), ("x-custom", "no")],
        )
        .await
        .text()
        .await;
    let seen = &mock.seen()[0];
    assert_eq!(
        seen.headers["anthropic-beta"],
        "interleaved-thinking-2025-05-14"
    );
    assert!(
        seen.headers.get("x-custom").is_none(),
        "only allow-listed headers are forwarded"
    );
}

#[tokio::test]
async fn client_policy_default_model_filter_and_rate_limit() {
    let (mock, base) = start_mock().await;
    for _ in 0..6 {
        mock.push("/v1/chat/completions", Reply::Sse(chat_stream_text("ok")));
    }
    let h = Harness::new(vec![
        spec("acme", "main", 0, "m1", Protocol::Chat, &base),
        spec("acme", "main", 0, "m2", Protocol::Chat, &base),
    ])
    .await;
    h.policies.replace([(
        ClientId::from("test-client"),
        ClientPolicy {
            default_model: Some("acme/main/m1".into()),
            allowed_models: vec!["acme/main/m1".into()],
            requests_per_minute: Some(2),
            ..ClientPolicy::default()
        },
    )]);

    // A tool that always sends its own model name gets the default model.
    let resp = h
        .post(
            "/v1/chat/completions",
            json!({"model": "gpt-4o", "stream": true, "messages": [{"role": "user", "content": "hi"}]}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers()["x-loadout-route"].to_str().expect("route"),
        "acme/main/m1"
    );
    let _ = resp.text().await;

    let denied = h
        .post(
            "/v1/chat/completions",
            json!({"model": "acme/main/m2", "messages": [{"role": "user", "content": "hi"}]}),
        )
        .await;
    assert_eq!(denied.status(), 403);
    let v: Value = denied.json().await.expect("json");
    assert_eq!(v["error"]["code"], "model_not_allowed");

    // The listing only shows allowed models.
    let list: Value = h
        .http
        .get(format!("http://{}/v1/models", h.addr))
        .bearer_auth(&h.key)
        .send()
        .await
        .expect("send")
        .json()
        .await
        .expect("json");
    assert_eq!(list["data"].as_array().map(Vec::len), Some(1));

    // At most 2 requests per minute: within a few more requests one is limited (a minute
    // boundary during the test resets the window once).
    let mut limited = None;
    for _ in 0..4 {
        let resp = h
            .post(
                "/v1/chat/completions",
                json!({"model": "acme/main/m1", "stream": true, "messages": [{"role": "user", "content": "hi"}]}),
            )
            .await;
        if resp.status() == 429 {
            limited = Some(resp.json::<Value>().await.expect("json"));
            break;
        }
        assert_eq!(resp.status(), 200);
        let _ = resp.text().await;
    }
    let v = limited.expect("rate limited");
    assert_eq!(v["error"]["code"], "client_limit");
    assert!(
        h.records(4)
            .await
            .iter()
            .any(|r| r.error_category == Some("client_limit"))
    );
}

/// Wraps the OpenAI-compatible module with the hooks a stream-only subscription upstream
/// uses (ExecPlan 010 §2.1).
#[derive(Debug, Default)]
struct StreamOnly(OpenAiCompatible);

impl ProviderModule for StreamOnly {
    fn descriptor(&self) -> &'static loadout_provider_sdk::ProviderDescriptor {
        self.0.descriptor()
    }
    fn endpoint(
        &self,
        config: &ProviderConfig,
        protocol: Protocol,
        key: Option<&Secret>,
    ) -> Result<loadout_provider_sdk::Endpoint, loadout_provider_sdk::ProviderError> {
        self.0.endpoint(config, protocol, key)
    }
    fn prepare_request(&self, _config: &ProviderConfig, _protocol: Protocol, body: &mut Value) {
        body["prepared"] = Value::Bool(true);
        if let Some(o) = body.as_object_mut() {
            o.remove("temperature");
        }
    }
    fn upstream_stream_only(&self) -> bool {
        true
    }
    fn account_concurrency(&self) -> Option<u32> {
        Some(1)
    }
    /// Passive usage hook (ExecPlan 013): reports something for every SSE response.
    fn usage_from_headers(&self, headers: &HeaderMap) -> Option<UsageReport> {
        (headers.get("content-type")? == "text/event-stream")
            .then(|| UsageReport::new(UsageSource::PrivateEndpoint, Confidence::Exact))
    }
}

#[tokio::test]
async fn stream_only_upstream_serves_non_streaming_clients_and_prepares_bodies() {
    let (mock, base) = start_mock().await;
    mock.push(
        "/v1/responses",
        Reply::Sse(responses_stream_text("r1", "Hello")),
    );
    let health = Arc::new(HealthRegistry::default());
    let mut target = Arc::into_inner(build_target(
        &health,
        &spec("sub", "main", 0, "m", Protocol::Responses, &base),
    ))
    .expect("unique");
    target.module = Arc::new(StreamOnly::default());
    target.trust = TrustLevel::PrivateEndpoint;
    target.opted_in = true;
    let h = Harness::with_targets(health, vec![Arc::new(target)]).await;

    let resp = h
        .post(
            "/v1/responses",
            json!({"model": "sub/main/m", "input": "hi", "temperature": 0.5}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    let v: Value = resp.json().await.expect("json");
    assert_eq!(v["output"][0]["content"][0]["text"], "Hello");

    let seen = mock.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].body["stream"], true, "upstream always streams");
    assert_eq!(seen[0].body["prepared"], true);
    assert!(seen[0].body.get("temperature").is_none());
    assert_eq!(
        seen[0].headers["accept"], "text/event-stream",
        "streaming accept header"
    );
    let usage = h.records.1.lock().expect("lock").clone();
    assert_eq!(usage.len(), 1, "passive usage observed once");
    assert_eq!(usage[0].0, "sub-main");
}

/// An upstream that asks for a conversation id header (ADR-013).
#[derive(Debug, Default)]
struct SessionAware(OpenAiCompatible);

impl ProviderModule for SessionAware {
    fn descriptor(&self) -> &'static loadout_provider_sdk::ProviderDescriptor {
        self.0.descriptor()
    }
    fn endpoint(
        &self,
        config: &ProviderConfig,
        protocol: Protocol,
        key: Option<&Secret>,
    ) -> Result<loadout_provider_sdk::Endpoint, loadout_provider_sdk::ProviderError> {
        self.0.endpoint(config, protocol, key)
    }
    fn session_header(&self) -> Option<axum::http::HeaderName> {
        Some(axum::http::HeaderName::from_static("x-test-session"))
    }
}

#[tokio::test]
async fn session_aware_upstreams_get_a_stable_conversation_id() {
    let (mock, base) = start_mock().await;
    for _ in 0..4 {
        mock.push("/v1/chat/completions", Reply::Sse(chat_stream_text("ok")));
    }
    let health = Arc::new(HealthRegistry::default());
    let mut aware = Arc::into_inner(build_target(
        &health,
        &spec("go", "main", 0, "m", Protocol::Chat, &base),
    ))
    .expect("unique");
    aware.module = Arc::new(SessionAware::default());
    let plain = build_target(
        &health,
        &spec("acme", "main", 0, "p", Protocol::Chat, &base),
    );
    let h = Harness::with_targets(health, vec![Arc::new(aware), plain]).await;

    let turn1 = json!({"model": "go/main/m", "stream": true, "messages": [
        {"role": "system", "content": "sys"},
        {"role": "user", "content": "first question"}
    ]});
    let turn2 = json!({"model": "go/main/m", "stream": true, "messages": [
        {"role": "system", "content": "sys"},
        {"role": "user", "content": "first question"},
        {"role": "assistant", "content": "answer"},
        {"role": "user", "content": "follow-up"}
    ]});
    let other = json!({"model": "go/main/m", "stream": true, "messages": [
        {"role": "user", "content": "something else"}
    ]});
    for body in [turn1, turn2, other] {
        let resp = h.post("/v1/chat/completions", body).await;
        assert_eq!(resp.status(), 200);
        let _ = resp.text().await;
    }
    let plain_resp = h
        .post(
            "/v1/chat/completions",
            json!({"model": "acme/main/p", "stream": true, "messages": [{"role": "user", "content": "x"}]}),
        )
        .await;
    assert_eq!(plain_resp.status(), 200);
    let _ = plain_resp.text().await;

    let seen = mock.seen();
    let ids: Vec<String> = seen[..3]
        .iter()
        .map(|s| {
            s.headers["x-test-session"]
                .to_str()
                .expect("ascii")
                .to_owned()
        })
        .collect();
    assert_eq!(ids[0], ids[1], "same conversation, same id");
    assert_ne!(ids[0], ids[2], "different conversation");
    assert_eq!(ids[0].len(), 36, "uuid form: {}", ids[0]);
    assert!(
        seen[3].headers.get("x-test-session").is_none(),
        "only upstreams that ask for it"
    );
}

#[tokio::test]
async fn conversation_id_survives_protocol_conversion() {
    let (mock, base) = start_mock().await;
    mock.push("/v1/chat/completions", Reply::Sse(chat_stream_text("ok")));
    let health = Arc::new(HealthRegistry::default());
    let mut aware = Arc::into_inner(build_target(
        &health,
        &spec("go", "main", 0, "m", Protocol::Chat, &base),
    ))
    .expect("unique");
    aware.module = Arc::new(SessionAware::default());
    let h = Harness::with_targets(health, vec![Arc::new(aware)]).await;

    let resp = h
        .post_with(
            "/v1/messages",
            json!({"model": "go/main/m", "max_tokens": 16, "stream": true,
                   "messages": [{"role": "user", "content": "hi"}]}),
            &[("x-claude-code-session-id", "cc-session-1")],
        )
        .await;
    assert_eq!(resp.status(), 200);
    let _ = resp.text().await;
    let seen = mock.seen();
    let id = seen[0].headers["x-test-session"].to_str().expect("ascii");
    assert_eq!(id.len(), 36);
    assert!(!id.contains("cc-session-1"), "client id is not forwarded");
}

#[tokio::test]
async fn lower_trust_targets_need_opt_in() {
    let (_mock, base) = start_mock().await;
    let health = Arc::new(HealthRegistry::default());
    let mut target = Arc::into_inner(build_target(
        &health,
        &spec("sub", "main", 0, "m", Protocol::Chat, &base),
    ))
    .expect("unique");
    target.trust = TrustLevel::PrivateEndpoint;
    let h = Harness::with_targets(health, vec![Arc::new(target)]).await;
    let resp = h
        .post(
            "/v1/chat/completions",
            json!({"model": "sub/main/m", "messages": [{"role": "user", "content": "x"}]}),
        )
        .await;
    assert_ne!(resp.status(), 200);
    let text = resp.text().await.expect("text");
    assert!(text.contains("not allowed for inference"), "{text}");
}

#[tokio::test]
async fn models_listing_puts_virtuals_and_pools_first_with_details() {
    let (_mock, base) = start_mock().await;
    let health = Arc::new(HealthRegistry::default());
    let a = build_target(&health, &spec("acme", "a", 0, "m", Protocol::Chat, &base));
    let b = build_target(&health, &spec("acme", "b", 1, "m", Protocol::Chat, &base));
    a.health.record_failure(
        loadout_provider_sdk::FailureKind::Auth,
        "x",
        loadout_router::now_ms(),
    );
    let (a_id, b_id) = (a.id.clone(), b.id.clone());
    let table = RouteTable::new(1, vec![a, b])
        .with_virtual("default", &[a_id.clone(), b_id.clone()])
        .with_virtual("realtime", &[])
        .with_pool("coding", &[b_id, a_id]);
    let h = Harness::with_table(health, table).await;

    let list: Value = h
        .http
        .get(format!("http://{}/v1/models", h.addr))
        .bearer_auth(&h.key)
        .send()
        .await
        .expect("send")
        .json()
        .await
        .expect("json");
    assert_eq!(list["object"], "list");
    let ids: Vec<&str> = list["data"]
        .as_array()
        .expect("data")
        .iter()
        .map(|m| m["id"].as_str().expect("id"))
        .collect();
    assert_eq!(
        ids,
        vec!["loadout/default", "loadout/coding", "acme/a/m", "acme/b/m"],
        "built-in virtuals, then pools, then exact names; empty virtuals hidden"
    );
    let default = &list["data"][0];
    assert_eq!(default["object"], "model");
    assert_eq!(default["loadout"]["kind"], "virtual");
    assert_eq!(default["loadout"]["state"], "AVAILABLE");
    assert_eq!(
        default["loadout"]["head"], "acme/b/m",
        "first available target"
    );
    assert_eq!(default["loadout"]["targets"], 2);
    assert_eq!(list["data"][1]["loadout"]["kind"], "pool");
    assert_eq!(list["data"][2]["loadout"]["state"], "AUTH_FAILED");
}
