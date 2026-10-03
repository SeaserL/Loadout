// SPDX-License-Identifier: Apache-2.0

//! Request lifecycle (ExecPlan 002 §5.4, ADR-003 §7, ADR-006):
//!
//! parse → resolve → select candidates → for each candidate: build the upstream request
//! (passthrough or converted), send, wait for the first output event. Any failure before
//! that point fails over to the next candidate. Once output arrives the response is
//! committed and streamed; later upstream failures end the stream with an error event.

mod decision;
mod meter;
mod render;

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use loadout_protocol::hints::{self, RouteHints};
use loadout_protocol::sse::SseParser;
use loadout_protocol::{
    Aggregator, CanonicalEvent, CanonicalRequest, Protocol, ProtocolError, chat, decoder_for,
    messages, observe, responses,
};
use loadout_provider_sdk::{FailureKind, UpstreamFailure};
use loadout_router::{
    ResolutionKind, RouteTarget, SelectRequest, account_model_key, now_ms, select,
};
use serde_json::Value;
use tokio::sync::OwnedSemaphorePermit;
use tokio::time::timeout_at;

use crate::plane::DataPlane;
use crate::{ApiError, ApiFlavor, ClientId, ErrorCategory, PolicyViolation};
use decision::{Trail, select_reason};
use meter::{Meter, RecordCtx};
use render::Renderer;

const ERROR_BODY_LIMIT: usize = 64 * 1024;
const PUMP_BUFFER: usize = 64;
/// Client headers forwarded to the upstream on passthrough (ExecPlan 002 §9).
const FORWARDED_HEADERS: [&str; 3] = ["anthropic-version", "anthropic-beta", "openai-beta"];
const SESSION_HEADER: &str = "x-loadout-session-id";

type ByteStream = Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>;

impl ApiFlavor {
    pub fn for_protocol(p: Protocol) -> Self {
        match p {
            Protocol::Messages => Self::Anthropic,
            Protocol::Chat | Protocol::Responses => Self::OpenAi,
        }
    }
}

fn error_response(
    flavor: ApiFlavor,
    category: ErrorCategory,
    message: impl Into<String>,
) -> Response {
    ApiError::new(category, message).into_response_for(flavor)
}

fn category_for(kind: FailureKind) -> ErrorCategory {
    match kind {
        FailureKind::Auth => ErrorCategory::Auth,
        FailureKind::RateLimited { .. } => ErrorCategory::RateLimit,
        FailureKind::QuotaExhausted { .. } => ErrorCategory::Quota,
        FailureKind::Server | FailureKind::Other => ErrorCategory::Upstream5xx,
        FailureKind::Timeout => ErrorCategory::UpstreamTimeout,
        FailureKind::Connect => ErrorCategory::TransportUnavailable,
        FailureKind::ModelNotFound => ErrorCategory::ModelNotFound,
        FailureKind::InvalidRequest => ErrorCategory::InvalidRequest,
    }
}

/// Maps an error event received inside an upstream stream.
fn stream_failure(kind: Option<&str>, message: &str) -> UpstreamFailure {
    let k = kind.unwrap_or("").to_ascii_lowercase();
    let m = message.to_ascii_lowercase();
    let failure_kind = if k.contains("rate_limit") {
        FailureKind::RateLimited { retry_after: None }
    } else if k.contains("quota") || k.contains("insufficient") || m.contains("quota") {
        FailureKind::QuotaExhausted { reset_after: None }
    } else if k.contains("authentication") || k.contains("permission") {
        FailureKind::Auth
    } else if k.contains("invalid_request") {
        FailureKind::InvalidRequest
    } else {
        FailureKind::Server
    };
    UpstreamFailure::new(failure_kind, None, message)
}

enum Outcome {
    Committed(Response),
    Failed(UpstreamFailure),
    /// This target cannot express the request (conversion unsupported); try the next one.
    Skipped(String),
}

struct Attempt<'a> {
    plane: &'a DataPlane,
    target: &'a Arc<RouteTarget>,
    ingress: Protocol,
    hints: &'a RouteHints,
    client_headers: &'a HeaderMap,
    ctx: &'a RecordCtx,
    attempt_no: u32,
    /// Decision steps before this attempt.
    trail: &'a Trail,
}

pub(crate) async fn handle(
    plane: Arc<DataPlane>,
    max_body: usize,
    ingress: Protocol,
    client: ClientId,
    req: Request,
) -> Response {
    let started = Instant::now();
    let ts_ms = now_ms();
    let flavor = ApiFlavor::for_protocol(ingress);
    let (parts, body) = req.into_parts();

    let bytes = match axum::body::to_bytes(body, max_body).await {
        Ok(b) => b,
        Err(_) => {
            return error_response(
                flavor,
                ErrorCategory::PayloadTooLarge,
                format!("request body exceeds {max_body} bytes"),
            );
        }
    };
    let json: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => {
            return error_response(
                flavor,
                ErrorCategory::InvalidRequest,
                format!("request body is not valid JSON: {e}"),
            );
        }
    };
    let mut hints = match hints::extract(ingress, &json, bytes.len()) {
        Ok(h) => h,
        Err(e) => return error_response(flavor, ErrorCategory::InvalidRequest, e.to_string()),
    };
    // Client policy (ExecPlan 006): default model, allowed models, limits.
    let policy = plane.policies.get(&client);
    let effective = plane
        .policies
        .effective_model(policy.as_deref(), &hints.model)
        .to_owned();
    hints.model = effective;
    let ctx = RecordCtx::new(&client, ingress, &hints, ts_ms, started);
    if let Err(violation) = plane
        .policies
        .admit(&client, policy.as_deref(), &hints.model, ts_ms)
    {
        let category = match violation {
            PolicyViolation::ModelNotAllowed(_) => ErrorCategory::ModelNotAllowed,
            _ => ErrorCategory::ClientLimit,
        };
        plane
            .telemetry
            .record(ctx.failure(None, 0, category.code(), category.status().as_u16()));
        return error_response(flavor, category, violation.to_string());
    }

    let table = plane.routes.load();
    let resolution = match table.resolve(&hints.model) {
        Ok(r) => r,
        Err(e) => {
            plane
                .telemetry
                .record(ctx.failure(None, 0, ErrorCategory::ModelNotFound.code(), 404));
            return error_response(flavor, ErrorCategory::ModelNotFound, e.to_string());
        }
    };
    let pinned = match &hints.previous_response_id {
        Some(id) => plane.response_routes.get(id).await,
        None => None,
    };
    let no_failover = parts
        .headers
        .get("x-loadout-no-failover")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("true") || v == "1");
    let session = parts
        .headers
        .get(SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let preferred = session.and_then(|s| plane.sessions.get(s, ts_ms));
    let candidates = select(
        &resolution,
        &SelectRequest {
            ingress,
            hints: &hints,
            now_ms: now_ms(),
            max_attempts: if no_failover {
                1
            } else {
                plane.limits.max_attempts
            },
            pinned_target: pinned.as_deref(),
            allow_account_failover: true,
            preferred: preferred.as_deref(),
        },
    );

    // Decision trail (metadata only): targets selection excluded, then each attempt.
    let mut trail = Trail::default();
    for (id, why) in &candidates.skipped {
        if let Some(t) = table.target(id) {
            trail.skipped(t, select_reason(t, why));
        }
    }

    if candidates.attempts.is_empty() {
        let (category, message) = if resolution.kind == ResolutionKind::Virtual
            && resolution.candidates.is_empty()
        {
            (
                ErrorCategory::NoEligibleTarget,
                format!(
                    "{} has no routes yet: loadout/realtime and loadout/fast only contain \
                     measured routes (run `loadout bench`), pools need members",
                    hints.model
                ),
            )
        } else if hints.is_stateful() {
            (
                ErrorCategory::UnsupportedCapability,
                format!(
                    "stateful Responses requests (previous_response_id) need a native Responses \
                     upstream; none is available for {}",
                    hints.model
                ),
            )
        } else {
            let reasons: Vec<String> = candidates
                .skipped
                .iter()
                .map(|(id, why)| {
                    let name = table
                        .target(id)
                        .map_or_else(|| id.to_string(), |t| t.exact_name());
                    format!("{name}: {why}")
                })
                .collect();
            (
                ErrorCategory::NoEligibleTarget,
                format!(
                    "no available route for {}: {}",
                    hints.model,
                    reasons.join("; ")
                ),
            )
        };
        let mut record = ctx.failure(None, 0, category.code(), category.status().as_u16());
        record.decision = trail.finish(None);
        plane.telemetry.record(record);
        return error_response(flavor, category, message);
    }

    let mut canonical: Option<Result<CanonicalRequest, ProtocolError>> = None;
    let mut last: Option<(UpstreamFailure, Arc<RouteTarget>)> = None;
    let mut skipped: Vec<String> = Vec::new();
    let mut attempts = 0u32;

    for target in &candidates.attempts {
        let permit = match target.module.account_concurrency() {
            Some(limit) => match plane.account_slots.try_acquire(&target.account_id, limit) {
                Some(p) => Some(p),
                None => {
                    skipped.push(format!(
                        "{}: account concurrency limit",
                        target.exact_name()
                    ));
                    trail.skipped(target, "busy");
                    continue;
                }
            },
            None => None,
        };
        let mut body = if target.protocol == ingress {
            let mut body = json.clone();
            let inject =
                hints::rewrite_for_passthrough(ingress, &mut body, &target.upstream_model, &hints);
            Prepared::Passthrough { body, inject }
        } else {
            let decoded = canonical.get_or_insert_with(|| decode(ingress, &json));
            match decoded {
                Ok(c) => Prepared::Converted(encode(c, target)),
                Err(e) => {
                    skipped.push(format!("{}: {e}", target.exact_name()));
                    trail.skipped(target, "cannot convert");
                    continue;
                }
            }
        };
        if target.module.upstream_stream_only()
            && let Prepared::Passthrough { body, .. } = &mut body
            && let Some(obj) = body.as_object_mut()
        {
            obj.insert("stream".into(), Value::Bool(true));
        }
        match &mut body {
            Prepared::Passthrough { body, .. } | Prepared::Converted(body) => {
                target
                    .module
                    .prepare_request(&target.config, target.protocol, body);
            }
        }
        attempts += 1;
        let attempt = Attempt {
            plane: &plane,
            target,
            ingress,
            hints: &hints,
            client_headers: &parts.headers,
            ctx: &ctx,
            attempt_no: attempts,
            trail: &trail,
        };
        match attempt.run(body, permit).await {
            Outcome::Committed(resp) => {
                if let Some(s) = session {
                    plane
                        .sessions
                        .put(s, Arc::from(account_model_key(&target.id)), now_ms());
                }
                return resp;
            }
            Outcome::Skipped(why) => {
                skipped.push(why);
                trail.skipped(target, "encode failed");
            }
            Outcome::Failed(failure) => {
                trail.failed(target, &failure);
                tracing::info!(
                    target = %target.exact_name(),
                    kind = failure.kind.code(),
                    status = ?failure.status,
                    "upstream attempt failed"
                );
                target
                    .health
                    .record_failure(failure.kind, &failure.message, now_ms());
                if !failure.kind.fails_over() {
                    last = Some((failure, target.clone()));
                    break;
                }
                last = Some((failure, target.clone()));
            }
        }
    }

    let (category, message, target) = match last {
        Some((failure, target)) => (
            category_for(failure.kind),
            format!(
                "{} ({}): {}",
                target.exact_name(),
                failure.kind.code(),
                failure
            ),
            Some(target),
        ),
        None => (
            ErrorCategory::UnsupportedCapability,
            format!(
                "request cannot be served by any route: {}",
                skipped.join("; ")
            ),
            None,
        ),
    };
    let mut record = ctx.failure(
        target.as_deref(),
        attempts,
        category.code(),
        category.status().as_u16(),
    );
    record.decision = trail.finish(None);
    plane.telemetry.record(record);
    error_response(flavor, category, message)
}

enum Prepared {
    Passthrough { body: Value, inject: bool },
    Converted(Value),
}

fn decode(ingress: Protocol, json: &Value) -> Result<CanonicalRequest, ProtocolError> {
    match ingress {
        Protocol::Chat => chat::decode_request(json),
        Protocol::Messages => messages::decode_request(json),
        Protocol::Responses => responses::decode_request(json),
    }
}

fn encode(c: &CanonicalRequest, target: &RouteTarget) -> Value {
    let model = &target.upstream_model;
    match target.protocol {
        Protocol::Chat => {
            chat::encode_request(c, model, target.module.chat_options(&target.config))
        }
        Protocol::Messages => {
            messages::encode_request(c, model, target.module.messages_options(&target.config))
        }
        Protocol::Responses => responses::encode_request(c, model),
    }
}

async fn read_error_body(resp: reqwest::Response) -> String {
    let mut stream = resp.bytes_stream();
    let mut buf = Vec::new();
    let read = async {
        while let Some(Ok(chunk)) = stream.next().await {
            buf.extend_from_slice(&chunk);
            if buf.len() >= ERROR_BODY_LIMIT {
                break;
            }
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(10), read).await;
    String::from_utf8_lossy(&buf).into_owned()
}

fn route_header(target: &RouteTarget) -> Option<HeaderValue> {
    HeaderValue::from_str(&target.exact_name()).ok()
}

impl Attempt<'_> {
    async fn run(&self, prepared: Prepared, permit: Option<OwnedSemaphorePermit>) -> Outcome {
        let (body, mode, upstream_stream, renderer) = match prepared {
            // A stream-only upstream answers non-streaming clients through the aggregator.
            Prepared::Passthrough { body, inject } => (
                body,
                "passthrough",
                self.hints.stream || self.target.module.upstream_stream_only(),
                Renderer::passthrough(self.ingress, inject),
            ),
            Prepared::Converted(body) => (
                body,
                "converted",
                true,
                Renderer::converted(
                    self.ingress,
                    &self.hints.model,
                    self.hints.client_wants_usage,
                ),
            ),
        };
        let body = match serde_json::to_vec(&body) {
            Ok(b) => b,
            Err(e) => return Outcome::Skipped(format!("encoding failed: {e}")),
        };

        let endpoint = &self.target.endpoint;
        let mut headers = endpoint.headers.clone();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_static(if upstream_stream {
                "text/event-stream"
            } else {
                "application/json"
            }),
        );
        if mode == "passthrough" {
            for name in FORWARDED_HEADERS {
                if let Some(v) = self.client_headers.get(name) {
                    headers.insert(name, v.clone());
                }
            }
        }

        let _in_flight = self.target.health.begin();
        let sent_at = Instant::now();
        let deadline = tokio::time::Instant::now() + self.plane.limits.first_token_timeout;
        let send = self
            .plane
            .http
            .for_url(&endpoint.url)
            .post(&endpoint.url)
            .headers(headers)
            .body(body)
            .send();
        let resp = match timeout_at(deadline, send).await {
            Err(_) => {
                return Outcome::Failed(UpstreamFailure::new(
                    FailureKind::Timeout,
                    None,
                    "no response from upstream in time",
                ));
            }
            Ok(Err(e)) => return Outcome::Failed(UpstreamFailure::from_reqwest(&e)),
            Ok(Ok(r)) => r,
        };
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let resp_headers = resp.headers().clone();
            let text = read_error_body(resp).await;
            return Outcome::Failed(self.target.module.classify(status, &resp_headers, &text));
        }

        if !upstream_stream {
            return self.passthrough_complete(resp, status, sent_at).await;
        }
        self.stream(resp, renderer, mode, sent_at, deadline, permit)
            .await
    }

    /// Non-streaming passthrough: forward the body unchanged.
    async fn passthrough_complete(
        &self,
        resp: reqwest::Response,
        status: u16,
        sent_at: Instant,
    ) -> Outcome {
        let content_type = resp.headers().get(header::CONTENT_TYPE).cloned();
        let body =
            match tokio::time::timeout(self.plane.limits.non_stream_timeout, resp.bytes()).await {
                Err(_) => {
                    return Outcome::Failed(UpstreamFailure::new(
                        FailureKind::Timeout,
                        None,
                        "upstream response did not complete in time",
                    ));
                }
                Ok(Err(e)) => return Outcome::Failed(UpstreamFailure::from_reqwest(&e)),
                Ok(Ok(b)) => b,
            };
        self.target.health.record_success(now_ms());
        let mut meter = Meter::new(sent_at);
        if let Ok(v) = serde_json::from_slice::<Value>(&body) {
            if let Some(u) = observe::usage_from_response(self.ingress, &v) {
                meter.observe(&[CanonicalEvent::Usage(u)]);
            }
            if self.ingress == Protocol::Responses
                && let Some(id) = observe::response_id(&v)
            {
                self.plane
                    .response_routes
                    .put(id.to_owned(), self.target.id.to_string());
            }
        }
        let mut record = self.trail.stamp(
            self.ctx.completed(
                self.target,
                "passthrough",
                self.attempt_no,
                &meter,
                "ok",
                None,
            ),
            self.target,
        );
        record.ttft_ms = None;
        self.plane.telemetry.record(record);

        let mut response =
            (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), body).into_response();
        let h = response.headers_mut();
        h.insert(
            header::CONTENT_TYPE,
            content_type.unwrap_or(HeaderValue::from_static("application/json")),
        );
        if let Some(v) = route_header(self.target) {
            h.insert("x-loadout-route", v);
        }
        Outcome::Committed(response)
    }

    /// Streaming upstream: wait for the first output event (failover still possible), then
    /// commit.
    async fn stream(
        &self,
        resp: reqwest::Response,
        mut renderer: Renderer,
        mode: &'static str,
        sent_at: Instant,
        deadline: tokio::time::Instant,
        permit: Option<OwnedSemaphorePermit>,
    ) -> Outcome {
        let mut upstream: ByteStream = Box::pin(resp.bytes_stream());
        let mut parser = SseParser::new();
        let mut decoder = decoder_for(self.target.protocol);
        let mut meter = Meter::new(sent_at);
        // Non-streaming clients only see the result at the very end, so everything before
        // that is still pre-commit and may fail over.
        let mut aggregator = (!self.hints.stream).then(Aggregator::new);
        let mut buffered: Vec<Bytes> = Vec::new();
        let mut ended = false;

        loop {
            let chunk = match timeout_at(deadline, upstream.next()).await {
                Err(_) if aggregator.is_none() || !meter.committed() => {
                    return Outcome::Failed(UpstreamFailure::new(
                        FailureKind::Timeout,
                        None,
                        "no output from upstream in time",
                    ));
                }
                // Aggregating a response that already started: keep reading past the
                // first-token deadline (the HTTP read timeout still bounds silence).
                Err(_) => upstream.next().await,
                Ok(next) => next,
            };
            let events = match chunk {
                None => {
                    ended = true;
                    parser.finish().into_iter().collect::<Vec<_>>()
                }
                Some(Err(e)) => return Outcome::Failed(UpstreamFailure::from_reqwest(&e)),
                Some(Ok(bytes)) => parser.feed(&bytes),
            };
            for event in &events {
                let canonical = match decoder.feed(event) {
                    Ok(c) => c,
                    Err(e) => {
                        return Outcome::Failed(UpstreamFailure::new(
                            FailureKind::Server,
                            None,
                            e.to_string(),
                        ));
                    }
                };
                if let Some(CanonicalEvent::Error { message, kind }) = canonical
                    .iter()
                    .find(|c| matches!(c, CanonicalEvent::Error { .. }))
                {
                    return Outcome::Failed(stream_failure(kind.as_deref(), message));
                }
                meter.observe(&canonical);
                match &mut aggregator {
                    Some(agg) => canonical.iter().for_each(|c| agg.push(c)),
                    None => {
                        let bytes = renderer.render(event, &canonical);
                        if !bytes.is_empty() {
                            buffered.push(bytes);
                        }
                    }
                }
            }
            if ended || decoder.is_done() {
                ended = true;
                break;
            }
            if aggregator.is_none() && meter.committed() {
                break;
            }
        }

        if !meter.saw_event {
            return Outcome::Failed(UpstreamFailure::new(
                FailureKind::Server,
                None,
                "upstream returned an empty stream",
            ));
        }
        self.target.health.record_success(now_ms());
        if self.ingress == Protocol::Responses
            && self.target.protocol == Protocol::Responses
            && let Some(id) = &meter.response_id
        {
            self.plane
                .response_routes
                .put(id.clone(), self.target.id.to_string());
        }

        if let Some(agg) = aggregator {
            return Outcome::Committed(self.aggregated_response(agg, mode, &meter));
        }

        // Commit: send what we have, then keep pumping in the background.
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, io::Error>>(PUMP_BUFFER);
        let pump = Pump {
            plane: self.plane.clone(),
            target: self.target.clone(),
            ctx_record: CompletionInfo {
                mode,
                attempt_no: self.attempt_no,
            },
            ctx: RecordSnapshot::from(self.ctx),
            trail: self.trail.clone(),
            _permit: permit,
        };
        let upstream = if ended { None } else { Some(upstream) };
        tokio::spawn(pump.run(upstream, parser, decoder, renderer, meter, tx));

        let head = futures_util::stream::iter(buffered.into_iter().map(Ok::<_, io::Error>));
        let tail = futures_util::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        });
        let mut response = Response::new(Body::from_stream(head.chain(tail)));
        let h = response.headers_mut();
        h.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream"),
        );
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        h.insert("x-accel-buffering", HeaderValue::from_static("no"));
        if let Some(v) = route_header(self.target) {
            h.insert("x-loadout-route", v);
        }
        Outcome::Committed(response)
    }

    fn aggregated_response(&self, agg: Aggregator, mode: &'static str, meter: &Meter) -> Response {
        let upstream_error = agg.error().map(str::to_owned);
        let r = agg.finish();
        let body = match self.ingress {
            Protocol::Chat => chat::encode_response(&r, &self.hints.model),
            Protocol::Messages => messages::encode_response(&r, &self.hints.model),
            Protocol::Responses => responses::encode_response(&r, &self.hints.model),
        };
        let record = self.ctx.completed(
            self.target,
            mode,
            self.attempt_no,
            meter,
            if upstream_error.is_some() {
                "error"
            } else {
                "ok"
            },
            upstream_error.as_ref().map(|_| "upstream_error"),
        );
        self.plane
            .telemetry
            .record(self.trail.stamp(record, self.target));
        let mut response = axum::Json(body).into_response();
        if let Some(v) = route_header(self.target) {
            response.headers_mut().insert("x-loadout-route", v);
        }
        response
    }
}

#[derive(Clone, Copy, Debug)]
struct CompletionInfo {
    mode: &'static str,
    attempt_no: u32,
}

/// Owned copy of the record context for the background pump.
#[derive(Debug)]
struct RecordSnapshot(RecordCtx);

impl From<&RecordCtx> for RecordSnapshot {
    fn from(c: &RecordCtx) -> Self {
        Self(RecordCtx {
            started: c.started,
            ts_ms: c.ts_ms,
            client_id: c.client_id.clone(),
            requested_model: c.requested_model.clone(),
            ingress: c.ingress,
            stream: c.stream,
            estimated_input_tokens: c.estimated_input_tokens,
        })
    }
}

struct Pump {
    plane: DataPlane,
    target: Arc<RouteTarget>,
    ctx_record: CompletionInfo,
    ctx: RecordSnapshot,
    /// Decision steps before the committed attempt.
    trail: Trail,
    /// Account concurrency slot, held until the stream ends.
    _permit: Option<OwnedSemaphorePermit>,
}

impl Pump {
    async fn run(
        self,
        upstream: Option<ByteStream>,
        mut parser: SseParser,
        mut decoder: Box<dyn loadout_protocol::StreamDecoder>,
        mut renderer: Renderer,
        mut meter: Meter,
        tx: tokio::sync::mpsc::Sender<Result<Bytes, io::Error>>,
    ) {
        let _in_flight = self.target.health.begin();
        let mut status: &'static str = "ok";
        let mut error_category: Option<&'static str> = None;

        if let Some(mut upstream) = upstream {
            'outer: loop {
                let mut upstream_ended = false;
                let events = match upstream.next().await {
                    None => {
                        upstream_ended = true;
                        parser.finish().into_iter().collect::<Vec<_>>()
                    }
                    Some(Ok(bytes)) => parser.feed(&bytes),
                    Some(Err(e)) => {
                        let failure = UpstreamFailure::from_reqwest(&e);
                        tracing::warn!(target = %self.target.exact_name(), error = %failure, "upstream stream broke after commit");
                        self.target
                            .health
                            .record_failure(failure.kind, &failure.message, now_ms());
                        status = "error";
                        error_category = Some("upstream_interrupted");
                        let _ = tx
                            .send(Ok(renderer.error("upstream connection interrupted")))
                            .await;
                        break;
                    }
                };
                for event in &events {
                    let canonical = decoder.feed(event).unwrap_or_default();
                    if canonical
                        .iter()
                        .any(|c| matches!(c, CanonicalEvent::Error { .. }))
                    {
                        status = "error";
                        error_category = Some("upstream_error");
                    }
                    meter.observe(&canonical);
                    let bytes = renderer.render(event, &canonical);
                    if !bytes.is_empty() && tx.send(Ok(bytes)).await.is_err() {
                        status = "client_closed";
                        break 'outer;
                    }
                }
                if decoder.is_done() || upstream_ended {
                    break;
                }
            }
        }
        if status != "client_closed" && status != "error" {
            let tail = renderer.finish();
            if !tail.is_empty() && tx.send(Ok(tail)).await.is_err() {
                status = "client_closed";
            }
        }
        let record = self.ctx.0.completed(
            &self.target,
            self.ctx_record.mode,
            self.ctx_record.attempt_no,
            &meter,
            status,
            error_category,
        );
        self.plane
            .telemetry
            .record(self.trail.stamp(record, &self.target));
    }
}
