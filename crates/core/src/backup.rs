// SPDX-License-Identifier: Apache-2.0

//! Configuration export / import and database backup (ExecPlan 006 §3.4).
//!
//! The export never contains secrets: API keys stay in Credential Manager (ADR-004) and
//! client keys are exported only as their SHA-256 hash, which cannot be reversed but lets
//! existing keys keep working after an import on another machine.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use loadout_ipc::api::{BackupResult, ClientPolicy, ImportReport};
use loadout_storage::{Database, ModelInput, NewAccount, NewClient, NewProvider, StorageError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::clients::{parse_policy, policy_json};

const FORMAT: &str = "loadout-config";
const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct ConfigFile {
    format: String,
    version: u32,
    exported_at_ms: i64,
    loadout_version: String,
    #[serde(default)]
    settings: Vec<SettingEntry>,
    #[serde(default)]
    providers: Vec<ProviderEntry>,
    #[serde(default)]
    pools: Vec<PoolEntry>,
    #[serde(default)]
    clients: Vec<ClientEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SettingEntry {
    key: String,
    value: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ProviderEntry {
    slug: String,
    kind: String,
    display_name: String,
    base_url: Option<String>,
    #[serde(default)]
    settings: Value,
    #[serde(default)]
    accounts: Vec<AccountEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AccountEntry {
    alias: String,
    display_name: Option<String>,
    account_type: String,
    enabled: bool,
    plan_name: Option<String>,
    price_monthly_usd: Option<f64>,
    renews_at_ms: Option<i64>,
    /// Source of an externally managed login (official CLI), e.g. `codex-cli`.
    external_credential: Option<String>,
    /// The account had an API key (it must be entered again after import).
    had_api_key: bool,
    #[serde(default)]
    models: Vec<ModelEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ModelEntry {
    upstream_model_id: String,
    protocols: Value,
    context_window: Option<i64>,
    max_output_tokens: Option<i64>,
    capabilities: Value,
    input_price: Option<f64>,
    output_price: Option<f64>,
    cache_read_price: Option<f64>,
    cache_write_price: Option<f64>,
    enabled: bool,
    source: String,
    tier_override: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PoolEntry {
    name: String,
    ordering: String,
    /// `provider/account/model`.
    members: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ClientEntry {
    name: String,
    key_prefix: String,
    /// Hex SHA-256 of the key.
    key_hash: String,
    #[serde(default)]
    policy: ClientPolicy,
    revoked_at_ms: Option<i64>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        out[i] = u8::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()?;
    }
    Some(out)
}

pub(crate) fn backups_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("backups")
}

fn target_path(data_dir: &Path, path: Option<String>, name: &str) -> Result<PathBuf, String> {
    match path.filter(|p| !p.trim().is_empty()) {
        Some(p) => Ok(PathBuf::from(p.trim())),
        None => {
            let dir = backups_dir(data_dir);
            std::fs::create_dir_all(&dir)
                .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
            Ok(dir.join(name))
        }
    }
}

fn read_config(db: &Database) -> Result<ConfigFile, StorageError> {
    let mut providers = Vec::new();
    for p in db.providers().list()? {
        let mut accounts = Vec::new();
        for a in db.accounts().list(Some(&p.id))? {
            let creds = db.accounts().credentials(&a.id)?;
            let models = db
                .account_models()
                .list(&a.id)?
                .into_iter()
                .map(|m| ModelEntry {
                    upstream_model_id: m.upstream_model_id,
                    protocols: serde_json::from_str(&m.protocols_json).unwrap_or(Value::Null),
                    context_window: m.context_window,
                    max_output_tokens: m.max_output_tokens,
                    capabilities: serde_json::from_str(&m.capabilities_json).unwrap_or(Value::Null),
                    input_price: m.input_price,
                    output_price: m.output_price,
                    cache_read_price: m.cache_read_price,
                    cache_write_price: m.cache_write_price,
                    enabled: m.enabled,
                    source: m.source,
                    tier_override: m.tier_override,
                })
                .collect();
            accounts.push(AccountEntry {
                alias: a.alias,
                display_name: a.display_name,
                account_type: a.account_type,
                enabled: a.enabled,
                plan_name: a.subscription_plan,
                price_monthly_usd: a.price_monthly_usd,
                renews_at_ms: a.renew_at,
                external_credential: creds
                    .iter()
                    .find(|c| c.credential_type == "external_managed")
                    .map(|c| c.source.clone()),
                had_api_key: creds.iter().any(|c| c.credential_type == "api_key"),
                models,
            });
        }
        providers.push(ProviderEntry {
            slug: p.slug,
            kind: p.kind,
            display_name: p.display_name,
            base_url: p.base_url,
            settings: serde_json::from_str(&p.config_json).unwrap_or(Value::Null),
            accounts,
        });
    }
    let names = db.model_names()?;
    let pools = db
        .pools()?
        .into_iter()
        .map(|p| PoolEntry {
            name: p.name,
            ordering: p.ordering,
            members: p
                .members
                .iter()
                .filter_map(|id| names.get(id).cloned())
                .collect(),
        })
        .collect();
    let mut clients = Vec::new();
    for c in db.clients().list()? {
        let Some(hash) = db.clients().key_hash(&c.name)? else {
            continue;
        };
        clients.push(ClientEntry {
            policy: parse_policy(&c.policy_json),
            name: c.name,
            key_prefix: c.key_prefix,
            key_hash: hex(&hash),
            revoked_at_ms: c.revoked_at,
        });
    }
    Ok(ConfigFile {
        format: FORMAT.to_owned(),
        version: FORMAT_VERSION,
        exported_at_ms: loadout_storage::now_ms(),
        loadout_version: crate::VERSION.to_owned(),
        settings: db
            .settings()
            .all()?
            .into_iter()
            .filter(|(k, _)| crate::settings::find(k).is_some())
            .map(|(key, value)| SettingEntry { key, value })
            .collect(),
        providers,
        pools,
        clients,
    })
}

/// Writes the configuration to `path` (or the backups folder) and returns where.
pub(crate) fn export(
    db: &Database,
    data_dir: &Path,
    path: Option<String>,
) -> Result<BackupResult, String> {
    let config = read_config(db).map_err(|e| e.to_string())?;
    let now = loadout_storage::now_ms();
    let target = target_path(
        data_dir,
        path,
        &format!(
            "loadout-config-{}.json",
            loadout_provider_sdk::time::utc_stamp(now)
        ),
    )?;
    let json = serde_json::to_vec_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(&target, json).map_err(|e| format!("cannot write {}: {e}", target.display()))?;
    let accounts = config.providers.iter().map(|p| p.accounts.len()).sum();
    let models = config
        .providers
        .iter()
        .flat_map(|p| &p.accounts)
        .map(|a| a.models.len())
        .sum();
    Ok(BackupResult {
        path: target.display().to_string(),
        providers: config.providers.len(),
        accounts,
        models,
        pools: config.pools.len(),
        clients: config.clients.len(),
    })
}

/// Automatic backups kept (ExecPlan 007 §2).
const AUTO_KEEP: usize = 7;

pub(crate) fn auto_dir(data_dir: &Path) -> PathBuf {
    backups_dir(data_dir).join("auto")
}

/// Writes a daily configuration backup and keeps the newest [`AUTO_KEEP`]. An empty
/// configuration (fresh install, or right after a database recovery) is not written, so it
/// never pushes good backups out.
pub(crate) fn auto_export(db: &Database, data_dir: &Path) -> Result<Option<String>, String> {
    let empty = db.providers().list().map_err(|e| e.to_string())?.is_empty()
        && db.clients().list().map_err(|e| e.to_string())?.is_empty();
    if empty {
        return Ok(None);
    }
    let dir = auto_dir(data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let name = format!(
        "loadout-config-{}.json",
        loadout_provider_sdk::time::utc_stamp(loadout_storage::now_ms())
    );
    let result = export(db, data_dir, dir.join(name).to_str().map(str::to_owned))?;
    prune(&dir, AUTO_KEEP);
    Ok(Some(result.path))
}

/// Deletes all but the newest `keep` automatic backups (names sort by time).
fn prune(dir: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("loadout-config-") && n.ends_with(".json"))
        })
        .collect();
    files.sort();
    let excess = files.len().saturating_sub(keep);
    for old in &files[..excess] {
        if let Err(e) = std::fs::remove_file(old) {
            tracing::warn!(file = %old.display(), error = %e, "could not delete old backup");
        }
    }
}

/// Copies the whole database (request history included; no secrets).
pub(crate) fn backup_database(
    db: &Database,
    data_dir: &Path,
    path: Option<String>,
) -> Result<String, String> {
    let target = target_path(
        data_dir,
        path,
        &format!(
            "loadout-{}.db",
            loadout_provider_sdk::time::utc_stamp(loadout_storage::now_ms())
        ),
    )?;
    if target.exists() {
        return Err(format!("{} already exists", target.display()));
    }
    db.backup_to(&target).map_err(|e| e.to_string())?;
    Ok(target.display().to_string())
}

/// Merges a configuration file: creates what is missing, skips what exists, applies
/// settings. `validate_setting` checks a setting value.
pub(crate) fn import(db: &Database, path: &str) -> Result<ImportReport, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let config: ConfigFile = serde_json::from_slice(&bytes)
        .map_err(|e| format!("{path} is not a Loadout configuration: {e}"))?;
    if config.format != FORMAT || config.version > FORMAT_VERSION {
        return Err(format!(
            "unsupported file (format {} version {}); expected {FORMAT} version {FORMAT_VERSION} or older",
            config.format, config.version
        ));
    }
    let mut report = ImportReport::default();
    let s = |e: StorageError| e.to_string();

    for entry in &config.settings {
        match crate::settings::find(&entry.key) {
            Some(def) if def.validate(&entry.value).is_ok() => {
                db.settings().set(def.key, &entry.value).map_err(s)?;
                report.settings += 1;
            }
            _ => report.skipped.push(format!("setting {}", entry.key)),
        }
    }

    for p in &config.providers {
        let provider = match db.providers().get_by_slug(&p.slug).map_err(s)? {
            Some(existing) if existing.kind == p.kind => existing,
            Some(existing) => {
                report.skipped.push(format!(
                    "provider {} (already used by a {} provider)",
                    p.slug, existing.kind
                ));
                continue;
            }
            None => {
                let settings = if p.settings.is_object() {
                    p.settings.to_string()
                } else {
                    "{}".to_owned()
                };
                report.providers += 1;
                db.providers()
                    .create(&NewProvider {
                        slug: &p.slug,
                        kind: &p.kind,
                        display_name: &p.display_name,
                        base_url: p.base_url.as_deref(),
                        config_json: &settings,
                    })
                    .map_err(s)?
            }
        };
        for a in &p.accounts {
            if db
                .accounts()
                .get(&provider.id, &a.alias)
                .map_err(s)?
                .is_some()
            {
                report
                    .skipped
                    .push(format!("account {}/{}", p.slug, a.alias));
                continue;
            }
            let account = db
                .accounts()
                .create(&NewAccount {
                    provider_id: &provider.id,
                    alias: &a.alias,
                    display_name: a.display_name.as_deref(),
                    account_type: &a.account_type,
                })
                .map_err(s)?;
            report.accounts += 1;
            if !a.enabled {
                db.accounts().set_enabled(&account.id, false).map_err(s)?;
            }
            if a.plan_name.is_some() || a.price_monthly_usd.is_some() || a.renews_at_ms.is_some() {
                db.accounts()
                    .set_plan(
                        &account.id,
                        a.plan_name.as_deref(),
                        a.price_monthly_usd,
                        a.renews_at_ms,
                    )
                    .map_err(s)?;
            }
            if let Some(source) = &a.external_credential {
                db.accounts()
                    .add_credential(&account.id, "external_managed", source, None)
                    .map_err(s)?;
            }
            if a.had_api_key {
                report
                    .accounts_needing_keys
                    .push(format!("{}/{}", p.slug, a.alias));
            }
            for m in &a.models {
                let model = db
                    .account_models()
                    .insert(
                        &account.id,
                        &ModelInput {
                            upstream_model_id: m.upstream_model_id.clone(),
                            protocols_json: m.protocols.to_string(),
                            context_window: m.context_window,
                            max_output_tokens: m.max_output_tokens,
                            capabilities_json: if m.capabilities.is_null() {
                                "[]".into()
                            } else {
                                m.capabilities.to_string()
                            },
                            input_price: m.input_price,
                            output_price: m.output_price,
                            cache_read_price: m.cache_read_price,
                            cache_write_price: m.cache_write_price,
                        },
                        m.enabled,
                        &m.source,
                    )
                    .map_err(s)?;
                if let Some(tier) = &m.tier_override {
                    db.set_tier_override(&model.id, Some(tier)).map_err(s)?;
                }
                report.models += 1;
            }
        }
    }

    let ids: HashMap<String, String> = db
        .model_names()
        .map_err(s)?
        .into_iter()
        .map(|(id, name)| (name, id))
        .collect();
    for pool in &config.pools {
        if db.pool(&pool.name).map_err(s)?.is_some() {
            report.skipped.push(format!("pool {}", pool.name));
            continue;
        }
        let members: Vec<String> = pool
            .members
            .iter()
            .filter_map(|m| ids.get(m).cloned())
            .collect();
        if members.is_empty() {
            report
                .skipped
                .push(format!("pool {} (no matching models)", pool.name));
            continue;
        }
        db.save_pool(&pool.name, &pool.ordering, &members)
            .map_err(s)?;
        report.pools += 1;
    }

    for c in &config.clients {
        let Some(hash) = unhex(&c.key_hash) else {
            report
                .skipped
                .push(format!("client {} (bad key hash)", c.name));
            continue;
        };
        match db.clients().create(&NewClient {
            name: &c.name,
            key_prefix: &c.key_prefix,
            key_hash: hash,
        }) {
            Ok(_) => {
                db.clients()
                    .set_policy(&c.name, &policy_json(&c.policy))
                    .map_err(s)?;
                if let Some(at) = c.revoked_at_ms {
                    db.clients().set_revoked(&c.name, at).map_err(s)?;
                }
                report.clients += 1;
            }
            Err(StorageError::Conflict(_)) => report.skipped.push(format!("client {}", c.name)),
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_backups_skip_empty_configs_and_rotate() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = Database::open_in_memory().expect("db");
        assert_eq!(
            auto_export(&db, dir.path()).expect("auto"),
            None,
            "empty config"
        );

        db.providers()
            .create(&NewProvider {
                slug: "p",
                kind: "openai-compatible",
                display_name: "P",
                base_url: Some("https://x.test/v1"),
                config_json: "{}",
            })
            .expect("provider");
        let auto = auto_dir(dir.path());
        std::fs::create_dir_all(&auto).expect("mkdir");
        for i in 0..9 {
            std::fs::write(
                auto.join(format!("loadout-config-2000010{i}-000000.json")),
                "{}",
            )
            .expect("old");
        }
        std::fs::write(auto.join("keep-me.txt"), "x").expect("other");
        let written = auto_export(&db, dir.path())
            .expect("auto")
            .expect("written");
        let names: Vec<String> = std::fs::read_dir(&auto)
            .expect("read")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names.iter().filter(|n| n.ends_with(".json")).count(),
            AUTO_KEEP
        );
        assert!(names.contains(&"keep-me.txt".to_owned()));
        assert!(std::path::Path::new(&written).is_file(), "newest kept");
        assert!(!names.contains(&"loadout-config-20000100-000000.json".to_owned()));
    }

    #[test]
    fn hex_round_trip() {
        let h = [0xab; 32];
        assert_eq!(unhex(&hex(&h)), Some(h));
        assert_eq!(unhex("zz"), None);
    }
}
