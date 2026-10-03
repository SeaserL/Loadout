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
    M::up(include_str!(
        "../migrations/0008_account_quota_reserve/up.sql"
    ))
    .down(include_str!(
        "../migrations/0008_account_quota_reserve/down.sql"
    )),
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
}
