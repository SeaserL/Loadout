// SPDX-License-Identifier: Apache-2.0

//! Local agent scan and import (Harness §12, ExecPlan 003 §4.3). A scan only inspects the
//! machine when the user asks; importing creates an account whose credentials stay managed
//! by the official CLI (`external_managed`, ADR-004 §2).

use loadout_ipc::ErrorBody;
use loadout_ipc::api::{AccountInfo, DiscoveredTool};
use loadout_storage::{NewAccount, NewProvider};
use serde_json::Value;

use crate::control::{ControlPlane, blocking, storage};

impl ControlPlane {
    pub async fn scan(&self) -> Result<Vec<DiscoveredTool>, ErrorBody> {
        let (db, registry, home) = (self.db.clone(), self.registry.clone(), self.home.clone());
        blocking(move || {
            let mut out = Vec::new();
            for m in registry.modules() {
                let d = m.descriptor();
                if !d.drivers.discovery {
                    continue;
                }
                let Some(found) = m.discover(&home) else {
                    continue;
                };
                let imported = match d.fixed_slug {
                    Some(slug) => match db.providers().get_by_slug(slug).map_err(storage)? {
                        Some(p) => db
                            .accounts()
                            .list(Some(&p.id))
                            .map_err(storage)?
                            .iter()
                            .any(|a| {
                                db.accounts().credentials(&a.id).is_ok_and(|c| {
                                    c.iter().any(|c| c.credential_type == "external_managed")
                                })
                            }),
                        None => false,
                    },
                    None => false,
                };
                out.push(DiscoveredTool {
                    kind: d.kind.to_owned(),
                    display_name: d.display_name.to_owned(),
                    installed: found.installed,
                    executable: found.executable.map(|p| p.display().to_string()),
                    logged_in: found.logged_in,
                    auth: found.auth,
                    detail: found.detail,
                    expired: found.expired,
                    config_path: found.config_path.map(|p| p.display().to_string()),
                    imported,
                    inference: !d.transports.is_empty(),
                });
            }
            Ok(out)
        })
        .await
    }

    /// Imports a discovered, logged-in tool as an account (and its provider if needed).
    pub async fn import(
        &self,
        kind: String,
        alias: Option<String>,
    ) -> Result<AccountInfo, ErrorBody> {
        let module = self
            .registry
            .get(&kind)
            .ok_or_else(|| ErrorBody::invalid_params(format!("unknown kind {kind:?}")))?
            .clone();
        let d = module.descriptor();
        let slug = d
            .fixed_slug
            .ok_or_else(|| ErrorBody::invalid_params(format!("{kind} cannot be imported")))?
            .to_owned();
        let home = self.home.clone();
        let found = {
            let module = module.clone();
            blocking(move || Ok(module.discover(&home))).await?
        };
        match found {
            Some(f) if f.logged_in => {}
            Some(_) => {
                return Err(ErrorBody::invalid_params(format!(
                    "{} is installed but not logged in; log in with the official CLI first",
                    d.display_name
                )));
            }
            None => {
                return Err(ErrorBody::not_found(format!(
                    "{} was not found on this machine",
                    d.display_name
                )));
            }
        }
        // Providers that also accept API keys get a distinct default alias for the CLI login.
        let alias = alias.unwrap_or_else(|| {
            if d.transports.is_empty() {
                "main"
            } else {
                "cli"
            }
            .to_owned()
        });
        loadout_router::validate_alias(&alias, std::iter::empty())
            .map_err(|e| ErrorBody::invalid_params(e.to_string()))?;
        let (db, display) = (self.db.clone(), d.display_name.to_owned());
        let (kind_c, slug_c, alias_c) = (kind.clone(), slug.clone(), alias.clone());
        blocking(move || {
            let provider = match db.providers().get_by_slug(&slug_c).map_err(storage)? {
                Some(p) if p.kind == kind_c => p,
                Some(p) => {
                    return Err(ErrorBody::conflict(format!(
                        "the slug {slug_c} is used by a {} provider",
                        p.kind
                    )));
                }
                None => db
                    .providers()
                    .create(&NewProvider {
                        slug: &slug_c,
                        kind: &kind_c,
                        display_name: &display,
                        base_url: None,
                        config_json: &Value::Object(Default::default()).to_string(),
                    })
                    .map_err(storage)?,
            };
            let account = db
                .accounts()
                .create(&NewAccount {
                    provider_id: &provider.id,
                    alias: &alias_c,
                    display_name: None,
                    account_type: "subscription",
                })
                .map_err(storage)?;
            db.accounts()
                .add_credential(
                    &account.id,
                    "external_managed",
                    &format!("{kind_c}-cli"),
                    None,
                )
                .map_err(storage)?;
            Ok(())
        })
        .await?;
        self.rebuild_routes().await?;
        self.account(&slug, &alias).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use loadout_providers::Registry;
    use loadout_storage::Database;

    use super::*;

    fn control(home: &std::path::Path) -> ControlPlane {
        ControlPlane::new(
            Arc::new(Database::open_in_memory().expect("db")),
            Arc::new(loadout_secrets::InMemorySecretStore::new()),
            Registry::builtin(),
            Arc::default(),
            loadout_provider_sdk::HttpClient::new(&Default::default()).expect("http"),
            home.to_path_buf(),
        )
    }

    #[tokio::test]
    async fn scan_and_import_kimi_cli_login() {
        let home = tempfile::tempdir().expect("tempdir");
        let creds = home.path().join(".kimi-code").join("credentials");
        std::fs::create_dir_all(&creds).expect("mkdir");
        std::fs::write(creds.join("kimi-code.json"), r#"{"access_token":"tok"}"#).expect("write");
        let c = control(home.path());

        let found = c.scan().await.expect("scan");
        let kimi = found
            .iter()
            .find(|t| t.kind == "kimi-code")
            .unwrap_or_else(|| panic!("kimi not in {found:?}"));
        assert!(kimi.logged_in && !kimi.imported && kimi.inference);
        assert!(
            !format!("{found:?}").contains("tok\""),
            "no token in scan output"
        );

        let account = c.import("kimi-code".into(), None).await.expect("import");
        assert_eq!(
            (account.provider.as_str(), account.alias.as_str()),
            ("kimi-code", "cli")
        );
        assert_eq!(account.account_type, "subscription");
        assert!(!account.has_key);
        let found = c.scan().await.expect("scan");
        assert!(
            found
                .iter()
                .find(|t| t.kind == "kimi-code")
                .expect("kimi")
                .imported
        );
        assert!(
            c.import("kimi-code".into(), None).await.is_err(),
            "alias already used"
        );
    }

    #[tokio::test]
    async fn import_requires_a_login() {
        let home = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(home.path().join(".gemini")).expect("mkdir");
        let c = control(home.path());
        let err = c
            .import("gemini-cli".into(), None)
            .await
            .expect_err("not logged in");
        assert!(err.message.contains("not logged in"), "{}", err.message);
        assert!(c.import("openai-compatible".into(), None).await.is_err());
    }
}
