// SPDX-License-Identifier: Apache-2.0

//! `response_id → route_target` for stateful Responses requests (ADR-006 §3).

use rusqlite::{Connection, OptionalExtension, params};

use crate::{Database, StorageError};

pub(crate) fn upsert(
    conn: &Connection,
    response_id: &str,
    target: &str,
    now: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO response_routes (response_id, route_target, created_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(response_id) DO UPDATE SET route_target = excluded.route_target",
        params![response_id, target, now],
    )?;
    Ok(())
}

impl Database {
    pub fn response_route(&self, response_id: &str) -> Result<Option<String>, StorageError> {
        self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT route_target FROM response_routes WHERE response_id = ?1",
                params![response_id],
                |r| r.get(0),
            )
            .optional()?)
        })
    }

    pub fn put_response_route(
        &self,
        response_id: &str,
        target: &str,
        now: i64,
    ) -> Result<(), StorageError> {
        self.with_conn(|c| Ok(upsert(c, response_id, target, now)?))
    }

    pub fn prune_response_routes(&self, before_ms: i64) -> Result<usize, StorageError> {
        self.with_conn(|c| {
            Ok(c.execute(
                "DELETE FROM response_routes WHERE created_at < ?1",
                params![before_ms],
            )?)
        })
    }
}
