// SPDX-License-Identifier: Apache-2.0

use rusqlite::params;

use crate::account_models::{AccountModel, map_account_model};
use crate::providers::{Provider, map_provider};
use crate::{Database, StorageError};

/// One enabled model of one enabled account, with what is needed to route to it.
#[derive(Clone, Debug, PartialEq)]
pub struct RoutableModel {
    pub provider: Provider,
    pub account_id: String,
    pub account_alias: String,
    pub account_position: i64,
    pub account_type: String,
    pub model: AccountModel,
    /// API key reference, if the account has one.
    pub secret_ref: Option<String>,
}

impl Database {
    /// Everything the route table is built from, in deterministic order
    /// (provider slug, account position, model id).
    pub fn routable_models(&self) -> Result<Vec<RoutableModel>, StorageError> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT p.id, p.slug, p.kind, p.display_name, p.base_url, p.config_json,
                        p.created_at, p.updated_at,
                        a.id, a.alias, a.position, a.account_type,
                        (SELECT secret_ref FROM credentials
                          WHERE account_id = a.id AND credential_type = 'api_key'
                          ORDER BY created_at DESC LIMIT 1),
                        {}
                 FROM account_models m
                 JOIN accounts a ON a.id = m.account_id
                 JOIN providers p ON p.id = a.provider_id
                 WHERE a.enabled = 1 AND m.enabled = 1
                 ORDER BY p.slug, a.position, a.alias, m.upstream_model_id",
                crate::account_models::COLUMNS
                    .split(", ")
                    .map(|col| format!("m.{}", col.trim()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))?;
            let rows = stmt.query_map(params![], |r| {
                Ok(RoutableModel {
                    provider: map_provider(r)?,
                    account_id: r.get(8)?,
                    account_alias: r.get(9)?,
                    account_position: r.get(10)?,
                    account_type: r.get(11)?,
                    secret_ref: r.get(12)?,
                    model: map_account_model(r, 13)?,
                })
            })?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }
}
