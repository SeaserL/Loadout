// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, Transaction};
use rusqlite_migration::SchemaVersion;

use crate::migrations::{LATEST_VERSION, migrations};
use crate::{ClientRepo, SettingsRepo, StorageError};

/// Milliseconds since the Unix epoch; the timestamp unit for every table.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Handle to the Loadout database.
///
/// Phase 1 uses a single connection behind a mutex. All calls are blocking and must stay off
/// the gateway hot path (Harness §2.2); async callers use `spawn_blocking`. High-volume
/// telemetry gets its own dedicated writer in Phase 2 (ADR-008).
#[derive(Debug)]
pub struct Database {
    conn: Mutex<Connection>,
    path: Option<PathBuf>,
}

impl Database {
    /// Opens (creating if needed) the database file, applies pragmas and migrates to the
    /// latest schema. Fails if the file was written by a newer Loadout.
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(path)?;
        configure(&conn, true)?;
        migrate(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            path: Some(path.to_path_buf()),
        })
    }

    /// Like [`Database::open`], but a damaged file is moved aside (with its WAL files) to
    /// `<name>.corrupt-<unix ms>` and a new, empty database is created. Returns the path the
    /// damaged file was moved to (ExecPlan 007 §2).
    pub fn open_or_recover(path: &Path) -> Result<(Self, Option<PathBuf>), StorageError> {
        match Self::open(path) {
            Ok(db) => Ok((db, None)),
            Err(e) if e.is_corruption() => {
                let suffix = format!("corrupt-{}", now_ms());
                let moved = path.with_extension(format!("db.{suffix}"));
                std::fs::rename(path, &moved)?;
                for ext in ["db-wal", "db-shm"] {
                    let side = path.with_extension(ext);
                    if side.exists() {
                        std::fs::rename(&side, path.with_extension(format!("{ext}.{suffix}")))?;
                    }
                }
                Ok((Self::open(path)?, Some(moved)))
            }
            Err(e) => Err(e),
        }
    }

    /// In-memory database for tests.
    pub fn open_in_memory() -> Result<Self, StorageError> {
        let mut conn = Connection::open_in_memory()?;
        configure(&conn, false)?;
        migrate(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            path: None,
        })
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn with_conn<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        f(&self.lock())
    }

    /// Runs `f` in a transaction that commits on `Ok` and rolls back on `Err`.
    pub fn with_tx<T>(
        &self,
        f: impl FnOnce(&Transaction<'_>) -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let value = f(&tx)?;
        tx.commit()?;
        Ok(value)
    }

    pub fn settings(&self) -> SettingsRepo<'_> {
        SettingsRepo::new(self)
    }

    pub fn clients(&self) -> ClientRepo<'_> {
        ClientRepo::new(self)
    }

    pub fn schema_version(&self) -> Result<usize, StorageError> {
        let conn = self.lock();
        Ok(match migrations().current_version(&conn)? {
            SchemaVersion::NoneSet => 0,
            SchemaVersion::Inside(v) | SchemaVersion::Outside(v) => v.get(),
        })
    }

    /// Writes a consistent copy of the whole database to `path`, which must not exist
    /// (`VACUUM INTO`). The database holds no secrets (ADR-004).
    pub fn backup_to(&self, path: &Path) -> Result<(), StorageError> {
        let target = path
            .to_str()
            .ok_or_else(|| StorageError::Corrupt("backup path is not valid UTF-8".into()))?;
        self.with_conn(|c| {
            c.execute("VACUUM INTO ?1", [target])?;
            Ok(())
        })
    }

    pub fn journal_mode(&self) -> Result<String, StorageError> {
        let conn = self.lock();
        Ok(conn.query_row("PRAGMA journal_mode", [], |row| row.get(0))?)
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock cannot leave the connection structurally invalid:
        // uncommitted transactions are rolled back when dropped.
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn configure(conn: &Connection, file_backed: bool) -> Result<(), StorageError> {
    if file_backed {
        let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(StorageError::Corrupt(format!(
                "could not enable WAL mode (got {mode})"
            )));
        }
    }
    conn.execute_batch(
        "PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )?;
    Ok(())
}

fn migrate(conn: &mut Connection) -> Result<(), StorageError> {
    let m = migrations();
    if let SchemaVersion::Outside(found) = m.current_version(conn)? {
        return Err(StorageError::SchemaTooNew {
            found: found.get(),
            supported: LATEST_VERSION,
        });
    }
    m.to_latest(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_damaged_file_is_moved_aside() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("loadout.db");
        std::fs::write(&path, vec![0x5a; 8192]).expect("write garbage");
        std::fs::write(path.with_extension("db-wal"), b"junk").expect("write wal");

        let (db, moved) = Database::open_or_recover(&path).expect("recover");
        let moved = moved.expect("moved aside");
        assert!(moved.to_string_lossy().contains("loadout.db.corrupt-"));
        assert_eq!(std::fs::read(&moved).expect("kept").len(), 8192);
        assert_eq!(db.schema_version().expect("version"), LATEST_VERSION);
        drop(db);

        let (_, again) = Database::open_or_recover(&path).expect("reopen");
        assert!(again.is_none(), "a healthy database is opened normally");
    }

    #[test]
    fn file_database_uses_wal_and_latest_schema() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested").join("loadout.db");

        let db = Database::open(&path).expect("open");
        assert_eq!(db.journal_mode().expect("mode"), "wal");
        assert_eq!(db.schema_version().expect("version"), LATEST_VERSION);
        assert_eq!(db.path(), Some(path.as_path()));

        let fk: i64 = db
            .with_conn(|c| Ok(c.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?))
            .expect("pragma");
        assert_eq!(fk, 1);
    }

    #[test]
    fn reopening_is_idempotent_and_keeps_data() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("loadout.db");

        Database::open(&path)
            .expect("open")
            .settings()
            .set("gateway.port", "9000")
            .expect("set");

        let db = Database::open(&path).expect("reopen");
        assert_eq!(
            db.settings().get("gateway.port").expect("get").as_deref(),
            Some("9000")
        );
    }

    #[test]
    fn refuses_schema_from_newer_build() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("loadout.db");
        drop(Database::open(&path).expect("open"));

        let future = LATEST_VERSION + 5;
        Connection::open(&path)
            .expect("raw open")
            .pragma_update(None, "user_version", future as i64)
            .expect("bump version");

        match Database::open(&path) {
            Err(StorageError::SchemaTooNew { found, supported }) => {
                assert_eq!(found, future);
                assert_eq!(supported, LATEST_VERSION);
            }
            other => panic!("expected SchemaTooNew, got {other:?}"),
        }
    }

    #[test]
    fn upgrades_a_version_1_database_keeping_data() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("loadout.db");
        {
            let mut conn = Connection::open(&path).expect("raw open");
            migrations().to_version(&mut conn, 1).expect("v1");
            conn.execute(
                "INSERT INTO clients (id, name, key_prefix, key_hash, created_at)
                 VALUES ('c1', 'old client', 'lo-12345678', ?1, 1)",
                [vec![7u8; 32]],
            )
            .expect("insert");
        }
        let db = Database::open(&path).expect("upgrade");
        assert_eq!(db.schema_version().expect("version"), LATEST_VERSION);
        let clients = db.clients().list().expect("list");
        assert_eq!(clients.len(), 1);
        assert_eq!(clients[0].name, "old client");
        assert!(db.providers().list().expect("providers").is_empty());
    }

    #[test]
    fn transaction_rolls_back_on_error() {
        let db = Database::open_in_memory().expect("open");
        let result: Result<(), StorageError> = db.with_tx(|tx| {
            tx.execute(
                "INSERT INTO settings (key, value, updated_at) VALUES ('a', '1', 0)",
                [],
            )?;
            Err(StorageError::NotFound("forced".into()))
        });
        assert!(result.is_err());
        assert_eq!(db.settings().get("a").expect("get"), None);
    }
}
