// SPDX-License-Identifier: Apache-2.0

//! Adapters from the gateway's data-plane interfaces to storage.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use loadout_gateway::{RequestRecord, ResponseRouteStore, TelemetrySink};
use loadout_provider_sdk::BoxFuture;
use loadout_router::{LatencyBook, account_model_key};
use loadout_storage::telemetry::{TelemetryEvent, TelemetrySender};
use loadout_storage::{Database, RequestEvent, now_ms};

#[derive(Debug)]
pub(crate) struct StorageTelemetry {
    pub sender: TelemetrySender,
}

fn to_i64(v: Option<u64>) -> Option<i64> {
    v.and_then(|n| i64::try_from(n).ok())
}

impl TelemetrySink for StorageTelemetry {
    fn record(&self, r: RequestRecord) {
        let event = RequestEvent {
            ts: r.ts_ms,
            client_id: r.client_id,
            requested_model: r.requested_model,
            route_target: r.route_target,
            provider: r.provider,
            account: r.account,
            upstream_model: r.upstream_model,
            protocol_in: r.protocol_in.map_or("unknown", |p| p.as_str()).to_owned(),
            protocol_out: r.protocol_out.map(|p| p.as_str().to_owned()),
            mode: r.mode.map(str::to_owned),
            stream: r.stream,
            status: r.status.to_owned(),
            error_category: r.error_category.map(str::to_owned),
            http_status: r.http_status.map(i64::from),
            attempts: i64::from(r.attempts),
            input_tokens: to_i64(r.usage.input_tokens),
            output_tokens: to_i64(r.usage.output_tokens),
            cache_read_tokens: to_i64(r.usage.cache_read_tokens),
            cache_write_tokens: to_i64(r.usage.cache_write_tokens),
            reasoning_tokens: to_i64(r.usage.reasoning_tokens),
            ttft_ms: to_i64(r.ttft_ms),
            duration_ms: i64::try_from(r.duration_ms).unwrap_or(i64::MAX),
            output_tps: r.output_tps,
            estimated_cost_usd: r.estimated_cost_usd,
            usage_confidence: Some(r.usage_confidence.to_owned()),
            decision: r.decision,
        };
        self.sender.send(TelemetryEvent::Request(Box::new(event)));
    }
}

/// Feeds completed requests into the latency statistics (passive benchmark, ExecPlan 004
/// §3.2), then forwards the record. Runs after the response, never before routing.
#[derive(Debug)]
pub(crate) struct LatencyTap<S> {
    pub latency: Arc<LatencyBook>,
    /// Client usage counters for policy limits (ExecPlan 006).
    pub policies: Arc<loadout_gateway::ClientPolicies>,
    pub inner: S,
}

impl<S: TelemetrySink> TelemetrySink for LatencyTap<S> {
    fn record(&self, r: RequestRecord) {
        if let Some(target) = &r.route_target
            && let Some(sample) = crate::latency::passive_sample(
                r.ts_ms,
                r.status,
                r.ttft_ms,
                r.usage.output_tokens,
                r.output_tps,
            )
        {
            self.latency.record(account_model_key(target), sample);
        }
        if let Some(client) = &r.client_id {
            let tokens = r.usage.input_tokens.unwrap_or(0) + r.usage.output_tokens.unwrap_or(0);
            self.policies
                .record(client, tokens, r.estimated_cost_usd.unwrap_or(0.0), r.ts_ms);
        }
        self.inner.record(r);
    }
}

const ROUTE_CACHE: usize = 10_000;

/// Recent mappings in memory; older ones from SQLite (only for stateful Responses requests
/// after a daemon restart — a controlled exception to "no database on the request path").
#[derive(Debug)]
pub(crate) struct ResponseRoutes {
    db: Arc<Database>,
    sender: TelemetrySender,
    cache: Mutex<(HashMap<String, String>, VecDeque<String>)>,
}

impl ResponseRoutes {
    pub fn new(db: Arc<Database>, sender: TelemetrySender) -> Self {
        Self {
            db,
            sender,
            cache: Mutex::new((HashMap::new(), VecDeque::new())),
        }
    }

    fn remember(&self, id: String, target: String) {
        let mut guard = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let (map, order) = &mut *guard;
        if map.insert(id.clone(), target).is_none() {
            order.push_back(id);
            while order.len() > ROUTE_CACHE {
                if let Some(old) = order.pop_front() {
                    map.remove(&old);
                }
            }
        }
    }
}

impl ResponseRouteStore for ResponseRoutes {
    fn get<'a>(&'a self, response_id: &'a str) -> BoxFuture<'a, Option<String>> {
        Box::pin(async move {
            if let Some(t) = self
                .cache
                .lock()
                .ok()
                .and_then(|c| c.0.get(response_id).cloned())
            {
                return Some(t);
            }
            let db = self.db.clone();
            let id = response_id.to_owned();
            let found = tokio::task::spawn_blocking(move || db.response_route(&id))
                .await
                .ok()?
                .ok()
                .flatten();
            if let Some(t) = &found {
                self.remember(response_id.to_owned(), t.clone());
            }
            found
        })
    }

    fn put(&self, response_id: String, route_target: String) {
        self.remember(response_id.clone(), route_target.clone());
        self.sender.send(TelemetryEvent::ResponseRoute {
            response_id,
            route_target,
            at_ms: now_ms(),
        });
    }
}
