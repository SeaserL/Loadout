// SPDX-License-Identifier: Apache-2.0

//! Observes a response to measure TTFT, usage and throughput, and assembles the telemetry
//! record.

use std::time::Instant;

use loadout_protocol::hints::RouteHints;
use loadout_protocol::{CanonicalEvent, Protocol, Usage};
use loadout_router::RouteTarget;

use crate::plane::RequestRecord;

#[derive(Debug)]
pub(crate) struct Meter {
    pub sent_at: Instant,
    pub first_content: Option<Instant>,
    pub usage: Usage,
    pub saw_usage: bool,
    pub saw_event: bool,
    pub output_chars: usize,
    pub response_id: Option<String>,
}

impl Meter {
    pub fn new(sent_at: Instant) -> Self {
        Self {
            sent_at,
            first_content: None,
            usage: Usage::default(),
            saw_usage: false,
            saw_event: false,
            output_chars: 0,
            response_id: None,
        }
    }

    pub fn observe(&mut self, events: &[CanonicalEvent]) {
        for e in events {
            self.saw_event = true;
            if e.is_content() && self.first_content.is_none() {
                self.first_content = Some(Instant::now());
            }
            match e {
                CanonicalEvent::Start { id, .. } => {
                    if self.response_id.is_none() {
                        self.response_id.clone_from(id);
                    }
                }
                CanonicalEvent::TextDelta(t) | CanonicalEvent::ReasoningDelta(t) => {
                    self.output_chars += t.len();
                }
                CanonicalEvent::ToolCallArgs { delta, .. } => self.output_chars += delta.len(),
                CanonicalEvent::Usage(u) => {
                    self.usage.merge(u);
                    self.saw_usage = true;
                }
                _ => {}
            }
        }
    }

    pub fn committed(&self) -> bool {
        self.first_content.is_some()
    }
}

/// Per-request telemetry context.
#[derive(Debug)]
pub(crate) struct RecordCtx {
    pub started: Instant,
    pub ts_ms: i64,
    pub client_id: String,
    pub requested_model: String,
    pub ingress: Protocol,
    pub stream: bool,
    pub estimated_input_tokens: u32,
}

impl RecordCtx {
    pub fn new(
        client_id: &str,
        ingress: Protocol,
        hints: &RouteHints,
        ts_ms: i64,
        started: Instant,
    ) -> Self {
        Self {
            started,
            ts_ms,
            client_id: client_id.to_owned(),
            requested_model: hints.model.clone(),
            ingress,
            stream: hints.stream,
            estimated_input_tokens: hints.estimated_input_tokens,
        }
    }

    fn base(&self, attempts: u32) -> RequestRecord {
        RequestRecord {
            ts_ms: self.ts_ms,
            client_id: Some(self.client_id.clone()),
            requested_model: self.requested_model.clone(),
            protocol_in: Some(self.ingress),
            stream: self.stream,
            attempts,
            usage_confidence: "UNKNOWN",
            duration_ms: self.started.elapsed().as_millis() as u64,
            status: "error",
            ..RequestRecord::default()
        }
    }

    pub fn failure(
        &self,
        target: Option<&RouteTarget>,
        attempts: u32,
        category: &'static str,
        http_status: u16,
    ) -> RequestRecord {
        let mut r = self.base(attempts);
        if let Some(t) = target {
            fill_target(&mut r, t);
        }
        r.error_category = Some(category);
        r.http_status = Some(http_status);
        r
    }

    /// Record for a request that reached the client. `status` is `ok`, `error`
    /// (upstream broke after commit) or `client_closed`.
    pub fn completed(
        &self,
        target: &RouteTarget,
        mode: &'static str,
        attempts: u32,
        meter: &Meter,
        status: &'static str,
        error_category: Option<&'static str>,
    ) -> RequestRecord {
        let mut r = self.base(attempts);
        fill_target(&mut r, target);
        r.mode = Some(mode);
        r.status = status;
        r.error_category = error_category;
        r.http_status = Some(200);

        let mut usage = meter.usage;
        r.usage_confidence = if meter.saw_usage && usage.output_tokens.is_some() {
            "EXACT"
        } else {
            // Rough estimate: ~4 characters per token.
            usage.input_tokens = usage
                .input_tokens
                .or(Some(u64::from(self.estimated_input_tokens)));
            usage.output_tokens = usage
                .output_tokens
                .or(Some((meter.output_chars as u64).div_ceil(4)));
            "ESTIMATED"
        };
        r.usage = usage;
        r.ttft_ms = meter
            .first_content
            .map(|t| t.duration_since(meter.sent_at).as_millis() as u64);
        if let (Some(first), Some(out)) = (meter.first_content, usage.output_tokens) {
            let secs = first.elapsed().as_secs_f64();
            if secs > 0.05 && out > 0 {
                r.output_tps = Some(out as f64 / secs);
            }
        }
        r.estimated_cost_usd = target.prices.cost(
            usage.input_tokens.unwrap_or(0),
            usage.output_tokens.unwrap_or(0),
            usage.cache_read_tokens.unwrap_or(0),
            usage.cache_write_tokens.unwrap_or(0),
        );
        r
    }
}

fn fill_target(r: &mut RequestRecord, t: &RouteTarget) {
    r.route_target = Some(t.id.to_string());
    r.provider = Some(t.provider_slug.to_string());
    r.account = Some(t.account_alias.to_string());
    r.upstream_model = Some(t.upstream_model.to_string());
    r.protocol_out = Some(t.protocol);
}
