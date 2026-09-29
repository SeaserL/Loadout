// SPDX-License-Identifier: Apache-2.0

use rusqlite::{OptionalExtension, params};

use crate::{Database, StorageError, now_ms};

/// Key/value settings. Keys are dotted names such as `gateway.port`; validation of values is
/// the caller's responsibility.
#[derive(Debug)]
pub struct SettingsRepo<'a> {
    db: &'a Database,
}

impl<'a> SettingsRepo<'a> {
    pub(crate) fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        self.db.with_conn(|c| {
            Ok(c.query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?)
        })
    }

    pub fn set(&self, key: &str, value: &str) -> Result<(), StorageError> {
        self.db.with_conn(|c| {
            c.execute(
                "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value,
                                                updated_at = excluded.updated_at",
                params![key, value, now_ms()],
            )?;
            Ok(())
        })
    }

    /// Removes a setting so the built-in default applies again. Returns `false` if unset.
    pub fn remove(&self, key: &str) -> Result<bool, StorageError> {
        self.db
            .with_conn(|c| Ok(c.execute("DELETE FROM settings WHERE key = ?1", params![key])? > 0))
    }

    pub fn all(&self) -> Result<Vec<(String, String)>, StorageError> {
        self.db.with_conn(|c| {
            let mut stmt = c.prepare("SELECT key, value FROM settings ORDER BY key")?;
            let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_overwrite_remove() {
        let db = Database::open_in_memory().expect("open");
        let s = db.settings();

        assert_eq!(s.get("gateway.port").expect("get"), None);
        s.set("gateway.port", "8317").expect("set");
        s.set("gateway.port", "9000").expect("overwrite");
        s.set("a.first", "x").expect("set");

        assert_eq!(s.get("gateway.port").expect("get").as_deref(), Some("9000"));
        assert_eq!(
            s.all().expect("all"),
            vec![
                ("a.first".to_owned(), "x".to_owned()),
                ("gateway.port".to_owned(), "9000".to_owned()),
            ]
        );

        assert!(s.remove("gateway.port").expect("remove"));
        assert!(!s.remove("gateway.port").expect("remove again"));
        assert_eq!(s.get("gateway.port").expect("get"), None);
    }
}
