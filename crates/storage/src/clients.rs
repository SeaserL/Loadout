// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use rusqlite::{OptionalExtension, Row, params};
use uuid::Uuid;

use crate::{Database, StorageError, now_ms};

/// Identifier of a local gateway client (ADR-011).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ClientId(String);

impl ClientId {
    pub fn generate() -> Self {
        Self(Uuid::new_v4().hyphenated().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ClientId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for ClientId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// A client row. The key hash is intentionally not part of this view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Client {
    pub id: ClientId,
    pub name: String,
    pub key_prefix: String,
    pub enabled: bool,
    pub created_at: i64,
    pub revoked_at: Option<i64>,
    /// JSON object of the client's policy (ExecPlan 006); `{}` when none.
    pub policy_json: String,
}

#[derive(Debug)]
pub struct NewClient<'a> {
    pub name: &'a str,
    pub key_prefix: &'a str,
    pub key_hash: [u8; 32],
}

#[derive(Debug)]
pub struct ClientRepo<'a> {
    db: &'a Database,
}

const COLUMNS: &str = "id, name, key_prefix, enabled, created_at, revoked_at, policy_json";

impl<'a> ClientRepo<'a> {
    pub(crate) fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn create(&self, new: &NewClient<'_>) -> Result<Client, StorageError> {
        let client = Client {
            id: ClientId::generate(),
            name: new.name.to_owned(),
            key_prefix: new.key_prefix.to_owned(),
            enabled: true,
            created_at: now_ms(),
            revoked_at: None,
            policy_json: "{}".to_owned(),
        };
        self.db.with_conn(|c| {
            c.execute(
                "INSERT INTO clients (id, name, key_prefix, key_hash, enabled, created_at)
                 VALUES (?1, ?2, ?3, ?4, 1, ?5)",
                params![
                    client.id.as_str(),
                    client.name,
                    client.key_prefix,
                    &new.key_hash[..],
                    client.created_at
                ],
            )
            .map_err(|e| StorageError::from_insert(e, format!("client '{}'", new.name)))?;
            Ok(())
        })?;
        Ok(client)
    }

    pub fn list(&self) -> Result<Vec<Client>, StorageError> {
        self.db.with_conn(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM clients ORDER BY created_at, name"
            ))?;
            let rows = stmt.query_map([], map_client)?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    pub fn get_by_name(&self, name: &str) -> Result<Option<Client>, StorageError> {
        self.db.with_conn(|c| {
            Ok(c.query_row(
                &format!("SELECT {COLUMNS} FROM clients WHERE name = ?1"),
                params![name],
                map_client,
            )
            .optional()?)
        })
    }

    /// Disables the client and records the revocation time. The row is kept so request
    /// history stays attributable. Returns `false` if no enabled client has that name.
    pub fn revoke_by_name(&self, name: &str) -> Result<bool, StorageError> {
        self.db.with_conn(|c| {
            Ok(c.execute(
                "UPDATE clients SET enabled = 0, revoked_at = ?2
                 WHERE name = ?1 AND enabled = 1",
                params![name, now_ms()],
            )? > 0)
        })
    }

    pub fn set_policy(&self, name: &str, policy_json: &str) -> Result<bool, StorageError> {
        self.db.with_conn(|c| {
            Ok(c.execute(
                "UPDATE clients SET policy_json = ?2 WHERE name = ?1",
                params![name, policy_json],
            )? == 1)
        })
    }

    /// The stored key hash (for configuration export; the hash cannot be reversed).
    pub fn key_hash(&self, name: &str) -> Result<Option<[u8; 32]>, StorageError> {
        self.db.with_conn(|c| {
            let hash: Option<Vec<u8>> = c
                .query_row(
                    "SELECT key_hash FROM clients WHERE name = ?1",
                    params![name],
                    |r| r.get(0),
                )
                .optional()?;
            hash.map(|h| {
                h.try_into()
                    .map_err(|_| StorageError::Corrupt("client key_hash length".into()))
            })
            .transpose()
        })
    }

    /// Restores a revocation (configuration import).
    pub fn set_revoked(&self, name: &str, revoked_at: i64) -> Result<bool, StorageError> {
        self.db.with_conn(|c| {
            Ok(c.execute(
                "UPDATE clients SET enabled = 0, revoked_at = ?2 WHERE name = ?1",
                params![name, revoked_at],
            )? == 1)
        })
    }

    /// Tokens and estimated cost per client id since `since_ms` (restores the in-memory
    /// daily counters after a restart).
    pub fn usage_since(&self, since_ms: i64) -> Result<Vec<(String, u64, f64)>, StorageError> {
        self.db.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT client_id,
                        COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)), 0),
                        COALESCE(SUM(estimated_cost_usd), 0.0)
                   FROM request_events
                  WHERE ts >= ?1 AND client_id IS NOT NULL
                  GROUP BY client_id",
            )?;
            let rows = stmt.query_map(params![since_ms], |r| {
                Ok((r.get(0)?, r.get::<_, i64>(1)?.max(0) as u64, r.get(2)?))
            })?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    /// `(key_hash, client_id)` for every enabled client; used to build the in-memory
    /// authentication index so the gateway never queries SQLite per request.
    pub fn active_key_hashes(&self) -> Result<Vec<([u8; 32], ClientId)>, StorageError> {
        self.db.with_conn(|c| {
            let mut stmt = c.prepare("SELECT key_hash, id FROM clients WHERE enabled = 1")?;
            let mut rows = stmt.query([])?;
            let mut out = Vec::new();
            while let Some(row) = rows.next()? {
                let hash: Vec<u8> = row.get(0)?;
                let hash: [u8; 32] = hash
                    .try_into()
                    .map_err(|_| StorageError::Corrupt("client key_hash length".into()))?;
                out.push((hash, ClientId(row.get(1)?)));
            }
            Ok(out)
        })
    }
}

fn map_client(row: &Row<'_>) -> rusqlite::Result<Client> {
    Ok(Client {
        id: ClientId(row.get(0)?),
        name: row.get(1)?,
        key_prefix: row.get(2)?,
        enabled: row.get::<_, i64>(3)? == 1,
        created_at: row.get(4)?,
        revoked_at: row.get(5)?,
        policy_json: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_client(name: &str, fill: u8) -> NewClient<'_> {
        NewClient {
            name,
            key_prefix: "lo-abcd",
            key_hash: [fill; 32],
        }
    }

    #[test]
    fn create_list_revoke() {
        let db = Database::open_in_memory().expect("open");
        let repo = db.clients();

        let a = repo.create(&new_client("trading", 1)).expect("create a");
        let b = repo.create(&new_client("opencode", 2)).expect("create b");
        assert!(a.enabled);

        let names: Vec<_> = repo
            .list()
            .expect("list")
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"trading".to_owned()));

        assert_eq!(repo.active_key_hashes().expect("hashes").len(), 2);

        assert!(repo.revoke_by_name("trading").expect("revoke"));
        assert!(!repo.revoke_by_name("trading").expect("revoke twice"));
        assert!(!repo.revoke_by_name("missing").expect("revoke missing"));

        let revoked = repo.get_by_name("trading").expect("get").expect("exists");
        assert!(!revoked.enabled);
        assert!(revoked.revoked_at.is_some());

        let active = repo.active_key_hashes().expect("hashes");
        assert_eq!(active, vec![([2; 32], b.id)]);

        assert!(
            repo.set_policy("opencode", r#"{"requests_per_minute":5}"#)
                .expect("policy")
        );
        assert!(!repo.set_policy("missing", "{}").expect("policy"));
        let b = repo.get_by_name("opencode").expect("get").expect("exists");
        assert_eq!(b.policy_json, r#"{"requests_per_minute":5}"#);
        assert_eq!(repo.key_hash("opencode").expect("hash"), Some([2; 32]));
        assert!(repo.usage_since(0).expect("usage").is_empty());
    }

    #[test]
    fn duplicate_name_or_hash_is_conflict() {
        let db = Database::open_in_memory().expect("open");
        let repo = db.clients();
        repo.create(&new_client("one", 1)).expect("create");

        assert!(matches!(
            repo.create(&new_client("one", 9)),
            Err(StorageError::Conflict(_))
        ));
        assert!(matches!(
            repo.create(&new_client("two", 1)),
            Err(StorageError::Conflict(_))
        ));
    }
}
