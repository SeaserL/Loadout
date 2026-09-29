// SPDX-License-Identifier: Apache-2.0

use rusqlite::{OptionalExtension, Row, params};
use uuid::Uuid;

use crate::{Database, StorageError, now_ms};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provider {
    pub id: String,
    pub slug: String,
    pub kind: String,
    pub display_name: String,
    pub base_url: Option<String>,
    pub config_json: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug)]
pub struct NewProvider<'a> {
    pub slug: &'a str,
    pub kind: &'a str,
    pub display_name: &'a str,
    pub base_url: Option<&'a str>,
    pub config_json: &'a str,
}

#[derive(Debug)]
pub struct ProviderRepo<'a> {
    db: &'a Database,
}

const COLUMNS: &str = "id, slug, kind, display_name, base_url, config_json, created_at, updated_at";

pub(crate) fn map_provider(row: &Row<'_>) -> rusqlite::Result<Provider> {
    Ok(Provider {
        id: row.get(0)?,
        slug: row.get(1)?,
        kind: row.get(2)?,
        display_name: row.get(3)?,
        base_url: row.get(4)?,
        config_json: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

impl<'a> ProviderRepo<'a> {
    pub(crate) fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn create(&self, new: &NewProvider<'_>) -> Result<Provider, StorageError> {
        let now = now_ms();
        let p = Provider {
            id: Uuid::new_v4().to_string(),
            slug: new.slug.to_owned(),
            kind: new.kind.to_owned(),
            display_name: new.display_name.to_owned(),
            base_url: new.base_url.map(str::to_owned),
            config_json: new.config_json.to_owned(),
            created_at: now,
            updated_at: now,
        };
        self.db.with_conn(|c| {
            c.execute(
                &format!(
                    "INSERT INTO providers ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
                ),
                params![
                    p.id,
                    p.slug,
                    p.kind,
                    p.display_name,
                    p.base_url,
                    p.config_json,
                    now,
                    now
                ],
            )
            .map_err(|e| StorageError::from_insert(e, format!("provider '{}'", new.slug)))?;
            Ok(())
        })?;
        Ok(p)
    }

    pub fn list(&self) -> Result<Vec<Provider>, StorageError> {
        self.db.with_conn(|c| {
            let mut stmt = c.prepare(&format!("SELECT {COLUMNS} FROM providers ORDER BY slug"))?;
            let rows = stmt.query_map([], map_provider)?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    pub fn get_by_slug(&self, slug: &str) -> Result<Option<Provider>, StorageError> {
        self.db.with_conn(|c| {
            Ok(c.query_row(
                &format!("SELECT {COLUMNS} FROM providers WHERE slug = ?1"),
                params![slug],
                map_provider,
            )
            .optional()?)
        })
    }

    pub fn update(
        &self,
        slug: &str,
        base_url: Option<&str>,
        config_json: &str,
    ) -> Result<bool, StorageError> {
        self.db.with_conn(|c| {
            Ok(c.execute(
                "UPDATE providers SET base_url = ?2, config_json = ?3, updated_at = ?4 WHERE slug = ?1",
                params![slug, base_url, config_json, now_ms()],
            )? > 0)
        })
    }

    /// Deletes the provider and (by cascade) its accounts, credentials and models. Returns
    /// the secret refs that must be removed from the OS store.
    pub fn delete_by_slug(&self, slug: &str) -> Result<Option<Vec<String>>, StorageError> {
        self.db.with_tx(|tx| {
            let Some(id): Option<String> = tx
                .query_row(
                    "SELECT id FROM providers WHERE slug = ?1",
                    params![slug],
                    |r| r.get(0),
                )
                .optional()?
            else {
                return Ok(None);
            };
            let mut stmt = tx.prepare(
                "SELECT c.secret_ref FROM credentials c JOIN accounts a ON a.id = c.account_id
                 WHERE a.provider_id = ?1 AND c.secret_ref IS NOT NULL",
            )?;
            let refs: Vec<String> = stmt
                .query_map(params![id], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            drop(stmt);
            tx.execute("DELETE FROM providers WHERE id = ?1", params![id])?;
            Ok(Some(refs))
        })
    }
}
