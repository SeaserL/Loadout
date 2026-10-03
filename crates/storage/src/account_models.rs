// SPDX-License-Identifier: Apache-2.0

use rusqlite::{Row, params};
use uuid::Uuid;

use crate::{Database, StorageError, now_ms};

#[derive(Clone, Debug, PartialEq)]
pub struct AccountModel {
    pub id: String,
    pub account_id: String,
    pub upstream_model_id: String,
    pub model_id: Option<String>,
    /// JSON array of protocol names (`["chat"]`).
    pub protocols_json: String,
    pub context_window: Option<i64>,
    pub max_output_tokens: Option<i64>,
    pub capabilities_json: String,
    pub input_price: Option<f64>,
    pub output_price: Option<f64>,
    pub cache_read_price: Option<f64>,
    pub cache_write_price: Option<f64>,
    pub enabled: bool,
    /// `catalog` (synced from upstream) or `manual`.
    pub source: String,
    /// Latency tier pinned by the user (ExecPlan 004).
    pub tier_override: Option<String>,
    /// `user` (never overwritten by a catalog sync), `catalog`, or `None` (no prices).
    pub price_source: Option<String>,
    pub price_updated_at: Option<i64>,
}

/// Model data from a catalog sync or manual entry.
#[derive(Clone, Debug, Default)]
pub struct ModelInput {
    pub upstream_model_id: String,
    pub protocols_json: String,
    pub context_window: Option<i64>,
    pub max_output_tokens: Option<i64>,
    pub capabilities_json: String,
    pub input_price: Option<f64>,
    pub output_price: Option<f64>,
    pub cache_read_price: Option<f64>,
    pub cache_write_price: Option<f64>,
}

/// Result of a catalog sync.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SyncCounts {
    pub added: usize,
    pub updated: usize,
    /// Catalog models no longer offered upstream (disabled, not deleted).
    pub disabled: usize,
}

#[derive(Debug)]
pub struct AccountModelRepo<'a> {
    db: &'a Database,
}

pub(crate) const COLUMNS: &str = "id, account_id, upstream_model_id, model_id, protocols_json, \
     context_window, max_output_tokens, capabilities_json, input_price, output_price, \
     cache_read_price, cache_write_price, enabled, source, tier_override, price_source, \
     price_updated_at";

pub(crate) fn map_account_model(row: &Row<'_>, offset: usize) -> rusqlite::Result<AccountModel> {
    Ok(AccountModel {
        id: row.get(offset)?,
        account_id: row.get(offset + 1)?,
        upstream_model_id: row.get(offset + 2)?,
        model_id: row.get(offset + 3)?,
        protocols_json: row.get(offset + 4)?,
        context_window: row.get(offset + 5)?,
        max_output_tokens: row.get(offset + 6)?,
        capabilities_json: row.get(offset + 7)?,
        input_price: row.get(offset + 8)?,
        output_price: row.get(offset + 9)?,
        cache_read_price: row.get(offset + 10)?,
        cache_write_price: row.get(offset + 11)?,
        enabled: row.get::<_, i64>(offset + 12)? == 1,
        source: row.get(offset + 13)?,
        tier_override: row.get(offset + 14)?,
        price_source: row.get(offset + 15)?,
        price_updated_at: row.get(offset + 16)?,
    })
}

impl ModelInput {
    fn has_price(&self) -> bool {
        self.input_price.is_some()
            || self.output_price.is_some()
            || self.cache_read_price.is_some()
            || self.cache_write_price.is_some()
    }
}

impl<'a> AccountModelRepo<'a> {
    pub(crate) fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// Applies a full catalog listing for an account:
    /// - new models are inserted (enabled);
    /// - existing catalog models get fresh metadata; the user's `enabled` flag is kept;
    /// - prices reported by the catalog replace stored prices unless the user set them
    ///   (`price_source = 'user'`); a listing without prices leaves stored prices alone;
    /// - catalog models missing from the listing are disabled;
    /// - manually added models are never touched.
    pub fn sync_catalog(
        &self,
        account_id: &str,
        models: &[ModelInput],
    ) -> Result<SyncCounts, StorageError> {
        self.db.with_tx(|tx| {
            let now = now_ms();
            let mut counts = SyncCounts::default();
            for m in models {
                // ?12: the listing carries prices, which replace stored non-user prices.
                let updated = tx.execute(
                    "UPDATE account_models SET
                        protocols_json = ?3, context_window = ?4, max_output_tokens = ?5,
                        capabilities_json = ?6,
                        input_price = CASE WHEN ?12 AND price_source IS NOT 'user' THEN ?7 ELSE input_price END,
                        output_price = CASE WHEN ?12 AND price_source IS NOT 'user' THEN ?8 ELSE output_price END,
                        cache_read_price = CASE WHEN ?12 AND price_source IS NOT 'user' THEN ?9 ELSE cache_read_price END,
                        cache_write_price = CASE WHEN ?12 AND price_source IS NOT 'user' THEN ?10 ELSE cache_write_price END,
                        price_updated_at = CASE WHEN ?12 AND price_source IS NOT 'user' THEN ?11 ELSE price_updated_at END,
                        price_source = CASE WHEN ?12 AND price_source IS NOT 'user' THEN 'catalog' ELSE price_source END,
                        updated_at = ?11
                     WHERE account_id = ?1 AND upstream_model_id = ?2 AND source = 'catalog'",
                    params![
                        account_id,
                        m.upstream_model_id,
                        m.protocols_json,
                        m.context_window,
                        m.max_output_tokens,
                        m.capabilities_json,
                        m.input_price,
                        m.output_price,
                        m.cache_read_price,
                        m.cache_write_price,
                        now,
                        m.has_price()
                    ],
                )?;
                if updated > 0 {
                    counts.updated += 1;
                    continue;
                }
                let inserted = tx.execute(
                    "INSERT OR IGNORE INTO account_models (id, account_id, upstream_model_id,
                        protocols_json, context_window, max_output_tokens, capabilities_json,
                        input_price, output_price, cache_read_price, cache_write_price,
                        enabled, source, created_at, updated_at, price_source, price_updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1, 'catalog', ?12, ?12,
                        ?13, CASE WHEN ?13 IS NULL THEN NULL ELSE ?12 END)",
                    params![
                        Uuid::new_v4().to_string(),
                        account_id,
                        m.upstream_model_id,
                        m.protocols_json,
                        m.context_window,
                        m.max_output_tokens,
                        m.capabilities_json,
                        m.input_price,
                        m.output_price,
                        m.cache_read_price,
                        m.cache_write_price,
                        now,
                        m.has_price().then_some("catalog")
                    ],
                )?;
                counts.added += inserted;
            }
            let ids: Vec<&str> = models
                .iter()
                .map(|m| m.upstream_model_id.as_str())
                .collect();
            let listed =
                serde_json::to_string(&ids).map_err(|e| StorageError::Corrupt(e.to_string()))?;
            counts.disabled = tx.execute(
                "UPDATE account_models SET enabled = 0, updated_at = ?3
                 WHERE account_id = ?1 AND source = 'catalog' AND enabled = 1
                   AND upstream_model_id NOT IN (SELECT value FROM json_each(?2))",
                params![account_id, listed, now],
            )?;
            Ok(counts)
        })
    }

    pub fn add_manual(
        &self,
        account_id: &str,
        m: &ModelInput,
    ) -> Result<AccountModel, StorageError> {
        self.insert(account_id, m, true, "manual")
    }

    /// Inserts a model with an explicit state and origin (configuration import).
    pub fn insert(
        &self,
        account_id: &str,
        m: &ModelInput,
        enabled: bool,
        source: &str,
    ) -> Result<AccountModel, StorageError> {
        let id = Uuid::new_v4().to_string();
        let now = now_ms();
        // Prices on a manual model are the user's; catalog rows keep catalog semantics.
        let price_source = m.has_price().then_some(if source == "manual" {
            "user"
        } else {
            "catalog"
        });
        self.db.with_conn(|c| {
            c.execute(
                "INSERT INTO account_models (id, account_id, upstream_model_id, protocols_json,
                    context_window, max_output_tokens, capabilities_json, input_price, output_price,
                    cache_read_price, cache_write_price, enabled, source, created_at, updated_at,
                    price_source, price_updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?13, ?14, ?12, ?12,
                    ?15, CASE WHEN ?15 IS NULL THEN NULL ELSE ?12 END)",
                params![
                    id,
                    account_id,
                    m.upstream_model_id,
                    m.protocols_json,
                    m.context_window,
                    m.max_output_tokens,
                    m.capabilities_json,
                    m.input_price,
                    m.output_price,
                    m.cache_read_price,
                    m.cache_write_price,
                    now,
                    i64::from(enabled),
                    source,
                    price_source
                ],
            )
            .map_err(|e| {
                StorageError::from_insert(e, format!("model '{}'", m.upstream_model_id))
            })?;
            Ok(())
        })?;
        self.get(&id)?
            .ok_or_else(|| StorageError::NotFound("model just inserted".into()))
    }

    pub fn get(&self, id: &str) -> Result<Option<AccountModel>, StorageError> {
        use rusqlite::OptionalExtension;
        self.db.with_conn(|c| {
            Ok(c.query_row(
                &format!("SELECT {COLUMNS} FROM account_models WHERE id = ?1"),
                params![id],
                |r| map_account_model(r, 0),
            )
            .optional()?)
        })
    }

    pub fn list(&self, account_id: &str) -> Result<Vec<AccountModel>, StorageError> {
        self.db.with_conn(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM account_models WHERE account_id = ?1 ORDER BY upstream_model_id"
            ))?;
            let rows = stmt.query_map(params![account_id], |r| map_account_model(r, 0))?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    pub fn find(
        &self,
        account_id: &str,
        upstream_model_id: &str,
    ) -> Result<Option<AccountModel>, StorageError> {
        use rusqlite::OptionalExtension;
        self.db.with_conn(|c| {
            Ok(c.query_row(
                &format!(
                    "SELECT {COLUMNS} FROM account_models WHERE account_id = ?1 AND upstream_model_id = ?2"
                ),
                params![account_id, upstream_model_id],
                |r| map_account_model(r, 0),
            )
            .optional()?)
        })
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<bool, StorageError> {
        self.db.with_conn(|c| {
            Ok(c.execute(
                "UPDATE account_models SET enabled = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, i64::from(enabled), now_ms()],
            )? > 0)
        })
    }

    /// Sets prices chosen by the user; they are never overwritten by a catalog sync.
    /// Clearing every price hands the model back to the catalog.
    #[allow(clippy::too_many_arguments)]
    pub fn set_prices(
        &self,
        id: &str,
        input: Option<f64>,
        output: Option<f64>,
        cache_read: Option<f64>,
        cache_write: Option<f64>,
    ) -> Result<bool, StorageError> {
        self.db.with_conn(|c| {
            let any = [input, output, cache_read, cache_write]
                .iter()
                .any(Option::is_some);
            Ok(c.execute(
                "UPDATE account_models SET input_price = ?2, output_price = ?3,
                    cache_read_price = ?4, cache_write_price = ?5, updated_at = ?6,
                    price_source = ?7, price_updated_at = ?6 WHERE id = ?1",
                params![
                    id,
                    input,
                    output,
                    cache_read,
                    cache_write,
                    now_ms(),
                    any.then_some("user")
                ],
            )? > 0)
        })
    }

    pub fn delete(&self, id: &str) -> Result<bool, StorageError> {
        self.db.with_conn(|c| {
            Ok(c.execute("DELETE FROM account_models WHERE id = ?1", params![id])? > 0)
        })
    }
}
