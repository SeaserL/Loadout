// SPDX-License-Identifier: Apache-2.0

//! Latest usage per account (quota windows, balances, plan) and spend aggregates from
//! request metadata.

use rusqlite::{OptionalExtension, params};

use crate::{Database, StorageError};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowRow {
    pub scope: String,
    pub label: String,
    pub unit: String,
    pub used: Option<f64>,
    pub limit: Option<f64>,
    pub window_seconds: Option<i64>,
    pub resets_at: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BalanceRow {
    pub currency: String,
    pub amount: f64,
    pub included: Option<f64>,
    pub topup: Option<f64>,
    pub granted: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageRecord {
    pub account_id: String,
    pub fetched_at: i64,
    pub source: Option<String>,
    pub confidence: Option<String>,
    pub plan_json: Option<String>,
    pub account_label: Option<String>,
    /// Last refresh error; windows/balances then hold the last good values.
    pub error: Option<String>,
    pub windows: Vec<WindowRow>,
    pub balances: Vec<BalanceRow>,
}

/// Spend aggregated from `request_events`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spend {
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_usd: f64,
}

impl Database {
    /// Replaces the account's usage with a successful refresh.
    pub fn save_usage(&self, r: &UsageRecord) -> Result<(), StorageError> {
        self.with_tx(|tx| {
            tx.execute(
                "INSERT INTO usage_snapshots (account_id, fetched_at, source, confidence, plan_json,
                                              account_label, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)
                 ON CONFLICT(account_id) DO UPDATE SET fetched_at = excluded.fetched_at,
                    source = excluded.source, confidence = excluded.confidence,
                    plan_json = excluded.plan_json, account_label = excluded.account_label,
                    error = NULL",
                params![r.account_id, r.fetched_at, r.source, r.confidence, r.plan_json, r.account_label],
            )?;
            tx.execute("DELETE FROM quota_windows WHERE account_id = ?1", params![r.account_id])?;
            tx.execute("DELETE FROM balances WHERE account_id = ?1", params![r.account_id])?;
            for w in &r.windows {
                tx.execute(
                    "INSERT OR REPLACE INTO quota_windows (account_id, scope, label, unit, used,
                        limit_value, window_seconds, resets_at, observed_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        r.account_id, w.scope, w.label, w.unit, w.used, w.limit, w.window_seconds,
                        w.resets_at, r.fetched_at
                    ],
                )?;
            }
            for b in &r.balances {
                tx.execute(
                    "INSERT OR REPLACE INTO balances (account_id, currency, amount, included, topup,
                        granted, observed_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![r.account_id, b.currency, b.amount, b.included, b.topup, b.granted, r.fetched_at],
                )?;
            }
            Ok(())
        })
    }

    /// Records a failed refresh, keeping the last good windows and balances.
    pub fn save_usage_error(
        &self,
        account_id: &str,
        at: i64,
        error: &str,
    ) -> Result<(), StorageError> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO usage_snapshots (account_id, fetched_at, error) VALUES (?1, ?2, ?3)
                 ON CONFLICT(account_id) DO UPDATE SET fetched_at = excluded.fetched_at,
                    error = excluded.error",
                params![account_id, at, error],
            )?;
            Ok(())
        })
    }

    pub fn usage(&self, account_id: &str) -> Result<Option<UsageRecord>, StorageError> {
        self.with_conn(|c| {
            let Some(mut r) = c
                .query_row(
                    "SELECT account_id, fetched_at, source, confidence, plan_json, account_label, error
                     FROM usage_snapshots WHERE account_id = ?1",
                    params![account_id],
                    |row| {
                        Ok(UsageRecord {
                            account_id: row.get(0)?,
                            fetched_at: row.get(1)?,
                            source: row.get(2)?,
                            confidence: row.get(3)?,
                            plan_json: row.get(4)?,
                            account_label: row.get(5)?,
                            error: row.get(6)?,
                            windows: Vec::new(),
                            balances: Vec::new(),
                        })
                    },
                )
                .optional()?
            else {
                return Ok(None);
            };
            let mut stmt = c.prepare(
                "SELECT scope, label, unit, used, limit_value, window_seconds, resets_at
                 FROM quota_windows WHERE account_id = ?1 ORDER BY COALESCE(window_seconds, 0), scope",
            )?;
            r.windows = stmt
                .query_map(params![account_id], |row| {
                    Ok(WindowRow {
                        scope: row.get(0)?,
                        label: row.get(1)?,
                        unit: row.get(2)?,
                        used: row.get(3)?,
                        limit: row.get(4)?,
                        window_seconds: row.get(5)?,
                        resets_at: row.get(6)?,
                    })
                })?
                .collect::<Result<_, _>>()?;
            let mut stmt = c.prepare(
                "SELECT currency, amount, included, topup, granted FROM balances
                 WHERE account_id = ?1 ORDER BY currency",
            )?;
            r.balances = stmt
                .query_map(params![account_id], |row| {
                    Ok(BalanceRow {
                        currency: row.get(0)?,
                        amount: row.get(1)?,
                        included: row.get(2)?,
                        topup: row.get(3)?,
                        granted: row.get(4)?,
                    })
                })?
                .collect::<Result<_, _>>()?;
            Ok(Some(r))
        })
    }

    /// Spend of one account since `since_ms`, from request metadata.
    pub fn account_spend(
        &self,
        provider: &str,
        account: &str,
        since_ms: i64,
    ) -> Result<Spend, StorageError> {
        self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*), COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
                        COALESCE(SUM(estimated_cost_usd), 0.0)
                 FROM request_events WHERE provider = ?1 AND account = ?2 AND ts >= ?3",
                params![provider, account, since_ms],
                |r| {
                    Ok(Spend {
                        requests: r.get(0)?,
                        input_tokens: r.get(1)?,
                        output_tokens: r.get(2)?,
                        cost_usd: r.get(3)?,
                    })
                },
            )?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NewAccount, NewProvider};

    fn account(db: &Database) -> String {
        let p = db
            .providers()
            .create(&NewProvider {
                slug: "p",
                kind: "k",
                display_name: "P",
                base_url: None,
                config_json: "{}",
            })
            .expect("provider");
        db.accounts()
            .create(&NewAccount {
                provider_id: &p.id,
                alias: "a",
                display_name: None,
                account_type: "api_key",
            })
            .expect("account")
            .id
    }

    #[test]
    fn save_replace_and_error_keeps_last_values() {
        let db = Database::open_in_memory().expect("db");
        let id = account(&db);
        assert!(db.usage(&id).expect("usage").is_none());

        let record = UsageRecord {
            account_id: id.clone(),
            fetched_at: 10,
            source: Some("OFFICIAL_API".into()),
            confidence: Some("EXACT".into()),
            windows: vec![
                WindowRow {
                    scope: "weekly".into(),
                    label: "Weekly".into(),
                    unit: "percent".into(),
                    used: Some(40.0),
                    window_seconds: Some(604_800),
                    ..WindowRow::default()
                },
                WindowRow {
                    scope: "5h".into(),
                    label: "5h".into(),
                    unit: "percent".into(),
                    used: Some(78.0),
                    window_seconds: Some(18_000),
                    resets_at: Some(99),
                    ..WindowRow::default()
                },
            ],
            balances: vec![BalanceRow {
                currency: "USD".into(),
                amount: 38.27,
                granted: Some(70.0),
                ..BalanceRow::default()
            }],
            ..UsageRecord::default()
        };
        db.save_usage(&record).expect("save");
        let got = db.usage(&id).expect("usage").expect("some");
        assert_eq!(got.windows[0].scope, "5h", "ordered by window length");
        assert_eq!(got.balances[0].amount, 38.27);

        db.save_usage_error(&id, 20, "HTTP 500").expect("error");
        let got = db.usage(&id).expect("usage").expect("some");
        assert_eq!(got.error.as_deref(), Some("HTTP 500"));
        assert_eq!(got.fetched_at, 20);
        assert_eq!(got.windows.len(), 2, "last good values kept");

        let mut fewer = record.clone();
        fewer.windows.truncate(1);
        fewer.balances.clear();
        db.save_usage(&fewer).expect("save");
        let got = db.usage(&id).expect("usage").expect("some");
        assert_eq!(got.windows.len(), 1);
        assert!(got.balances.is_empty());
        assert_eq!(got.error, None);
    }

    #[test]
    fn spend_aggregates_request_events() {
        let db = Database::open_in_memory().expect("db");
        assert_eq!(
            db.account_spend("p", "a", 0).expect("spend"),
            Spend::default()
        );
    }
}
