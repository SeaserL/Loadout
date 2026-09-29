// SPDX-License-Identifier: Apache-2.0

//! Latency history, tier overrides and user pools (ExecPlan 004).

use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

use crate::{Database, StorageError, now_ms};

/// One synthetic probe result.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProbeSample {
    pub ts: i64,
    pub route_target: String,
    /// `ok` or `error`.
    pub status: String,
    pub error: Option<String>,
    pub ttft_ms: Option<i64>,
    pub duration_ms: i64,
    pub output_tokens: Option<i64>,
    pub output_tps: Option<f64>,
}

/// A successful sample from either source, for rebuilding in-memory statistics.
#[derive(Clone, Debug, PartialEq)]
pub struct HistorySample {
    pub ts: i64,
    pub route_target: String,
    pub probe: bool,
    pub ttft_ms: i64,
    pub output_tokens: Option<i64>,
    pub output_tps: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pool {
    pub id: String,
    pub name: String,
    /// `manual` or `latency`.
    pub ordering: String,
    /// Account model ids in the user's order.
    pub members: Vec<String>,
}

impl Database {
    pub fn insert_probe_sample(&self, s: &ProbeSample) -> Result<(), StorageError> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO latency_samples
                   (ts, route_target, status, error, ttft_ms, duration_ms, output_tokens, output_tps)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    s.ts,
                    s.route_target,
                    s.status,
                    s.error,
                    s.ttft_ms,
                    s.duration_ms,
                    s.output_tokens,
                    s.output_tps
                ],
            )?;
            Ok(())
        })
    }

    /// Most recent successful samples per account model (target ids are
    /// `"{account_model_id}:{protocol}"`), at most `per_model` each, since `since_ms`.
    /// Oldest first.
    pub fn latency_history(
        &self,
        since_ms: i64,
        per_model: usize,
    ) -> Result<Vec<HistorySample>, StorageError> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT ts, route_target, probe, ttft_ms, output_tokens, output_tps FROM (
                   SELECT *, ROW_NUMBER() OVER (
                       PARTITION BY substr(route_target, 1, instr(route_target || ':', ':') - 1)
                       ORDER BY ts DESC) AS rn
                   FROM (
                     SELECT ts, route_target, 0 AS probe, ttft_ms, output_tokens, output_tps
                       FROM request_events
                      WHERE ts >= ?1 AND status = 'ok' AND route_target IS NOT NULL
                        AND ttft_ms IS NOT NULL
                     UNION ALL
                     SELECT ts, route_target, 1, ttft_ms, output_tokens, output_tps
                       FROM latency_samples
                      WHERE ts >= ?1 AND status = 'ok' AND ttft_ms IS NOT NULL))
                 WHERE rn <= ?2
                 ORDER BY ts",
            )?;
            let rows = stmt.query_map(params![since_ms, per_model as i64], |r| {
                Ok(HistorySample {
                    ts: r.get(0)?,
                    route_target: r.get(1)?,
                    probe: r.get::<_, i64>(2)? == 1,
                    ttft_ms: r.get(3)?,
                    output_tokens: r.get(4)?,
                    output_tps: r.get(5)?,
                })
            })?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    /// Latest probe per target id prefix (account model), for scheduling.
    pub fn last_probe_times(&self) -> Result<Vec<(String, i64)>, StorageError> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT substr(route_target, 1, instr(route_target || ':', ':') - 1) AS m, MAX(ts)
                   FROM latency_samples GROUP BY m",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    pub fn prune_probe_samples(&self, before_ms: i64) -> Result<usize, StorageError> {
        self.with_conn(|c| {
            Ok(c.execute(
                "DELETE FROM latency_samples WHERE ts < ?1",
                params![before_ms],
            )?)
        })
    }

    /// `account_model_id → provider/account/model` for every model.
    pub fn model_names(&self) -> Result<std::collections::HashMap<String, String>, StorageError> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT m.id, p.slug || '/' || a.alias || '/' || m.upstream_model_id
                   FROM account_models m
                   JOIN accounts a ON a.id = m.account_id
                   JOIN providers p ON p.id = a.provider_id",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    // ---- tier overrides -------------------------------------------------------------------

    pub fn set_tier_override(
        &self,
        account_model_id: &str,
        tier: Option<&str>,
    ) -> Result<bool, StorageError> {
        self.with_conn(|c| {
            Ok(c.execute(
                "UPDATE account_models SET tier_override = ?2 WHERE id = ?1",
                params![account_model_id, tier],
            )? == 1)
        })
    }

    /// `(account_model_id, tier)` for every pinned tier.
    pub fn tier_overrides(&self) -> Result<Vec<(String, String)>, StorageError> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT id, tier_override FROM account_models WHERE tier_override IS NOT NULL",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    // ---- pools ----------------------------------------------------------------------------

    pub fn pools(&self) -> Result<Vec<Pool>, StorageError> {
        self.with_conn(|c| {
            let mut stmt = c.prepare("SELECT id, name, ordering FROM pools ORDER BY name")?;
            let mut pools: Vec<Pool> = stmt
                .query_map([], |r| {
                    Ok(Pool {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        ordering: r.get(2)?,
                        members: Vec::new(),
                    })
                })?
                .collect::<Result<_, _>>()?;
            let mut members = c.prepare(
                "SELECT account_model_id FROM pool_members WHERE pool_id = ?1 ORDER BY position",
            )?;
            for p in &mut pools {
                p.members = members
                    .query_map(params![p.id], |r| r.get(0))?
                    .collect::<Result<_, _>>()?;
            }
            Ok(pools)
        })
    }

    pub fn pool(&self, name: &str) -> Result<Option<Pool>, StorageError> {
        Ok(self.pools()?.into_iter().find(|p| p.name == name))
    }

    /// Creates a pool, or replaces its ordering and members when it exists.
    pub fn save_pool(
        &self,
        name: &str,
        ordering: &str,
        members: &[String],
    ) -> Result<Pool, StorageError> {
        let now = now_ms();
        self.with_tx(|tx| {
            let existing: Option<String> = tx
                .query_row("SELECT id FROM pools WHERE name = ?1", params![name], |r| {
                    r.get(0)
                })
                .optional()?;
            let id = match existing {
                Some(id) => {
                    tx.execute(
                        "UPDATE pools SET ordering = ?2, updated_at = ?3 WHERE id = ?1",
                        params![id, ordering, now],
                    )?;
                    tx.execute("DELETE FROM pool_members WHERE pool_id = ?1", params![id])?;
                    id
                }
                None => {
                    let id = Uuid::new_v4().to_string();
                    tx.execute(
                        "INSERT INTO pools (id, name, ordering, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?4)",
                        params![id, name, ordering, now],
                    )?;
                    id
                }
            };
            for (i, m) in members.iter().enumerate() {
                tx.execute(
                    "INSERT OR IGNORE INTO pool_members (pool_id, account_model_id, position)
                     VALUES (?1, ?2, ?3)",
                    params![id, m, i as i64],
                )?;
            }
            Ok(())
        })?;
        self.pool(name)?
            .ok_or_else(|| StorageError::NotFound(format!("pool {name}")))
    }

    pub fn delete_pool(&self, name: &str) -> Result<bool, StorageError> {
        self.with_conn(|c| Ok(c.execute("DELETE FROM pools WHERE name = ?1", params![name])? == 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModelInput, NewAccount, NewProvider, RequestEvent};

    fn setup() -> (Database, Vec<String>) {
        let db = Database::open_in_memory().expect("db");
        let p = db
            .providers()
            .create(&NewProvider {
                slug: "p",
                kind: "openai-compatible",
                display_name: "P",
                base_url: None,
                config_json: "{}",
            })
            .expect("provider");
        let a = db
            .accounts()
            .create(&NewAccount {
                provider_id: &p.id,
                alias: "main",
                display_name: None,
                account_type: "api_key",
            })
            .expect("account");
        let mut ids = Vec::new();
        for m in ["m1", "m2"] {
            let am = db
                .account_models()
                .add_manual(
                    &a.id,
                    &ModelInput {
                        upstream_model_id: m.into(),
                        protocols_json: "[\"chat\"]".into(),
                        capabilities_json: "[]".into(),
                        ..ModelInput::default()
                    },
                )
                .expect("model");
            ids.push(am.id);
        }
        (db, ids)
    }

    #[test]
    fn history_merges_sources_and_limits_per_model() {
        let (db, ids) = setup();
        let target = format!("{}:chat", ids[0]);
        let event = |ts: i64, status: &str, ttft: Option<i64>| RequestEvent {
            ts,
            requested_model: "x".into(),
            route_target: Some(target.clone()),
            protocol_in: "chat".into(),
            status: status.into(),
            attempts: 1,
            ttft_ms: ttft,
            duration_ms: 1000,
            ..RequestEvent::default()
        };
        let mut events: Vec<RequestEvent> = (0..5)
            .map(|i| event(1000 + i, "ok", Some(100 + i)))
            .collect();
        events.push(event(1100, "error", None));
        crate::request_events::insert_batch(&mut db.lock(), &events).expect("insert");
        db.insert_probe_sample(&ProbeSample {
            ts: 2000,
            route_target: format!("{}:messages", ids[0]),
            status: "ok".into(),
            ttft_ms: Some(50),
            duration_ms: 60,
            ..ProbeSample::default()
        })
        .expect("probe");
        db.insert_probe_sample(&ProbeSample {
            ts: 2001,
            route_target: format!("{}:chat", ids[1]),
            status: "error".into(),
            error: Some("401".into()),
            duration_ms: 60,
            ..ProbeSample::default()
        })
        .expect("probe");

        let all = db.latency_history(0, 64).expect("history");
        assert_eq!(all.len(), 6, "errors excluded");
        assert!(all.last().expect("last").probe);
        let limited = db.latency_history(0, 3).expect("history");
        assert_eq!(limited.len(), 3, "per account model, across protocols");
        assert_eq!(
            limited.iter().map(|h| h.ts).collect::<Vec<_>>(),
            vec![1003, 1004, 2000],
            "newest per model, returned oldest first"
        );
        assert_eq!(db.latency_history(1003, 64).expect("h").len(), 3);

        let last = db.last_probe_times().expect("last");
        assert_eq!(last.len(), 2);
        assert_eq!(db.prune_probe_samples(2001).expect("prune"), 1);
    }

    #[test]
    fn pools_and_overrides() {
        let (db, ids) = setup();
        let pool = db
            .save_pool("fastest", "manual", &[ids[1].clone(), ids[0].clone()])
            .expect("create");
        assert_eq!(pool.members, vec![ids[1].clone(), ids[0].clone()]);
        let pool = db
            .save_pool("fastest", "latency", &[ids[0].clone()])
            .expect("replace");
        assert_eq!((pool.ordering.as_str(), pool.members.len()), ("latency", 1));
        db.account_models().delete(&ids[0]).expect("delete model");
        assert!(
            db.pool("fastest")
                .expect("get")
                .expect("pool")
                .members
                .is_empty()
        );
        assert!(db.delete_pool("fastest").expect("delete"));
        assert!(!db.delete_pool("fastest").expect("delete again"));

        assert!(
            db.set_tier_override(&ids[1], Some("ULTRA_FAST"))
                .expect("set")
        );
        assert_eq!(
            db.tier_overrides().expect("list"),
            vec![(ids[1].clone(), "ULTRA_FAST".to_owned())]
        );
        db.set_tier_override(&ids[1], None).expect("clear");
        assert!(db.tier_overrides().expect("list").is_empty());
    }
}
