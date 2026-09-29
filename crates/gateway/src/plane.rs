// SPDX-License-Identifier: Apache-2.0

//! What the data plane needs from the rest of the daemon, as narrow interfaces.

use std::sync::Arc;
use std::time::Duration;

use loadout_protocol::{Protocol, Usage};
use loadout_provider_sdk::{BoxFuture, HttpClient};
use loadout_router::{RouteTableHandle, SessionAffinity};

/// Metadata of one client request (ADR-008 §1). No content, no credentials.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RequestRecord {
    pub ts_ms: i64,
    pub client_id: Option<String>,
    pub requested_model: String,
    pub route_target: Option<String>,
    pub provider: Option<String>,
    pub account: Option<String>,
    pub upstream_model: Option<String>,
    pub protocol_in: Option<Protocol>,
    pub protocol_out: Option<Protocol>,
    /// `passthrough` or `converted`.
    pub mode: Option<&'static str>,
    pub stream: bool,
    /// `ok`, `error` or `client_closed`.
    pub status: &'static str,
    pub error_category: Option<&'static str>,
    pub http_status: Option<u16>,
    pub attempts: u32,
    pub usage: Usage,
    /// `EXACT`, `ESTIMATED` or `UNKNOWN`.
    pub usage_confidence: &'static str,
    pub ttft_ms: Option<u64>,
    pub duration_ms: u64,
    pub output_tps: Option<f64>,
    pub estimated_cost_usd: Option<f64>,
}

/// Receives request records. Implementations must not block (ADR-008 §5).
pub trait TelemetrySink: Send + Sync + 'static {
    fn record(&self, record: RequestRecord);
}

/// `response_id → route target` for stateful Responses conversations (ADR-006 §3).
pub trait ResponseRouteStore: Send + Sync + 'static {
    fn get<'a>(&'a self, response_id: &'a str) -> BoxFuture<'a, Option<String>>;
    fn put(&self, response_id: String, route_target: String);
}

/// Discards everything (tests, or before the daemon wires real sinks).
#[derive(Debug, Default)]
pub struct NullSink;

impl TelemetrySink for NullSink {
    fn record(&self, _record: RequestRecord) {}
}

impl ResponseRouteStore for NullSink {
    fn get<'a>(&'a self, _response_id: &'a str) -> BoxFuture<'a, Option<String>> {
        Box::pin(async { None })
    }
    fn put(&self, _response_id: String, _route_target: String) {}
}

#[derive(Clone, Copy, Debug)]
pub struct UpstreamLimits {
    /// From sending the request until the first output token (reasoning models may think
    /// for a long time before producing output).
    pub first_token_timeout: Duration,
    /// Whole-response limit for non-streaming passthrough.
    pub non_stream_timeout: Duration,
    /// Candidates tried per request (ADR-003 §7).
    pub max_attempts: usize,
}

impl Default for UpstreamLimits {
    fn default() -> Self {
        Self {
            first_token_timeout: Duration::from_secs(120),
            non_stream_timeout: Duration::from_secs(600),
            max_attempts: 3,
        }
    }
}

/// Everything the proxy handlers share.
#[derive(Clone)]
pub struct DataPlane {
    pub routes: Arc<RouteTableHandle>,
    pub http: HttpClient,
    pub telemetry: Arc<dyn TelemetrySink>,
    pub response_routes: Arc<dyn ResponseRouteStore>,
    pub limits: UpstreamLimits,
    /// `X-Loadout-Session-ID` → account model (ADR-003 §5).
    pub sessions: Arc<SessionAffinity>,
}

impl DataPlane {
    /// A plane with the given routes and HTTP client, discarding telemetry.
    pub fn new(routes: Arc<RouteTableHandle>, http: HttpClient) -> Self {
        Self {
            routes,
            http,
            telemetry: Arc::new(NullSink),
            response_routes: Arc::new(NullSink),
            limits: UpstreamLimits::default(),
            sessions: Arc::default(),
        }
    }

    /// No routes at all (tests, benchmarks).
    pub fn empty() -> Self {
        Self::new(
            Arc::default(),
            HttpClient::new(&loadout_provider_sdk::HttpConfig::default())
                .expect("default HTTP client"),
        )
    }
}

impl std::fmt::Debug for DataPlane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataPlane")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}
