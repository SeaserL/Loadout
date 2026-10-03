// SPDX-License-Identifier: Apache-2.0

use rusqlite::{OptionalExtension, Row, params};
use uuid::Uuid;

use crate::{Database, StorageError, now_ms};

#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    pub id: String,
    pub provider_id: String,
    pub alias: String,
    pub display_name: Option<String>,
    pub account_type: String,
    pub enabled: bool,
    pub position: i64,
    pub subscription_plan: Option<String>,
    pub price_monthly_usd: Option<f64>,
    pub renew_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    /// When the user confirmed a lower-trust transport for this account (ADR-012).
    pub transport_opt_in_at: Option<i64>,
    /// Per-account quota reserve (JSON, e.g. `{"default":10,"weekly":20}`); `None` uses
    /// the global setting (ADR-003 §9).
    pub quota_reserve_json: Option<String>,
}

#[derive(Debug)]
pub struct NewAccount<'a> {
    pub provider_id: &'a str,
    pub alias: &'a str,
    pub display_name: Option<&'a str>,
    /// `api_key`, `subscription` or `local`.
    pub account_type: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Credential {
    pub id: String,
    pub account_id: String,
    /// `api_key`, `oauth` or `external_managed`.
    pub credential_type: String,
    /// Where it came from, e.g. `user` or `codex-cli`.
    pub source: String,
    pub secret_ref: Option<String>,
    pub scope: Option<String>,
    pub expires_at: Option<i64>,
}

#[derive(Debug)]
pub struct AccountRepo<'a> {
    db: &'a Database,
}

const COLUMNS: &str = "id, provider_id, alias, display_name, account_type, enabled, position, \
                       subscription_plan, price_monthly_usd, renew_at, created_at, updated_at, \
                       transport_opt_in_at, quota_reserve_json";

fn map_account(row: &Row<'_>) -> rusqlite::Result<Account> {
    Ok(Account {
        id: row.get(0)?,
        provider_id: row.get(1)?,
        alias: row.get(2)?,
        display_name: row.get(3)?,
        account_type: row.get(4)?,
        enabled: row.get::<_, i64>(5)? == 1,
        position: row.get(6)?,
        subscription_plan: row.get(7)?,
        price_monthly_usd: row.get(8)?,
        renew_at: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
        transport_opt_in_at: row.get(12)?,
        quota_reserve_json: row.get(13)?,
    })
}

fn map_credential(row: &Row<'_>) -> rusqlite::Result<Credential> {
    Ok(Credential {
        id: row.get(0)?,
        account_id: row.get(1)?,
        credential_type: row.get(2)?,
        source: row.get(3)?,
        secret_ref: row.get(4)?,
        scope: row.get(5)?,
        expires_at: row.get(6)?,
    })
}

impl<'a> AccountRepo<'a> {
    pub(crate) fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// Creates an account at the end of the provider's order.
    pub fn create(&self, new: &NewAccount<'_>) -> Result<Account, StorageError> {
        self.db.with_tx(|tx| {
            let position: i64 = tx.query_row(
                "SELECT COALESCE(MAX(position) + 1, 0) FROM accounts WHERE provider_id = ?1",
                params![new.provider_id],
                |r| r.get(0),
            )?;
            let now = now_ms();
            let a = Account {
                id: Uuid::new_v4().to_string(),
                provider_id: new.provider_id.to_owned(),
                alias: new.alias.to_owned(),
                display_name: new.display_name.map(str::to_owned),
                account_type: new.account_type.to_owned(),
                enabled: true,
                position,
                subscription_plan: None,
                price_monthly_usd: None,
                renew_at: None,
                created_at: now,
                updated_at: now,
                transport_opt_in_at: None,
                quota_reserve_json: None,
            };
            tx.execute(
                "INSERT INTO accounts (id, provider_id, alias, display_name, account_type, enabled,
                                       position, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7, ?7)",
                params![
                    a.id,
                    a.provider_id,
                    a.alias,
                    a.display_name,
                    a.account_type,
                    position,
                    now
                ],
            )
            .map_err(|e| StorageError::from_insert(e, format!("account '{}'", new.alias)))?;
            Ok(a)
        })
    }

    /// All accounts, or only those of one provider, in user order.
    pub fn list(&self, provider_id: Option<&str>) -> Result<Vec<Account>, StorageError> {
        self.db.with_conn(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM accounts WHERE ?1 IS NULL OR provider_id = ?1
                 ORDER BY provider_id, position, alias"
            ))?;
            let rows = stmt.query_map(params![provider_id], map_account)?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    pub fn get(&self, provider_id: &str, alias: &str) -> Result<Option<Account>, StorageError> {
        self.db.with_conn(|c| {
            Ok(c.query_row(
                &format!("SELECT {COLUMNS} FROM accounts WHERE provider_id = ?1 AND alias = ?2"),
                params![provider_id, alias],
                map_account,
            )
            .optional()?)
        })
    }

    pub fn get_by_id(&self, id: &str) -> Result<Option<Account>, StorageError> {
        self.db.with_conn(|c| {
            Ok(c.query_row(
                &format!("SELECT {COLUMNS} FROM accounts WHERE id = ?1"),
                params![id],
                map_account,
            )
            .optional()?)
        })
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<bool, StorageError> {
        self.db.with_conn(|c| {
            Ok(c.execute(
                "UPDATE accounts SET enabled = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, i64::from(enabled), now_ms()],
            )? > 0)
        })
    }

    pub fn set_position(&self, id: &str, position: i64) -> Result<bool, StorageError> {
        self.db.with_conn(|c| {
            Ok(c.execute(
                "UPDATE accounts SET position = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, position, now_ms()],
            )? > 0)
        })
    }

    pub fn set_plan(
        &self,
        id: &str,
        plan: Option<&str>,
        price_monthly_usd: Option<f64>,
        renew_at: Option<i64>,
    ) -> Result<bool, StorageError> {
        self.db.with_conn(|c| {
            Ok(c.execute(
                "UPDATE accounts SET subscription_plan = ?2, price_monthly_usd = ?3, renew_at = ?4,
                                     updated_at = ?5 WHERE id = ?1",
                params![id, plan, price_monthly_usd, renew_at, now_ms()],
            )? > 0)
        })
    }

    /// Deletes the account; returns the secret refs to remove from the OS store.
    pub fn delete(&self, id: &str) -> Result<Option<Vec<String>>, StorageError> {
        self.db.with_tx(|tx| {
            let exists: Option<String> = tx
                .query_row("SELECT id FROM accounts WHERE id = ?1", params![id], |r| r.get(0))
                .optional()?;
            if exists.is_none() {
                return Ok(None);
            }
            let mut stmt = tx.prepare(
                "SELECT secret_ref FROM credentials WHERE account_id = ?1 AND secret_ref IS NOT NULL",
            )?;
            let refs: Vec<String> = stmt
                .query_map(params![id], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            drop(stmt);
            tx.execute("DELETE FROM accounts WHERE id = ?1", params![id])?;
            Ok(Some(refs))
        })
    }

    /// Records (or clears, with `None`) the user's lower-trust transport confirmation.
    pub fn set_transport_opt_in(&self, id: &str, at: Option<i64>) -> Result<bool, StorageError> {
        self.db.with_conn(|c| {
            Ok(c.execute(
                "UPDATE accounts SET transport_opt_in_at = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, at, now_ms()],
            )? > 0)
        })
    }

    /// Stores (or clears, with `None`) the account's quota reserve JSON.
    pub fn set_quota_reserve(&self, id: &str, json: Option<&str>) -> Result<bool, StorageError> {
        self.db.with_conn(|c| {
            Ok(c.execute(
                "UPDATE accounts SET quota_reserve_json = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, json, now_ms()],
            )? > 0)
        })
    }

    // ---- credentials (metadata only; secrets live in the OS store, ADR-004) ----

    pub fn add_credential(
        &self,
        account_id: &str,
        credential_type: &str,
        source: &str,
        secret_ref: Option<&str>,
    ) -> Result<Credential, StorageError> {
        let now = now_ms();
        let cred = Credential {
            id: Uuid::new_v4().to_string(),
            account_id: account_id.to_owned(),
            credential_type: credential_type.to_owned(),
            source: source.to_owned(),
            secret_ref: secret_ref.map(str::to_owned),
            scope: None,
            expires_at: None,
        };
        self.db.with_conn(|c| {
            c.execute(
                "INSERT INTO credentials (id, account_id, credential_type, source, secret_ref,
                                          created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![
                    cred.id,
                    account_id,
                    credential_type,
                    source,
                    secret_ref,
                    now
                ],
            )?;
            Ok(())
        })?;
        Ok(cred)
    }

    pub fn credentials(&self, account_id: &str) -> Result<Vec<Credential>, StorageError> {
        self.db.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT id, account_id, credential_type, source, secret_ref, scope, expires_at
                 FROM credentials WHERE account_id = ?1 ORDER BY created_at",
            )?;
            let rows = stmt.query_map(params![account_id], map_credential)?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    /// Replaces the account's OAuth tokens (`scope` `access` / `refresh`) in one transaction.
    /// Returns the secret refs no longer referenced, which the caller deletes from the OS
    /// store *after* this succeeds.
    pub fn set_oauth_tokens(
        &self,
        account_id: &str,
        source: &str,
        access_ref: &str,
        refresh_ref: Option<&str>,
        expires_at: Option<i64>,
    ) -> Result<Vec<String>, StorageError> {
        let now = now_ms();
        self.db.with_tx(|tx| {
            let mut stmt = tx.prepare(
                "SELECT secret_ref FROM credentials
                 WHERE account_id = ?1 AND credential_type = 'oauth' AND secret_ref IS NOT NULL",
            )?;
            let old: Vec<String> = stmt
                .query_map(params![account_id], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            drop(stmt);
            tx.execute(
                "DELETE FROM credentials WHERE account_id = ?1 AND credential_type = 'oauth'",
                params![account_id],
            )?;
            let insert = |scope: &str, secret_ref: &str, expires: Option<i64>| {
                tx.execute(
                    "INSERT INTO credentials (id, account_id, credential_type, source, secret_ref,
                                              scope, expires_at, created_at, updated_at)
                     VALUES (?1, ?2, 'oauth', ?3, ?4, ?5, ?6, ?7, ?7)",
                    params![
                        Uuid::new_v4().to_string(),
                        account_id,
                        source,
                        secret_ref,
                        scope,
                        expires,
                        now
                    ],
                )
            };
            insert("access", access_ref, expires_at)?;
            if let Some(r) = refresh_ref {
                insert("refresh", r, None)?;
            }
            Ok(old
                .into_iter()
                .filter(|r| r != access_ref && Some(r.as_str()) != refresh_ref)
                .collect())
        })
    }

    /// OAuth access credentials expiring before `before` (unix ms), with their accounts.
    pub fn oauth_expiring(&self, before: i64) -> Result<Vec<Credential>, StorageError> {
        self.db.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT id, account_id, credential_type, source, secret_ref, scope, expires_at
                 FROM credentials
                 WHERE credential_type = 'oauth' AND scope = 'access'
                   AND expires_at IS NOT NULL AND expires_at < ?1
                 ORDER BY expires_at",
            )?;
            let rows = stmt.query_map(params![before], map_credential)?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    /// Replaces all credentials of an account with one; returns the old secret refs.
    pub fn replace_credential(
        &self,
        account_id: &str,
        credential_type: &str,
        source: &str,
        secret_ref: Option<&str>,
    ) -> Result<Vec<String>, StorageError> {
        let old = self.db.with_tx(|tx| {
            let mut stmt = tx.prepare(
                "SELECT secret_ref FROM credentials WHERE account_id = ?1 AND secret_ref IS NOT NULL",
            )?;
            let refs: Vec<String> = stmt
                .query_map(params![account_id], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            drop(stmt);
            tx.execute("DELETE FROM credentials WHERE account_id = ?1", params![account_id])?;
            Ok(refs)
        })?;
        self.add_credential(account_id, credential_type, source, secret_ref)?;
        Ok(old)
    }
}
