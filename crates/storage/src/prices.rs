// SPDX-License-Identifier: Apache-2.0

//! Inputs for cross-account price comparison (ExecPlan 011). Control plane only; never
//! read on the request hot path.

use rusqlite::params;

use crate::account_models::{AccountModel, COLUMNS, map_account_model};
use crate::{Database, StorageError};

/// One account model with the names needed to display it.
#[derive(Clone, Debug, PartialEq)]
pub struct PricedModel {
    pub provider_slug: String,
    pub account_alias: String,
    pub account_enabled: bool,
    pub model: AccountModel,
}

/// Token totals of successful requests for one upstream model id.
/// `input_tokens` includes cached tokens (canonical usage semantics).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelTokenTotals {
    pub upstream_model: String,
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
}

impl Database {
    /// Every account model, ordered by provider slug, account position and model id.
    pub fn priced_models(&self) -> Result<Vec<PricedModel>, StorageError> {
        let columns = COLUMNS
            .split(", ")
            .map(|col| format!("m.{}", col.trim()))
            .collect::<Vec<_>>()
            .join(", ");
        self.with_conn(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT p.slug, a.alias, a.enabled, {columns}
                 FROM account_models m
                 JOIN accounts a ON a.id = m.account_id
                 JOIN providers p ON p.id = a.provider_id
                 ORDER BY p.slug, a.position, a.alias, m.upstream_model_id"
            ))?;
            let rows = stmt.query_map(params![], |r| {
                Ok(PricedModel {
                    provider_slug: r.get(0)?,
                    account_alias: r.get(1)?,
                    account_enabled: r.get::<_, i64>(2)? == 1,
                    model: map_account_model(r, 3)?,
                })
            })?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    /// Token totals per upstream model of successful requests since `since_ms`.
    pub fn token_totals_by_model(
        &self,
        since_ms: i64,
    ) -> Result<Vec<ModelTokenTotals>, StorageError> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT upstream_model, COUNT(*),
                        COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
                        COALESCE(SUM(cache_read_tokens), 0), COALESCE(SUM(cache_write_tokens), 0)
                 FROM request_events
                 WHERE ts >= ?1 AND status = 'ok' AND upstream_model IS NOT NULL
                 GROUP BY upstream_model
                 ORDER BY upstream_model",
            )?;
            let rows = stmt.query_map(params![since_ms], |r| {
                Ok(ModelTokenTotals {
                    upstream_model: r.get(0)?,
                    requests: r.get(1)?,
                    input_tokens: r.get(2)?,
                    output_tokens: r.get(3)?,
                    cache_read_tokens: r.get(4)?,
                    cache_write_tokens: r.get(5)?,
                })
            })?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{Database, RequestEvent};

    fn event(ts: i64, model: &str, status: &str, input: i64, cache_read: i64) -> RequestEvent {
        RequestEvent {
            ts,
            requested_model: model.into(),
            upstream_model: Some(model.into()),
            protocol_in: "chat".into(),
            status: status.into(),
            attempts: 1,
            input_tokens: Some(input),
            output_tokens: Some(10),
            cache_read_tokens: Some(cache_read),
            duration_ms: 1,
            ..RequestEvent::default()
        }
    }

    #[test]
    fn token_totals_count_recent_successes_only() {
        let db = Database::open_in_memory().expect("db");
        crate::request_events::insert_batch(
            &mut db.lock(),
            &[
                event(100, "m1", "ok", 1000, 400),
                event(200, "m1", "ok", 500, 0),
                event(200, "m1", "error", 9999, 0),
                event(5, "m1", "ok", 9999, 0),
                event(300, "m2", "ok", 1, 0),
            ],
        )
        .expect("insert");
        let totals = db.token_totals_by_model(50).expect("totals");
        assert_eq!(totals.len(), 2);
        assert_eq!(totals[0].upstream_model, "m1");
        assert_eq!(
            (
                totals[0].requests,
                totals[0].input_tokens,
                totals[0].output_tokens,
                totals[0].cache_read_tokens,
                totals[0].cache_write_tokens
            ),
            (2, 1500, 20, 400, 0)
        );
    }
}
