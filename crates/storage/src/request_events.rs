// SPDX-License-Identifier: Apache-2.0

//! Request metadata (ADR-008 §1). Never contains prompts, responses or credentials.

use rusqlite::{Connection, Row, params};

use crate::{Database, StorageError};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RequestEvent {
    pub ts: i64,
    pub client_id: Option<String>,
    pub requested_model: String,
    pub route_target: Option<String>,
    pub provider: Option<String>,
    pub account: Option<String>,
    pub upstream_model: Option<String>,
    pub protocol_in: String,
    pub protocol_out: Option<String>,
    /// `passthrough` or `converted`.
    pub mode: Option<String>,
    pub stream: bool,
    /// `ok`, `error` or `client_closed`.
    pub status: String,
    pub error_category: Option<String>,
    pub http_status: Option<i64>,
    pub attempts: i64,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub ttft_ms: Option<i64>,
    pub duration_ms: i64,
    pub output_tps: Option<f64>,
    pub estimated_cost_usd: Option<f64>,
    /// `EXACT`, `ESTIMATED` or `UNKNOWN`.
    pub usage_confidence: Option<String>,
}

const COLUMNS: &str = "ts, client_id, requested_model, route_target, provider, account, \
     upstream_model, protocol_in, protocol_out, mode, stream, status, error_category, \
     http_status, attempts, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, \
     reasoning_tokens, ttft_ms, duration_ms, output_tps, estimated_cost_usd, usage_confidence";

fn map_event(r: &Row<'_>) -> rusqlite::Result<RequestEvent> {
    Ok(RequestEvent {
        ts: r.get(0)?,
        client_id: r.get(1)?,
        requested_model: r.get(2)?,
        route_target: r.get(3)?,
        provider: r.get(4)?,
        account: r.get(5)?,
        upstream_model: r.get(6)?,
        protocol_in: r.get(7)?,
        protocol_out: r.get(8)?,
        mode: r.get(9)?,
        stream: r.get::<_, i64>(10)? == 1,
        status: r.get(11)?,
        error_category: r.get(12)?,
        http_status: r.get(13)?,
        attempts: r.get(14)?,
        input_tokens: r.get(15)?,
        output_tokens: r.get(16)?,
        cache_read_tokens: r.get(17)?,
        cache_write_tokens: r.get(18)?,
        reasoning_tokens: r.get(19)?,
        ttft_ms: r.get(20)?,
        duration_ms: r.get(21)?,
        output_tps: r.get(22)?,
        estimated_cost_usd: r.get(23)?,
        usage_confidence: r.get(24)?,
    })
}

/// Inserts events in one transaction (used by the telemetry writer's own connection).
pub(crate) fn insert_batch(conn: &mut Connection, events: &[RequestEvent]) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare_cached(&format!(
            "INSERT INTO request_events ({COLUMNS}) VALUES
             (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18,
              ?19, ?20, ?21, ?22, ?23, ?24, ?25)"
        ))?;
        for e in events {
            stmt.execute(params![
                e.ts,
                e.client_id,
                e.requested_model,
                e.route_target,
                e.provider,
                e.account,
                e.upstream_model,
                e.protocol_in,
                e.protocol_out,
                e.mode,
                i64::from(e.stream),
                e.status,
                e.error_category,
                e.http_status,
                e.attempts,
                e.input_tokens,
                e.output_tokens,
                e.cache_read_tokens,
                e.cache_write_tokens,
                e.reasoning_tokens,
                e.ttft_ms,
                e.duration_ms,
                e.output_tps,
                e.estimated_cost_usd,
                e.usage_confidence,
            ])?;
        }
    }
    tx.commit()
}

impl Database {
    /// Most recent events first.
    pub fn recent_requests(&self, limit: usize) -> Result<Vec<RequestEvent>, StorageError> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM request_events ORDER BY ts DESC, id DESC LIMIT ?1"
            ))?;
            let rows = stmt.query_map(params![limit as i64], map_event)?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    /// Deletes events older than `before_ms`; returns the number removed.
    pub fn prune_requests(&self, before_ms: i64) -> Result<usize, StorageError> {
        self.with_conn(|c| {
            Ok(c.execute(
                "DELETE FROM request_events WHERE ts < ?1",
                params![before_ms],
            )?)
        })
    }
}
