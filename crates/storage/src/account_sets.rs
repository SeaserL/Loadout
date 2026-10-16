// SPDX-License-Identifier: Apache-2.0

//! Account sub-pools (ExecPlan 019): named subsets of accounts. While one is active, only its
//! accounts are routable; no active set means the main pool (every account).

use rusqlite::{OptionalExtension, Transaction, params};
use uuid::Uuid;

use crate::{Database, StorageError, now_ms};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountSet {
    pub id: String,
    pub name: String,
    pub active: bool,
    /// Account ids, sorted.
    pub members: Vec<String>,
}

#[derive(Debug)]
pub struct AccountSetRepo<'a> {
    db: &'a Database,
}

fn members(tx: &rusqlite::Connection, set_id: &str) -> Result<Vec<String>, StorageError> {
    let mut stmt = tx.prepare(
        "SELECT account_id FROM account_set_members WHERE set_id = ?1 ORDER BY account_id",
    )?;
    let rows = stmt.query_map(params![set_id], |r| r.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn replace_members(
    tx: &Transaction<'_>,
    set_id: &str,
    account_ids: &[String],
) -> Result<(), StorageError> {
    tx.execute(
        "DELETE FROM account_set_members WHERE set_id = ?1",
        params![set_id],
    )?;
    for a in account_ids {
        tx.execute(
            "INSERT OR IGNORE INTO account_set_members (set_id, account_id) VALUES (?1, ?2)",
            params![set_id, a],
        )?;
    }
    Ok(())
}

impl<'a> AccountSetRepo<'a> {
    pub(crate) fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn list(&self) -> Result<Vec<AccountSet>, StorageError> {
        self.db.with_conn(|c| {
            let mut stmt = c.prepare("SELECT id, name, active FROM account_sets ORDER BY name")?;
            let mut sets: Vec<AccountSet> = stmt
                .query_map([], |r| {
                    Ok(AccountSet {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        active: r.get::<_, i64>(2)? == 1,
                        members: Vec::new(),
                    })
                })?
                .collect::<Result<_, _>>()?;
            for s in &mut sets {
                s.members = members(c, &s.id)?;
            }
            Ok(sets)
        })
    }

    pub fn get(&self, name: &str) -> Result<Option<AccountSet>, StorageError> {
        Ok(self.list()?.into_iter().find(|s| s.name == name))
    }

    /// The active set, if any (none = the main pool).
    pub fn active(&self) -> Result<Option<AccountSet>, StorageError> {
        Ok(self.list()?.into_iter().find(|s| s.active))
    }

    /// Creates a set, or replaces the members of an existing one. With `rename_from`, the set
    /// of that name is renamed to `name` first (keeping its active state).
    pub fn save(
        &self,
        name: &str,
        rename_from: Option<&str>,
        account_ids: &[String],
    ) -> Result<AccountSet, StorageError> {
        let now = now_ms();
        self.db.with_tx(|tx| {
            let lookup = rename_from.unwrap_or(name);
            let existing: Option<String> = tx
                .query_row(
                    "SELECT id FROM account_sets WHERE name = ?1",
                    params![lookup],
                    |r| r.get(0),
                )
                .optional()?;
            let id = match existing {
                Some(id) => {
                    tx.execute(
                        "UPDATE account_sets SET name = ?2, updated_at = ?3 WHERE id = ?1",
                        params![id, name, now],
                    )?;
                    id
                }
                None if rename_from.is_some() => {
                    return Err(StorageError::NotFound(format!("sub-pool {lookup}")));
                }
                None => {
                    let id = Uuid::new_v4().to_string();
                    tx.execute(
                        "INSERT INTO account_sets (id, name, active, created_at, updated_at)
                         VALUES (?1, ?2, 0, ?3, ?3)",
                        params![id, name, now],
                    )?;
                    id
                }
            };
            replace_members(tx, &id, account_ids)
        })?;
        self.get(name)?
            .ok_or_else(|| StorageError::NotFound(format!("sub-pool {name}")))
    }

    pub fn delete(&self, name: &str) -> Result<bool, StorageError> {
        self.db.with_conn(|c| {
            Ok(c.execute("DELETE FROM account_sets WHERE name = ?1", params![name])? == 1)
        })
    }

    /// Makes `name` the active set, or returns to the main pool with `None`. Returns `false`
    /// when no set has that name (nothing changes then).
    pub fn activate(&self, name: Option<&str>) -> Result<bool, StorageError> {
        self.db.with_tx(|tx| {
            if let Some(name) = name {
                let exists: Option<String> = tx
                    .query_row(
                        "SELECT id FROM account_sets WHERE name = ?1",
                        params![name],
                        |r| r.get(0),
                    )
                    .optional()?;
                let Some(id) = exists else {
                    return Ok(false);
                };
                tx.execute("UPDATE account_sets SET active = 0 WHERE active = 1", [])?;
                tx.execute(
                    "UPDATE account_sets SET active = 1 WHERE id = ?1",
                    params![id],
                )?;
            } else {
                tx.execute("UPDATE account_sets SET active = 0 WHERE active = 1", [])?;
            }
            Ok(true)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NewAccount, NewProvider};

    fn setup() -> (Database, Vec<String>) {
        let db = Database::open_in_memory().expect("db");
        let p = db
            .providers()
            .create(&NewProvider {
                slug: "p",
                kind: "openai-compatible",
                display_name: "P",
                base_url: Some("https://x/v1"),
                config_json: "{}",
            })
            .expect("provider");
        let ids = ["a", "b", "c"]
            .iter()
            .map(|alias| {
                db.accounts()
                    .create(&NewAccount {
                        provider_id: &p.id,
                        alias,
                        display_name: None,
                        account_type: "api_key",
                    })
                    .expect("account")
                    .id
            })
            .collect();
        (db, ids)
    }

    #[test]
    fn save_rename_activate_delete() {
        let (db, ids) = setup();
        let repo = db.account_sets();
        assert!(repo.active().expect("active").is_none());

        let s = repo
            .save("work", None, &[ids[0].clone(), ids[1].clone()])
            .expect("save");
        assert_eq!(s.members.len(), 2);
        assert!(!s.active);
        repo.save("home", None, &[ids[2].clone()]).expect("save");

        assert!(repo.activate(Some("work")).expect("activate"));
        assert_eq!(repo.active().expect("a").expect("set").name, "work");
        assert!(repo.activate(Some("home")).expect("activate"));
        let sets = repo.list().expect("list");
        assert_eq!(sets.iter().filter(|s| s.active).count(), 1);
        assert!(!repo.activate(Some("nope")).expect("activate"));
        assert_eq!(repo.active().expect("a").expect("set").name, "home");

        // Rename keeps the active state; members are replaced.
        let renamed = repo
            .save("house", Some("home"), &[ids[1].clone()])
            .expect("rename");
        assert!(renamed.active);
        assert_eq!(renamed.members, vec![ids[1].clone()]);
        assert!(repo.save("x", Some("missing"), &[]).is_err());

        // Deleting an account removes it from every set.
        db.accounts().delete(&ids[1]).expect("delete");
        assert!(
            repo.get("house")
                .expect("get")
                .expect("set")
                .members
                .is_empty()
        );

        assert!(repo.activate(None).expect("main"));
        assert!(repo.active().expect("a").is_none());
        assert!(repo.delete("work").expect("delete"));
        assert!(!repo.delete("work").expect("delete"));
    }

    #[test]
    fn names_are_unique() {
        let (db, _) = setup();
        db.account_sets().save("a", None, &[]).expect("save");
        db.account_sets().save("b", None, &[]).expect("save");
        assert!(db.account_sets().save("b", Some("a"), &[]).is_err());
    }
}
