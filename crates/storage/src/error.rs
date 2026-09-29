// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("cannot create database directory: {0}")]
    Io(#[from] std::io::Error),

    #[error("migration error: {0}")]
    Migration(#[from] rusqlite_migration::Error),

    #[error(
        "database schema version {found} is newer than this build supports ({supported}); \
         upgrade Loadout"
    )]
    SchemaTooNew { found: usize, supported: usize },

    #[error("{0} already exists")]
    Conflict(String),

    #[error("{0} not found")]
    NotFound(String),

    #[error("corrupt row: {0}")]
    Corrupt(String),
}

impl StorageError {
    /// Maps a UNIQUE constraint violation to [`StorageError::Conflict`].
    pub(crate) fn from_insert(err: rusqlite::Error, what: impl Into<String>) -> Self {
        match &err {
            rusqlite::Error::SqliteFailure(e, _)
                if e.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
                    || e.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY =>
            {
                Self::Conflict(what.into())
            }
            _ => Self::Sqlite(err),
        }
    }
}
