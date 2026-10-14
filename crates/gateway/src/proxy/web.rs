// SPDX-License-Identifier: Apache-2.0

//! Browser-session targets (ADR-014, ExecPlan 017 §5.3): the request is decoded to canonical
//! form, typed into the account's web page by its session, and the page's answer comes back
//! as canonical events. Same commit rule as HTTP: anything before the first output may fail
//! over; afterwards the answer streams to the client.

use std::io;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use futures_util::StreamExt;
use loadout_protocol::{Aggregator, CanonicalEvent, CanonicalRequest};
use loadout_provider_sdk::web::{SessionError, SessionRequest, SessionStream};
use loadout_provider_sdk::{FailureKind, UpstreamFailure};
use loadout_router::{RouteTarget, now_ms};
use tokio::sync::OwnedSemaphorePermit;
use tokio::time::timeout_at;

use super::render::Renderer;
use super::{
    Attempt, CompletionInfo, Outcome, PUMP_BUFFER, RecordSnapshot, route_header, stream_failure,
};
use crate::plane::DataPlane;
use crate::proxy::decision::Trail;
use crate::proxy::meter::Meter;

const MODE: &str = "web";

impl Attempt<'_> {
    pub(super) async fn run_web(
        &self,
        request: &CanonicalRequest,
        conversation: Option<String>,
        permit: Option<OwnedSemaphorePermit>,
    ) -> Outcome {
        let Some(binding) = self.target.web.as_ref() else {
            return Outcome::Skipped(format!("{}: no web session", self.target.exact_name()));
        };
        let timeout = self
            .target
            .module
            .first_token_timeout()
            .unwrap_or(self.plane.limits.first_token_timeout);
        let _in_flight = self.target.health.begin();
        let sent_at = Instant::now();
        let deadline = tokio::time::Instant::now() + timeout;
        let execute = binding.upstream.clone().execute(SessionRequest {
            request: request.clone(),
            model: self.target.upstream_model.to_string(),
            conversation,
        });
        let mut stream: SessionStream = match timeout_at(deadline, execute).await {
            Err(_) => {
                return Outcome::Failed(UpstreamFailure::new(
                    FailureKind::Timeout,
                    None,
                    "the web page did not start an answer in time",
                ));
            }
            Ok(Err(SessionError::Busy(why))) => {
                return Outcome::Skipped(format!("{}: {why}", self.target.exact_name()));
            }
            Ok(Err(SessionError::Unsupported(why))) => {
                return Outcome::Skipped(format!("{}: {why}", self.target.exact_name()));
            }
            Ok(Err(SessionError::Failed(f))) => return Outcome::Failed(f),
            Ok(Ok(s)) => s,
        };

        let mut renderer = Renderer::converted(
            self.ingress,
            &self.hints.model,
            self.hints.client_wants_usage,
        );
        let mut meter = Meter::new(sent_at);
        let mut aggregator = (!self.hints.stream).then(Aggregator::new);
        let mut buffered: Vec<Bytes> = Vec::new();
        let mut ended = false;
        loop {
            let next = match timeout_at(deadline, stream.next()).await {
                Err(_) if aggregator.is_none() || !meter.committed() => {
                    return Outcome::Failed(UpstreamFailure::new(
                        FailureKind::Timeout,
                        None,
                        "no output from the web page in time",
                    ));
                }
                // Aggregating an answer that already started: keep reading.
                Err(_) => stream.next().await,
                Ok(next) => next,
            };
            let Some(event) = next else {
                ended = true;
                break;
            };
            if let CanonicalEvent::Error { message, kind } = &event
                && (aggregator.is_some() || !meter.committed())
            {
                return Outcome::Failed(stream_failure(kind.as_deref(), message));
            }
            let events = std::slice::from_ref(&event);
            meter.observe(events);
            match &mut aggregator {
                Some(agg) => agg.push(&event),
                None => {
                    let bytes = renderer.render_events(events);
                    if !bytes.is_empty() {
                        buffered.push(bytes);
                    }
                }
            }
            if aggregator.is_none() && meter.committed() {
                break;
            }
        }
        if !meter.saw_event || (ended && !meter.committed()) {
            return Outcome::Failed(UpstreamFailure::new(
                FailureKind::Server,
                None,
                "the web page returned an empty answer",
            ));
        }
        self.target.health.record_success(now_ms());
        if let Some(agg) = aggregator {
            return Outcome::Committed(self.aggregated_response(agg, MODE, &meter));
        }

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, io::Error>>(PUMP_BUFFER);
        let pump = WebPump {
            plane: self.plane.clone(),
            target: self.target.clone(),
            info: CompletionInfo {
                mode: MODE,
                attempt_no: self.attempt_no,
            },
            ctx: RecordSnapshot::from(self.ctx),
            trail: self.trail.clone(),
            _permit: permit,
        };
        let rest = if ended { None } else { Some(stream) };
        tokio::spawn(pump.run(rest, renderer, meter, tx));

        let head = futures_util::stream::iter(buffered.into_iter().map(Ok::<_, io::Error>));
        let tail = futures_util::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        });
        let mut response =
            axum::response::Response::new(axum::body::Body::from_stream(head.chain(tail)));
        let h = response.headers_mut();
        h.insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("text/event-stream"),
        );
        h.insert(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-cache"),
        );
        h.insert(
            "x-accel-buffering",
            axum::http::HeaderValue::from_static("no"),
        );
        if let Some(v) = route_header(self.target) {
            h.insert("x-loadout-route", v);
        }
        Outcome::Committed(response)
    }
}

struct WebPump {
    plane: DataPlane,
    target: Arc<RouteTarget>,
    info: CompletionInfo,
    ctx: RecordSnapshot,
    trail: Trail,
    /// Account concurrency slot, held until the answer ends.
    _permit: Option<OwnedSemaphorePermit>,
}

impl WebPump {
    async fn run(
        self,
        rest: Option<SessionStream>,
        mut renderer: Renderer,
        mut meter: Meter,
        tx: tokio::sync::mpsc::Sender<Result<Bytes, io::Error>>,
    ) {
        let _in_flight = self.target.health.begin();
        let mut status: &'static str = "ok";
        let mut error_category: Option<&'static str> = None;
        if let Some(mut stream) = rest {
            // Dropping `stream` (client gone) stops the answer on the page.
            while let Some(event) = stream.next().await {
                if matches!(event, CanonicalEvent::Error { .. }) {
                    status = "error";
                    error_category = Some("upstream_error");
                }
                let events = std::slice::from_ref(&event);
                meter.observe(events);
                let bytes = renderer.render_events(events);
                if !bytes.is_empty() && tx.send(Ok(bytes)).await.is_err() {
                    status = "client_closed";
                    break;
                }
            }
        }
        if status == "ok" {
            let tail = renderer.finish();
            if !tail.is_empty() && tx.send(Ok(tail)).await.is_err() {
                status = "client_closed";
            }
        }
        let record = self.ctx.0.completed(
            &self.target,
            self.info.mode,
            self.info.attempt_no,
            &meter,
            status,
            error_category,
        );
        self.plane
            .telemetry
            .record(self.trail.stamp(record, &self.target));
    }
}
