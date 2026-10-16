// SPDX-License-Identifier: Apache-2.0

//! Account sub-pools (ExecPlan 019): named subsets of accounts the user switches between,
//! like switching accounts. Routing honours the active one when the route table is built
//! (`route_build`); the request path is unchanged.

use std::collections::HashMap;

use loadout_ipc::ErrorBody;
use loadout_ipc::api::{SubpoolInfo, SubpoolState};
use loadout_storage::{AccountSet, Database};

use crate::control::{ControlPlane, blocking, storage};
use crate::settings;

/// Names that mean the main pool and so cannot name a sub-pool.
const MAIN_NAMES: &[&str] = &["main", "主号池"];

fn validate_name(name: &str) -> Result<String, ErrorBody> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 40 || name.chars().any(char::is_control) {
        return Err(ErrorBody::invalid_params(
            "a sub-pool name needs 1-40 characters",
        ));
    }
    if MAIN_NAMES.iter().any(|m| m.eq_ignore_ascii_case(name)) {
        return Err(ErrorBody::invalid_params(format!(
            "{name:?} is the main pool; choose another name"
        )));
    }
    Ok(name.to_owned())
}

/// `account id -> provider/alias` for every account.
fn account_names(db: &Database) -> Result<HashMap<String, String>, ErrorBody> {
    let mut out = HashMap::new();
    for p in db.providers().list().map_err(storage)? {
        for a in db.accounts().list(Some(&p.id)).map_err(storage)? {
            out.insert(a.id, format!("{}/{}", p.slug, a.alias));
        }
    }
    Ok(out)
}

fn info(set: AccountSet, names: &HashMap<String, String>) -> SubpoolInfo {
    let mut accounts: Vec<String> = set
        .members
        .iter()
        .filter_map(|id| names.get(id).cloned())
        .collect();
    accounts.sort();
    SubpoolInfo {
        name: set.name,
        active: set.active,
        accounts,
    }
}

fn state(db: &Database) -> Result<SubpoolState, ErrorBody> {
    let names = account_names(db)?;
    let sets = db.account_sets().list().map_err(storage)?;
    let remember = settings::value(db, settings::SUBPOOL_REMEMBER)
        .ok()
        .and_then(|v| settings::parse_bool(&v))
        .unwrap_or(false);
    Ok(SubpoolState {
        active: sets.iter().find(|s| s.active).map(|s| s.name.clone()),
        remember,
        subpools: sets.into_iter().map(|s| info(s, &names)).collect(),
    })
}

impl ControlPlane {
    pub async fn list_subpools(&self) -> Result<SubpoolState, ErrorBody> {
        let db = self.db.clone();
        blocking(move || state(&db)).await
    }

    pub async fn save_subpool(
        &self,
        name: String,
        rename_from: Option<String>,
        accounts: Vec<String>,
    ) -> Result<SubpoolInfo, ErrorBody> {
        let name = validate_name(&name)?;
        let db = self.db.clone();
        let (info, active) = blocking(move || {
            let mut ids = Vec::new();
            for full in &accounts {
                let (slug, alias) = full.split_once('/').ok_or_else(|| {
                    ErrorBody::invalid_params(format!("{full:?} is not provider/alias"))
                })?;
                let p = Self::load_provider(&db, slug)?;
                ids.push(Self::load_account(&db, &p, alias)?.id);
            }
            let renaming = rename_from.as_deref().is_some_and(|old| old != name);
            if renaming && db.account_sets().get(&name).map_err(storage)?.is_some() {
                return Err(ErrorBody::invalid_params(format!(
                    "a sub-pool named {name:?} already exists"
                )));
            }
            let set = db
                .account_sets()
                .save(&name, rename_from.as_deref(), &ids)
                .map_err(storage)?;
            let active = set.active;
            Ok((info(set, &account_names(&db)?), active))
        })
        .await?;
        if active {
            self.rebuild_routes().await?;
        }
        Ok(info)
    }

    pub async fn remove_subpool(&self, name: String) -> Result<bool, ErrorBody> {
        let db = self.db.clone();
        let (removed, was_active) = blocking(move || {
            let was_active = db
                .account_sets()
                .get(&name)
                .map_err(storage)?
                .is_some_and(|s| s.active);
            Ok((
                db.account_sets().delete(&name).map_err(storage)?,
                was_active,
            ))
        })
        .await?;
        if was_active {
            tracing::info!("active sub-pool removed; back to the main pool");
            self.rebuild_routes().await?;
        }
        Ok(removed)
    }

    /// Switches to a sub-pool, or back to the main pool with `None`.
    pub async fn activate_subpool(&self, name: Option<String>) -> Result<SubpoolState, ErrorBody> {
        let db = self.db.clone();
        let target = name.clone();
        blocking(move || {
            if db
                .account_sets()
                .activate(target.as_deref())
                .map_err(storage)?
            {
                Ok(())
            } else {
                Err(ErrorBody::not_found(format!(
                    "sub-pool {:?} not found",
                    target.unwrap_or_default()
                )))
            }
        })
        .await?;
        tracing::info!(
            subpool = name.as_deref().unwrap_or("main"),
            "switched account pool"
        );
        self.rebuild_routes().await?;
        self.list_subpools().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(validate_name("  工作 ").expect("ok"), "工作");
        assert!(validate_name("").is_err());
        assert!(validate_name("Main").is_err());
        assert!(validate_name("主号池").is_err());
        assert!(validate_name(&"x".repeat(41)).is_err());
        assert!(validate_name("a\nb").is_err());
    }
}
