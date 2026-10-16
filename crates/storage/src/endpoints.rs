// SPDX-License-Identifier: Apache-2.0

//! Dedicated endpoints (ADR-015, ExecPlan 021): `/e/<name>/v1` scoped to one provider.

use rusqlite::{OptionalExtension, params};

use crate::{Database, StorageError, now_ms};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointRow {
    pub name: String,
    pub provider_slug: String,
    pub enabled: bool,
    /// Account ids; `None` = every account of the provider in the current pool.
    pub members: Option<Vec<String>>,
    pub default_model: Option<String>,
    pub wait_secs: u32,
}

#[derive(Debug)]
pub struct EndpointRepo<'a> {
    db: &'a Database,
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<EndpointRow> {
    let members: Option<String> = r.get(3)?;
    Ok(EndpointRow {
        name: r.get(0)?,
        provider_slug: r.get(1)?,
        enabled: r.get::<_, i64>(2)? == 1,
        members: members.and_then(|m| serde_json::from_str(&m).ok()),
        default_model: r.get(4)?,
        wait_secs: u32::try_from(r.get::<_, i64>(5)?).unwrap_or(0),
    })
}

const COLUMNS: &str = "name, provider_slug, enabled, members_json, default_model, wait_secs";

impl<'a> EndpointRepo<'a> {
    pub(crate) fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn list(&self) -> Result<Vec<EndpointRow>, StorageError> {
        self.db.with_conn(|c| {
            let mut stmt = c.prepare(&format!("SELECT {COLUMNS} FROM endpoints ORDER BY name"))?;
            let rows = stmt.query_map([], row)?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    pub fn get(&self, name: &str) -> Result<Option<EndpointRow>, StorageError> {
        self.db.with_conn(|c| {
            Ok(c.query_row(
                &format!("SELECT {COLUMNS} FROM endpoints WHERE name = ?1"),
                params![name],
                row,
            )
            .optional()?)
        })
    }

    /// Creates or replaces an endpoint.
    pub fn save(&self, e: &EndpointRow) -> Result<(), StorageError> {
        let members = e
            .members
            .as_ref()
            .map(|m| serde_json::to_string(m).unwrap_or_else(|_| "[]".into()));
        let now = now_ms();
        self.db.with_conn(|c| {
            c.execute(
                "INSERT INTO endpoints (name, provider_slug, enabled, members_json, default_model,
                                        wait_secs, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                 ON CONFLICT(name) DO UPDATE SET provider_slug = excluded.provider_slug,
                     enabled = excluded.enabled, members_json = excluded.members_json,
                     default_model = excluded.default_model, wait_secs = excluded.wait_secs,
                     updated_at = excluded.updated_at",
                params![
                    e.name,
                    e.provider_slug,
                    i64::from(e.enabled),
                    members,
                    e.default_model,
                    i64::from(e.wait_secs),
                    now
                ],
            )?;
            Ok(())
        })
    }

    pub fn delete(&self, name: &str) -> Result<bool, StorageError> {
        self.db.with_conn(|c| {
            Ok(c.execute("DELETE FROM endpoints WHERE name = ?1", params![name])? == 1)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_list_replace_delete() {
        let db = Database::open_in_memory().expect("db");
        let repo = db.endpoints();
        let mut e = EndpointRow {
            name: "sensenova".into(),
            provider_slug: "sensenova".into(),
            enabled: true,
            members: None,
            default_model: None,
            wait_secs: 30,
        };
        repo.save(&e).expect("save");
        assert_eq!(repo.get("sensenova").expect("get"), Some(e.clone()));

        e.members = Some(vec!["a1".into(), "a2".into()]);
        e.default_model = Some("deepseek-v4-flash".into());
        e.enabled = false;
        e.wait_secs = 0;
        repo.save(&e).expect("replace");
        assert_eq!(repo.list().expect("list"), vec![e.clone()]);

        e.wait_secs = 301;
        assert!(repo.save(&e).is_err(), "CHECK limits the wait");

        assert!(repo.delete("sensenova").expect("delete"));
        assert!(repo.get("sensenova").expect("get").is_none());
    }
}
