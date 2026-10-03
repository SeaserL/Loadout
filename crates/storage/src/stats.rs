// SPDX-License-Identifier: Apache-2.0

//! Filtered request history and aggregate statistics (ExecPlan 006 §3.2–3.3).

use rusqlite::params_from_iter;
use rusqlite::types::Value as SqlValue;

use crate::request_events::{COLUMNS, map_event};
use crate::{Database, RequestEvent, StorageError};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RequestFilter {
    pub limit: usize,
    pub client_id: Option<String>,
    /// Substring of the requested model or the route (`provider/account/model`).
    pub search: Option<String>,
    /// `ok` or `error` (anything but ok).
    pub status: Option<String>,
    pub since_ms: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatsGroup {
    Model,
    Route,
    Provider,
    Client,
    /// UTC calendar day.
    Day,
}

impl StatsGroup {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "model" => Self::Model,
            "route" => Self::Route,
            "provider" => Self::Provider,
            "client" => Self::Client,
            "day" => Self::Day,
            _ => return None,
        })
    }

    fn key_sql(self) -> &'static str {
        match self {
            Self::Model => "requested_model",
            Self::Route => {
                "COALESCE(provider || '/' || account || '/' || upstream_model, '(no route)')"
            }
            Self::Provider => "COALESCE(provider, '(no route)')",
            Self::Client => "COALESCE(client_id, '(none)')",
            Self::Day => "date(ts / 1000, 'unixepoch')",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StatsRow {
    pub key: String,
    pub requests: i64,
    pub errors: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_usd: f64,
    pub avg_ttft_ms: Option<f64>,
    pub avg_tps: Option<f64>,
    /// Cached input tokens read / written (part of `input_tokens`).
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    /// Net USD saved by cache reads (minus any cache-write premium), using the account
    /// model's current prices; only counts requests whose model has a cache price set.
    pub cache_savings_usd: f64,
}

/// Per-request cache savings in USD: cache reads billed below the input price, minus the
/// premium paid for cache writes. `NULL` when the model has no input price.
const CACHE_SAVINGS_SQL: &str = "(COALESCE(cache_read_tokens, 0) * (am_in - COALESCE(am_cr, am_in)) \
     - COALESCE(cache_write_tokens, 0) * (COALESCE(am_cw, am_in) - am_in)) / 1000000.0";

fn like_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('%');
    for c in s.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

impl Database {
    /// Most recent matching events first.
    pub fn find_requests(&self, f: &RequestFilter) -> Result<Vec<RequestEvent>, StorageError> {
        let mut clauses: Vec<&str> = Vec::new();
        let mut args: Vec<SqlValue> = Vec::new();
        if let Some(id) = &f.client_id {
            clauses.push("client_id = ?");
            args.push(SqlValue::Text(id.clone()));
        }
        if let Some(search) = f.search.as_deref().filter(|s| !s.is_empty()) {
            clauses.push(
                "(requested_model LIKE ? ESCAPE '\\' OR \
                  (provider || '/' || account || '/' || upstream_model) LIKE ? ESCAPE '\\')",
            );
            let pattern = like_escape(search);
            args.push(SqlValue::Text(pattern.clone()));
            args.push(SqlValue::Text(pattern));
        }
        match f.status.as_deref() {
            Some("ok") => clauses.push("status = 'ok'"),
            Some("error") => clauses.push("status <> 'ok'"),
            _ => {}
        }
        if let Some(since) = f.since_ms {
            clauses.push("ts >= ?");
            args.push(SqlValue::Integer(since));
        }
        let filter = if clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", clauses.join(" AND "))
        };
        args.push(SqlValue::Integer(f.limit.clamp(1, 10_000) as i64));
        self.with_conn(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM request_events {filter} ORDER BY ts DESC, id DESC LIMIT ?"
            ))?;
            let rows = stmt.query_map(params_from_iter(args), map_event)?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    /// Aggregates since `since_ms`, grouped by `group`. Days are in date order, other
    /// groups by request count (descending).
    pub fn request_stats(
        &self,
        since_ms: i64,
        group: StatsGroup,
    ) -> Result<Vec<StatsRow>, StorageError> {
        let key = group.key_sql();
        let order = if group == StatsGroup::Day {
            "key"
        } else {
            "requests DESC, key"
        };
        self.with_conn(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {key} AS key,
                        COUNT(*) AS requests,
                        SUM(status <> 'ok'),
                        COALESCE(SUM(input_tokens), 0),
                        COALESCE(SUM(output_tokens), 0),
                        COALESCE(SUM(estimated_cost_usd), 0.0),
                        AVG(ttft_ms),
                        AVG(output_tps),
                        COALESCE(SUM(cache_read_tokens), 0),
                        COALESCE(SUM(cache_write_tokens), 0),
                        COALESCE(SUM({CACHE_SAVINGS_SQL}), 0.0)
                   FROM request_events
                   LEFT JOIN (SELECT id AS am_id, input_price AS am_in,
                                     cache_read_price AS am_cr, cache_write_price AS am_cw
                                FROM account_models)
                     -- route_target is the target id: account model id, ':', protocol
                     ON am_id = substr(route_target, 1,
                                       length(route_target) - length(protocol_out) - 1)
                  WHERE ts >= ?1
                  GROUP BY key
                  ORDER BY {order}"
            ))?;
            let rows = stmt.query_map([since_ms], |r| {
                Ok(StatsRow {
                    key: r.get(0)?,
                    requests: r.get(1)?,
                    errors: r.get(2)?,
                    input_tokens: r.get(3)?,
                    output_tokens: r.get(4)?,
                    cost_usd: r.get(5)?,
                    avg_ttft_ms: r.get(6)?,
                    avg_tps: r.get(7)?,
                    cache_read_tokens: r.get(8)?,
                    cache_write_tokens: r.get(9)?,
                    cache_savings_usd: r.get(10)?,
                })
            })?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(ts: i64, client: &str, model: &str, status: &str, cost: f64) -> RequestEvent {
        RequestEvent {
            ts,
            client_id: Some(client.into()),
            requested_model: model.into(),
            provider: Some("p".into()),
            account: Some("main".into()),
            upstream_model: Some(model.rsplit('/').next().unwrap_or(model).into()),
            protocol_in: "chat".into(),
            status: status.into(),
            attempts: 1,
            input_tokens: Some(10),
            output_tokens: Some(5),
            ttft_ms: (status == "ok").then_some(200),
            duration_ms: 1000,
            output_tps: (status == "ok").then_some(50.0),
            estimated_cost_usd: Some(cost),
            ..RequestEvent::default()
        }
    }

    fn db() -> Database {
        let db = Database::open_in_memory().expect("db");
        let day = 86_400_000;
        crate::request_events::insert_batch(
            &mut db.lock(),
            &[
                event(day, "a", "loadout/fast", "ok", 0.01),
                event(day + 1, "a", "p/main/m1", "error", 0.0),
                event(2 * day, "b", "loadout/fast", "ok", 0.02),
                event(2 * day + 1, "b", "100%_odd", "ok", 0.0),
            ],
        )
        .expect("insert");
        db
    }

    #[test]
    fn filters_combine() {
        let db = db();
        let all = RequestFilter {
            limit: 100,
            ..RequestFilter::default()
        };
        assert_eq!(db.find_requests(&all).expect("all").len(), 4);
        let a = RequestFilter {
            client_id: Some("a".into()),
            ..all.clone()
        };
        assert_eq!(db.find_requests(&a).expect("a").len(), 2);
        let errors = RequestFilter {
            status: Some("error".into()),
            ..all.clone()
        };
        assert_eq!(
            db.find_requests(&errors).expect("e")[0].requested_model,
            "p/main/m1"
        );
        let search = RequestFilter {
            search: Some("fast".into()),
            since_ms: Some(2 * 86_400_000),
            ..all.clone()
        };
        assert_eq!(db.find_requests(&search).expect("s").len(), 1);
        let literal = RequestFilter {
            search: Some("0%_".into()),
            ..all.clone()
        };
        assert_eq!(
            db.find_requests(&literal).expect("l").len(),
            1,
            "LIKE wildcards escaped"
        );
        let route = RequestFilter {
            search: Some("p/main/m1".into()),
            ..all
        };
        assert_eq!(db.find_requests(&route).expect("r").len(), 1);
    }

    #[test]
    fn aggregates_by_group() {
        let db = db();
        let by_model = db.request_stats(0, StatsGroup::Model).expect("model");
        assert_eq!(by_model[0].key, "loadout/fast");
        assert_eq!(by_model[0].requests, 2);
        assert!((by_model[0].cost_usd - 0.03).abs() < 1e-9);
        assert_eq!(by_model[0].avg_ttft_ms, Some(200.0));

        let by_day = db.request_stats(0, StatsGroup::Day).expect("day");
        assert_eq!(
            by_day.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(),
            vec!["1970-01-02", "1970-01-03"]
        );
        assert_eq!(by_day[0].errors, 1);

        let by_client = db
            .request_stats(2 * 86_400_000, StatsGroup::Client)
            .expect("client");
        assert_eq!(by_client.len(), 1);
        assert_eq!(by_client[0].input_tokens, 20);
        assert!(StatsGroup::parse("route").is_some() && StatsGroup::parse("x").is_none());
    }

    #[test]
    fn cache_tokens_and_savings_use_model_prices() {
        let db = Database::open_in_memory().expect("db");
        db.lock()
            .execute_batch(
                "INSERT INTO providers (id, slug, kind, display_name, created_at, updated_at)
                     VALUES ('prov', 'p', 'openai-compatible', 'P', 0, 0);
                 INSERT INTO accounts (id, provider_id, alias, account_type, created_at, updated_at)
                     VALUES ('acc', 'prov', 'main', 'api_key', 0, 0);
                 INSERT INTO account_models (id, account_id, upstream_model_id, input_price,
                         output_price, cache_read_price, cache_write_price, source, created_at,
                         updated_at)
                     VALUES ('am1', 'acc', 'm1', 3.0, 15.0, 0.3, 3.75, 'manual', 0, 0);",
            )
            .expect("seed");
        let cached = RequestEvent {
            route_target: Some("am1:messages".into()),
            protocol_out: Some("messages".into()),
            input_tokens: Some(1_000_000),
            cache_read_tokens: Some(800_000),
            cache_write_tokens: Some(100_000),
            decision: Some("p/main/m1: ok".into()),
            ..event(1, "a", "p/main/m1", "ok", 0.0)
        };
        // No matching account model: tokens count, savings do not.
        let unpriced = RequestEvent {
            route_target: Some("gone:chat".into()),
            protocol_out: Some("chat".into()),
            cache_read_tokens: Some(5),
            ..event(2, "a", "p/main/m1", "ok", 0.0)
        };
        crate::request_events::insert_batch(&mut db.lock(), &[cached, unpriced]).expect("insert");

        let rows = db.request_stats(0, StatsGroup::Model).expect("stats");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].cache_read_tokens, 800_005);
        assert_eq!(rows[0].cache_write_tokens, 100_000);
        // 0.8M × (3.0 − 0.3) − 0.1M × (3.75 − 3.0), per million.
        assert!((rows[0].cache_savings_usd - (2.16 - 0.075)).abs() < 1e-9);

        let logged = db
            .find_requests(&RequestFilter {
                limit: 10,
                ..RequestFilter::default()
            })
            .expect("find");
        assert_eq!(logged[1].decision.as_deref(), Some("p/main/m1: ok"));
        assert_eq!(logged[0].decision, None);
    }
}
