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
    M::up(include_str!("../migrations/0007_price_source/up.sql"))
        .down(include_str!("../migrations/0007_price_source/down.sql")),
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

    #[test]
    fn price_source_backfill_marks_existing_prices() {
        let mut conn = Connection::open_in_memory().expect("open");
        let m = migrations();
        m.to_version(&mut conn, 6).expect("up to 6");
        conn.execute_batch(
            "INSERT INTO providers (id, slug, kind, display_name, created_at, updated_at)
                VALUES ('p', 'p', 'openai-compatible', 'P', 1, 1);
             INSERT INTO accounts (id, provider_id, alias, account_type, created_at, updated_at)
                VALUES ('a', 'p', 'main', 'api_key', 1, 1);
             INSERT INTO account_models (id, account_id, upstream_model_id, input_price,
                    enabled, source, created_at, updated_at)
                VALUES ('m1', 'a', 'manual-priced', 1.0, 1, 'manual', 1, 5),
                       ('m2', 'a', 'catalog-priced', 2.0, 1, 'catalog', 1, 6),
                       ('m3', 'a', 'unpriced', NULL, 1, 'catalog', 1, 7);",
        )
        .expect("seed");
        m.to_version(&mut conn, 7).expect("up to 7");
        let row = |id: &str| -> (Option<String>, Option<i64>) {
            conn.query_row(
                "SELECT price_source, price_updated_at FROM account_models WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("row")
        };
        assert_eq!(row("m1"), (Some("user".into()), Some(5)));
        assert_eq!(row("m2"), (Some("catalog".into()), Some(6)));
        assert_eq!(row("m3"), (None, None));
        assert!(
            conn.execute(
                "UPDATE account_models SET price_source = 'bogus' WHERE id = 'm1'",
                []
            )
            .is_err(),
            "price_source is constrained"
        );
        m.to_version(&mut conn, 6).expect("down to 6");
        assert_eq!(
            conn.query_row(
                "SELECT input_price FROM account_models WHERE id = 'm2'",
                [],
                |r| r.get::<_, Option<f64>>(0)
            )
            .expect("price kept"),
            Some(2.0)
        );
    }
}
