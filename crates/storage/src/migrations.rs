// SPDX-License-Identifier: Apache-2.0

//! Versioned schema migrations (Harness §21). Every migration ships an `up` and a `down`
//! script and is covered by the round-trip test below. Never edit a released migration;
//! add a new one.

use rusqlite_migration::{M, Migrations};

const MIGRATIONS: &[M<'static>] = &[
    M::up(include_str!("../migrations/0001_init/up.sql"))
        .down(include_str!("../migrations/0001_init/down.sql")),
    M::up(include_str!(
        "../migrations/0002_accounts_models_telemetry/up.sql"
    ))
    .down(include_str!(
        "../migrations/0002_accounts_models_telemetry/down.sql"
    )),
    M::up(include_str!("../migrations/0003_usage/up.sql"))
        .down(include_str!("../migrations/0003_usage/down.sql")),
    M::up(include_str!("../migrations/0004_latency_pools/up.sql"))
        .down(include_str!("../migrations/0004_latency_pools/down.sql")),
    M::up(include_str!("../migrations/0005_client_policies/up.sql"))
        .down(include_str!("../migrations/0005_client_policies/down.sql")),
    M::up(include_str!("../migrations/0006_transport_opt_in/up.sql"))
        .down(include_str!("../migrations/0006_transport_opt_in/down.sql")),
    M::up(include_str!("../migrations/0009_request_decision/up.sql"))
        .down(include_str!("../migrations/0009_request_decision/down.sql")),
];

/// Schema version after applying every migration.
pub const LATEST_VERSION: usize = MIGRATIONS.len();

pub fn migrations() -> Migrations<'static> {
    Migrations::from_slice(MIGRATIONS)
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;
    use rusqlite_migration::SchemaVersion;

    use super::*;

    fn table_names(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .expect("prepare");
        stmt.query_map([], |row| row.get(0))
            .expect("query")
            .collect::<Result<_, _>>()
            .expect("collect")
    }

    #[test]
    fn migrations_are_valid() {
        migrations().validate().expect("valid migrations");
    }

    #[test]
    fn every_migration_round_trips() {
        let mut conn = Connection::open_in_memory().expect("open");
        let m = migrations();

        m.to_latest(&mut conn).expect("up");
        let latest_tables = table_names(&conn);
        assert_eq!(
            m.current_version(&conn).expect("version"),
            SchemaVersion::Inside(LATEST_VERSION.try_into().expect("non-zero"))
        );

        // Step down to zero one version at a time, then back up.
        for version in (0..LATEST_VERSION).rev() {
            m.to_version(&mut conn, version).expect("down");
        }
        assert!(
            table_names(&conn).is_empty(),
            "down must remove every table"
        );

        m.to_latest(&mut conn).expect("up again");
        assert_eq!(table_names(&conn), latest_tables);
    }

    fn columns(conn: &Connection, table: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .expect("prepare");
        stmt.query_map([], |row| row.get(0))
            .expect("query")
            .collect::<Result<_, _>>()
            .expect("collect")
    }

    #[test]
    fn request_decision_column_round_trips() {
        let mut conn = Connection::open_in_memory().expect("open");
        let m = migrations();
        m.to_latest(&mut conn).expect("up");
        assert!(columns(&conn, "request_events").contains(&"decision".to_owned()));
        conn.execute(
            "INSERT INTO request_events (ts, requested_model, protocol_in, stream, status, \
             attempts, duration_ms, decision) VALUES (1, 'loadout/default', 'chat', 0, 'ok', 1, \
             5, 'a/b/m: ok')",
            [],
        )
        .expect("insert");

        // The decision migration is the last one: stepping down drops only the column.
        m.to_version(&mut conn, LATEST_VERSION - 1).expect("down");
        assert!(!columns(&conn, "request_events").contains(&"decision".to_owned()));
        let kept: i64 = conn
            .query_row("SELECT COUNT(*) FROM request_events", [], |r| r.get(0))
            .expect("count");
        assert_eq!(kept, 1, "rows survive the down migration");

        m.to_latest(&mut conn).expect("up again");
        let decision: Option<String> = conn
            .query_row("SELECT decision FROM request_events", [], |r| r.get(0))
            .expect("decision");
        assert_eq!(decision, None);
    }
}
