// SPDX-License-Identifier: Apache-2.0

//! Control-plane operations on providers, accounts and models (ExecPlan 002 §5.6). Every
//! change rebuilds and republishes the route table.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use loadout_ipc::ErrorBody;
use loadout_ipc::api::{
    AccountInfo, ModelInfo, ProviderInfo, ProviderKind, RequestSummary, SyncResult, TargetStatus,
};
use loadout_protocol::Protocol;
use loadout_provider_sdk::{AuthMethod, CatalogModel, DriverContext, HttpClient, ProviderError};
use loadout_providers::Registry;
use loadout_router::{
    HealthRegistry, LatencyBook, RouteTableHandle, now_ms, validate_alias, validate_slug,
};
use loadout_secrets::{Secret, SecretRef, SecretStore};
use loadout_storage::{
    Account, AccountModel, Database, ModelInput, NewAccount, NewProvider, Provider, StorageError,
};
use serde_json::Value;

use crate::route_build::{self, parse_protocols, protocols_json, provider_config, read_secret};

pub(crate) struct ControlPlane {
    pub db: Arc<Database>,
    pub secrets: Arc<dyn SecretStore>,
    pub registry: Registry,
    pub health: Arc<HealthRegistry>,
    pub routes: Arc<RouteTableHandle>,
    pub http: HttpClient,
    /// Home directory whose official-CLI state is read (overridable for tests).
    pub home: std::path::PathBuf,
    /// Latency statistics per account model (ExecPlan 004).
    pub latency: Arc<LatencyBook>,
    /// Last automatic probe per account model (ms).
    pub last_probe: std::sync::Mutex<HashMap<String, i64>>,
    /// Client policies and usage counters shared with the gateway (ExecPlan 006).
    pub policies: Arc<loadout_gateway::ClientPolicies>,
    version: AtomicU64,
    rebuild: tokio::sync::Mutex<()>,
}

impl std::fmt::Debug for ControlPlane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlPlane")
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

pub(crate) fn storage(e: StorageError) -> ErrorBody {
    match e {
        StorageError::Conflict(what) => ErrorBody::conflict(format!("{what} already exists")),
        StorageError::NotFound(what) => ErrorBody::not_found(format!("{what} not found")),
        other => {
            tracing::error!(error = %other, "database operation failed");
            ErrorBody::internal("database error; see the daemon log")
        }
    }
}

fn provider_error(e: ProviderError) -> ErrorBody {
    match e {
        ProviderError::Config(m) => ErrorBody::invalid_params(m),
        ProviderError::MissingKey => ErrorBody::invalid_params("an API key is required"),
        other => ErrorBody::internal(other.to_string()),
    }
}

/// Runs blocking work (SQLite, Credential Manager) on the blocking pool.
pub(crate) async fn blocking<T, F>(f: F) -> Result<T, ErrorBody>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ErrorBody> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| ErrorBody::internal("background task failed"))?
}

fn auth_name(a: AuthMethod) -> &'static str {
    match a {
        AuthMethod::ApiKey => "api_key",
        AuthMethod::OptionalApiKey => "optional_api_key",
        AuthMethod::ExternalManaged => "external_managed",
    }
}

/// Splits `provider/account/model` (model ids may contain `/`).
pub(crate) fn split_exact(name: &str) -> Result<(&str, &str, &str), ErrorBody> {
    let mut parts = name.splitn(3, '/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(p), Some(a), Some(m)) if !p.is_empty() && !a.is_empty() && !m.is_empty() => {
            Ok((p, a, m))
        }
        _ => Err(ErrorBody::invalid_params(format!(
            "expected provider/account/model, got {name:?}"
        ))),
    }
}

impl ControlPlane {
    pub fn new(
        db: Arc<Database>,
        secrets: Arc<dyn SecretStore>,
        registry: Registry,
        routes: Arc<RouteTableHandle>,
        http: HttpClient,
        home: std::path::PathBuf,
    ) -> Self {
        Self {
            db,
            secrets,
            registry,
            health: Arc::default(),
            routes,
            http,
            home,
            latency: Arc::default(),
            last_probe: std::sync::Mutex::default(),
            policies: Arc::default(),
            version: AtomicU64::new(0),
            rebuild: tokio::sync::Mutex::new(()),
        }
    }

    /// Rebuilds and publishes the route table. Returns build warnings.
    pub async fn rebuild_routes(&self) -> Result<Vec<String>, ErrorBody> {
        let _serial = self.rebuild.lock().await;
        let version = self.version.fetch_add(1, Ordering::AcqRel) + 1;
        let (db, secrets, registry, health, latency) = (
            self.db.clone(),
            self.secrets.clone(),
            self.registry.clone(),
            self.health.clone(),
            self.latency.clone(),
        );
        let (table, report) = blocking(move || {
            route_build::build(&db, secrets.as_ref(), &registry, &health, &latency, version)
                .map_err(storage)
        })
        .await?;
        for w in &report.warnings {
            tracing::warn!("{w}");
        }
        tracing::info!(version, targets = report.targets, "route table rebuilt");
        self.routes.store(table);
        Ok(report.warnings)
    }

    // ---- providers ----------------------------------------------------------------------

    pub fn kinds(&self) -> Vec<ProviderKind> {
        self.registry
            .modules()
            .map(|m| {
                let d = m.descriptor();
                let drivers = [
                    ("discovery", d.drivers.discovery),
                    ("auth", d.drivers.auth),
                    ("usage", d.drivers.usage),
                    ("catalog", d.drivers.catalog),
                    ("transport", d.drivers.transport),
                    ("health", d.drivers.health),
                ];
                ProviderKind {
                    kind: d.kind.to_owned(),
                    display_name: d.display_name.to_owned(),
                    fixed_slug: d.fixed_slug.map(str::to_owned),
                    default_base_url: d.default_base_url.map(str::to_owned),
                    auth: auth_name(d.auth).to_owned(),
                    protocols: d
                        .transports
                        .iter()
                        .map(|t| t.protocol.as_str().to_owned())
                        .collect(),
                    drivers: drivers
                        .iter()
                        .filter(|(_, on)| *on)
                        .map(|(n, _)| (*n).to_owned())
                        .collect(),
                }
            })
            .collect()
    }

    fn provider_info(&self, p: &Provider, accounts: usize) -> ProviderInfo {
        ProviderInfo {
            slug: p.slug.clone(),
            kind: p.kind.clone(),
            display_name: p.display_name.clone(),
            base_url: p.base_url.clone(),
            settings: serde_json::from_str(&p.config_json).unwrap_or(Value::Null),
            accounts,
        }
    }

    pub async fn add_provider(
        &self,
        slug: String,
        kind: String,
        base_url: Option<String>,
        settings: Value,
        display_name: Option<String>,
    ) -> Result<ProviderInfo, ErrorBody> {
        validate_slug(&slug).map_err(|e| ErrorBody::invalid_params(e.to_string()))?;
        let module = self.registry.get(&kind).ok_or_else(|| {
            let kinds: Vec<_> = self
                .registry
                .modules()
                .map(|m| m.descriptor().kind)
                .collect();
            ErrorBody::invalid_params(format!(
                "unknown provider kind {kind:?}; available: {}",
                kinds.join(", ")
            ))
        })?;
        let d = module.descriptor();
        if let Some(fixed) = d.fixed_slug
            && fixed != slug
        {
            return Err(ErrorBody::invalid_params(format!(
                "{kind} providers must use the slug {fixed:?}"
            )));
        }
        let settings = if settings.is_null() {
            Value::Object(Default::default())
        } else {
            settings
        };
        if !settings.is_object() {
            return Err(ErrorBody::invalid_params("settings must be a JSON object"));
        }
        let base_url = base_url.map(|u| u.trim().trim_end_matches('/').to_owned());
        let config = loadout_provider_sdk::ProviderConfig {
            slug: slug.clone(),
            kind: kind.clone(),
            base_url: base_url.clone(),
            settings: settings.clone(),
        };
        module.validate(&config).map_err(provider_error)?;
        let display = display_name.unwrap_or_else(|| {
            if d.fixed_slug.is_some() {
                d.display_name.to_owned()
            } else {
                slug.clone()
            }
        });
        let db = self.db.clone();
        let config_json = settings.to_string();
        let p = blocking(move || {
            db.providers()
                .create(&NewProvider {
                    slug: &slug,
                    kind: &kind,
                    display_name: &display,
                    base_url: base_url.as_deref(),
                    config_json: &config_json,
                })
                .map_err(storage)
        })
        .await?;
        Ok(self.provider_info(&p, 0))
    }

    pub async fn list_providers(&self) -> Result<Vec<ProviderInfo>, ErrorBody> {
        let db = self.db.clone();
        let (providers, accounts) = blocking(move || {
            Ok((
                db.providers().list().map_err(storage)?,
                db.accounts().list(None).map_err(storage)?,
            ))
        })
        .await?;
        Ok(providers
            .iter()
            .map(|p| {
                let n = accounts.iter().filter(|a| a.provider_id == p.id).count();
                self.provider_info(p, n)
            })
            .collect())
    }

    pub async fn remove_provider(&self, slug: String) -> Result<bool, ErrorBody> {
        let (db, secrets) = (self.db.clone(), self.secrets.clone());
        let removed = blocking(move || {
            let refs = db.providers().delete_by_slug(&slug).map_err(storage)?;
            if let Some(refs) = &refs {
                delete_secrets(secrets.as_ref(), refs);
            }
            Ok(refs.is_some())
        })
        .await?;
        if removed {
            self.rebuild_routes().await?;
        }
        Ok(removed)
    }

    // ---- accounts -------------------------------------------------------------------------

    pub(crate) fn load_provider(db: &Database, slug: &str) -> Result<Provider, ErrorBody> {
        db.providers()
            .get_by_slug(slug)
            .map_err(storage)?
            .ok_or_else(|| ErrorBody::not_found(format!("provider {slug:?} not found")))
    }

    pub(crate) fn load_account(
        db: &Database,
        provider: &Provider,
        alias: &str,
    ) -> Result<Account, ErrorBody> {
        db.accounts()
            .get(&provider.id, alias)
            .map_err(storage)?
            .ok_or_else(|| {
                ErrorBody::not_found(format!("account {}/{alias} not found", provider.slug))
            })
    }

    fn account_info(
        db: &Database,
        provider: &Provider,
        a: &Account,
    ) -> Result<AccountInfo, ErrorBody> {
        let models = db.account_models().list(&a.id).map_err(storage)?;
        let has_key = db
            .accounts()
            .credentials(&a.id)
            .map_err(storage)?
            .iter()
            .any(|c| c.secret_ref.is_some());
        Ok(AccountInfo {
            provider: provider.slug.clone(),
            alias: a.alias.clone(),
            display_name: a.display_name.clone(),
            account_type: a.account_type.clone(),
            enabled: a.enabled,
            position: a.position,
            has_key,
            enabled_models: models.iter().filter(|m| m.enabled).count(),
            models: models.len(),
        })
    }

    /// All model ids of a provider (across accounts), for alias ambiguity checks.
    fn provider_model_ids(db: &Database, provider: &Provider) -> Result<Vec<String>, ErrorBody> {
        let mut ids = Vec::new();
        for a in db.accounts().list(Some(&provider.id)).map_err(storage)? {
            ids.extend(
                db.account_models()
                    .list(&a.id)
                    .map_err(storage)?
                    .into_iter()
                    .map(|m| m.upstream_model_id),
            );
        }
        Ok(ids)
    }

    pub async fn add_account(
        &self,
        provider_slug: String,
        alias: String,
        api_key: Option<String>,
        display_name: Option<String>,
        sync: bool,
    ) -> Result<(AccountInfo, Option<Result<SyncResult, ErrorBody>>), ErrorBody> {
        let (db, secrets, registry) =
            (self.db.clone(), self.secrets.clone(), self.registry.clone());
        let slug = provider_slug.clone();
        let alias_c = alias.clone();
        let info = blocking(move || {
            let provider = Self::load_provider(&db, &slug)?;
            let module = registry.get(&provider.kind).ok_or_else(|| {
                ErrorBody::internal(format!("unknown provider kind {}", provider.kind))
            })?;
            let ids = Self::provider_model_ids(&db, &provider)?;
            validate_alias(&alias_c, ids.iter().map(String::as_str))
                .map_err(|e| ErrorBody::invalid_params(e.to_string()))?;
            let key = api_key
                .map(|k| k.trim().to_owned())
                .filter(|k| !k.is_empty());
            let d = module.descriptor();
            if d.auth == AuthMethod::ApiKey && key.is_none() {
                return Err(ErrorBody::invalid_params(format!(
                    "{} accounts need an API key",
                    d.display_name
                )));
            }
            let account_type = if key.is_some() {
                "api_key"
            } else if d.auth == AuthMethod::ExternalManaged {
                "subscription"
            } else {
                "local"
            };
            let account = db
                .accounts()
                .create(&NewAccount {
                    provider_id: &provider.id,
                    alias: &alias_c,
                    display_name: display_name.as_deref(),
                    account_type,
                })
                .map_err(storage)?;
            if let Some(key) = key {
                let secret_ref = SecretRef::generate();
                secrets
                    .put(&secret_ref, &Secret::new(key))
                    .map_err(|e| ErrorBody::internal(format!("cannot store the API key: {e}")))?;
                if let Err(e) = db.accounts().add_credential(
                    &account.id,
                    "api_key",
                    "user",
                    Some(&secret_ref.to_string()),
                ) {
                    let _ = secrets.delete(&secret_ref);
                    let _ = db.accounts().delete(&account.id);
                    return Err(storage(e));
                }
            }
            Self::account_info(&db, &provider, &account)
        })
        .await?;

        let sync_result = if sync {
            Some(self.sync_models(provider_slug.clone(), alias.clone()).await)
        } else {
            self.rebuild_routes().await?;
            None
        };
        let info = self.account(&provider_slug, &alias).await.unwrap_or(info);
        Ok((info, sync_result))
    }

    pub async fn account(
        &self,
        provider_slug: &str,
        alias: &str,
    ) -> Result<AccountInfo, ErrorBody> {
        let db = self.db.clone();
        let (slug, alias) = (provider_slug.to_owned(), alias.to_owned());
        blocking(move || {
            let p = Self::load_provider(&db, &slug)?;
            let a = Self::load_account(&db, &p, &alias)?;
            Self::account_info(&db, &p, &a)
        })
        .await
    }

    pub async fn list_accounts(
        &self,
        provider: Option<String>,
    ) -> Result<Vec<AccountInfo>, ErrorBody> {
        let db = self.db.clone();
        blocking(move || {
            let providers = match &provider {
                Some(slug) => vec![Self::load_provider(&db, slug)?],
                None => db.providers().list().map_err(storage)?,
            };
            let mut out = Vec::new();
            for p in &providers {
                for a in db.accounts().list(Some(&p.id)).map_err(storage)? {
                    out.push(Self::account_info(&db, p, &a)?);
                }
            }
            Ok(out)
        })
        .await
    }

    pub async fn set_account_enabled(
        &self,
        provider: String,
        alias: String,
        enabled: bool,
    ) -> Result<AccountInfo, ErrorBody> {
        let db = self.db.clone();
        let (p2, a2) = (provider.clone(), alias.clone());
        blocking(move || {
            let p = Self::load_provider(&db, &p2)?;
            let a = Self::load_account(&db, &p, &a2)?;
            db.accounts().set_enabled(&a.id, enabled).map_err(storage)?;
            Ok(())
        })
        .await?;
        self.rebuild_routes().await?;
        self.account(&provider, &alias).await
    }

    pub async fn set_account_key(
        &self,
        provider: String,
        alias: String,
        api_key: String,
    ) -> Result<AccountInfo, ErrorBody> {
        let key = api_key.trim().to_owned();
        if key.is_empty() {
            return Err(ErrorBody::invalid_params("the API key is empty"));
        }
        let (db, secrets) = (self.db.clone(), self.secrets.clone());
        let (p2, a2) = (provider.clone(), alias.clone());
        let account_id = blocking(move || {
            let p = Self::load_provider(&db, &p2)?;
            let a = Self::load_account(&db, &p, &a2)?;
            let secret_ref = SecretRef::generate();
            secrets
                .put(&secret_ref, &Secret::new(key))
                .map_err(|e| ErrorBody::internal(format!("cannot store the API key: {e}")))?;
            let old = db
                .accounts()
                .replace_credential(&a.id, "api_key", "user", Some(&secret_ref.to_string()))
                .map_err(storage)?;
            delete_secrets(secrets.as_ref(), &old);
            Ok(a.id)
        })
        .await?;
        // New credentials: forget auth failures of this account's targets.
        for t in self.routes.load().targets() {
            if *t.account_id == *account_id {
                t.health.reset();
            }
        }
        self.rebuild_routes().await?;
        self.account(&provider, &alias).await
    }

    /// Records the user's subscription plan, price and renewal date (product doc §35).
    pub async fn set_account_plan(
        &self,
        provider: String,
        alias: String,
        name: Option<String>,
        price: Option<f64>,
        renews_at_ms: Option<i64>,
    ) -> Result<AccountInfo, ErrorBody> {
        if price.is_some_and(|p| !p.is_finite() || p < 0.0) {
            return Err(ErrorBody::invalid_params(
                "price must be a non-negative number",
            ));
        }
        let db = self.db.clone();
        let (p2, a2) = (provider.clone(), alias.clone());
        blocking(move || {
            let p = Self::load_provider(&db, &p2)?;
            let a = Self::load_account(&db, &p, &a2)?;
            let name = name.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty());
            db.accounts()
                .set_plan(&a.id, name.as_deref(), price, renews_at_ms)
                .map_err(storage)?;
            Ok(())
        })
        .await?;
        self.account(&provider, &alias).await
    }

    pub async fn remove_account(&self, provider: String, alias: String) -> Result<bool, ErrorBody> {
        let (db, secrets) = (self.db.clone(), self.secrets.clone());
        let removed = blocking(move || {
            let p = Self::load_provider(&db, &provider)?;
            let Some(a) = db.accounts().get(&p.id, &alias).map_err(storage)? else {
                return Ok(false);
            };
            if let Some(refs) = db.accounts().delete(&a.id).map_err(storage)? {
                delete_secrets(secrets.as_ref(), &refs);
            }
            Ok(true)
        })
        .await?;
        if removed {
            self.rebuild_routes().await?;
        }
        Ok(removed)
    }

    // ---- models ---------------------------------------------------------------------------

    /// Fetches the account's model list from the upstream Catalog driver and applies it.
    pub async fn sync_models(
        &self,
        provider: String,
        alias: String,
    ) -> Result<SyncResult, ErrorBody> {
        let (db, secrets, registry) =
            (self.db.clone(), self.secrets.clone(), self.registry.clone());
        let (p_slug, a_alias) = (provider.clone(), alias.clone());
        let (provider_row, account, key, others) = blocking(move || {
            let p = Self::load_provider(&db, &p_slug)?;
            registry
                .get(&p.kind)
                .ok_or_else(|| ErrorBody::internal("unknown provider kind"))?;
            let a = Self::load_account(&db, &p, &a_alias)?;
            let key = db
                .accounts()
                .credentials(&a.id)
                .map_err(storage)?
                .into_iter()
                .find_map(|c| c.secret_ref)
                .and_then(|r| read_secret(secrets.as_ref(), &r));
            let others: Vec<String> = db
                .accounts()
                .list(Some(&p.id))
                .map_err(storage)?
                .into_iter()
                .filter(|o| o.id != a.id)
                .map(|o| o.alias)
                .collect();
            Ok((p, a, key, others))
        })
        .await?;
        let module = self
            .registry
            .get(&provider_row.kind)
            .ok_or_else(|| ErrorBody::internal("unknown provider kind"))?
            .clone();
        let config = provider_config(&provider_row);
        let models = module
            .list_models(DriverContext {
                http: &self.http,
                config: &config,
                key: key.as_ref(),
                home: Some(&self.home),
            })
            .await
            .map_err(|e| match e {
                ProviderError::NotSupported(_) => ErrorBody::invalid_params(format!(
                    "{} cannot list models; add them with `loadout model add`",
                    provider_row.slug
                )),
                ProviderError::Upstream(f) => ErrorBody::new(
                    loadout_ipc::ErrorCode::Internal,
                    format!("model list request failed: {f}"),
                ),
                other => provider_error(other),
            })?;

        let default_protocols = config
            .settings
            .get("model_protocols")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .filter_map(Protocol::parse)
                    .collect::<Vec<_>>()
            })
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| module.descriptor().default_model_protocols.to_vec());
        let aliases: HashSet<&str> = others
            .iter()
            .map(String::as_str)
            .chain([alias.as_str()])
            .collect();
        let ambiguous: Vec<String> = models
            .iter()
            .filter(|m| {
                m.upstream_id
                    .split_once('/')
                    .is_some_and(|(head, _)| aliases.contains(head))
            })
            .map(|m| m.upstream_id.clone())
            .collect();
        let inputs: Vec<ModelInput> = models
            .iter()
            .map(|m| model_input(m, &default_protocols))
            .collect();
        let total = inputs.len();
        let db = self.db.clone();
        let counts = blocking(move || {
            db.account_models()
                .sync_catalog(&account.id, &inputs)
                .map_err(storage)
        })
        .await?;
        self.rebuild_routes().await?;
        Ok(SyncResult {
            added: counts.added,
            updated: counts.updated,
            disabled: counts.disabled,
            total,
            ambiguous,
        })
    }

    pub async fn add_model(
        &self,
        provider: String,
        alias: String,
        model: String,
        protocols: Vec<String>,
        context_window: Option<i64>,
    ) -> Result<ModelInfo, ErrorBody> {
        let model = model.trim().to_owned();
        if model.is_empty() {
            return Err(ErrorBody::invalid_params("model id is empty"));
        }
        let parsed: Vec<Protocol> = protocols
            .iter()
            .map(|p| {
                Protocol::parse(p).ok_or_else(|| {
                    ErrorBody::invalid_params(format!(
                        "unknown protocol {p:?} (chat, responses, messages)"
                    ))
                })
            })
            .collect::<Result<_, _>>()?;
        let (db, registry) = (self.db.clone(), self.registry.clone());
        let info = blocking(move || {
            let p = Self::load_provider(&db, &provider)?;
            let module = registry.get(&p.kind).ok_or_else(|| ErrorBody::internal("unknown provider kind"))?;
            let a = Self::load_account(&db, &p, &alias)?;
            if let Some((head, _)) = model.split_once('/') {
                let aliases: Vec<String> = db.accounts().list(Some(&p.id)).map_err(storage)?.into_iter().map(|a| a.alias).collect();
                if aliases.iter().any(|x| x == head) {
                    return Err(ErrorBody::invalid_params(format!(
                        "model {model:?} starts with the account alias {head:?}; rename that account first (ADR-010)"
                    )));
                }
            }
            let protocols = if parsed.is_empty() {
                module.descriptor().default_model_protocols.to_vec()
            } else {
                for proto in &parsed {
                    if module.descriptor().transport(*proto).is_none() {
                        return Err(ErrorBody::invalid_params(format!("{} does not support the {proto} protocol", p.slug)));
                    }
                }
                parsed
            };
            let m = db
                .account_models()
                .add_manual(
                    &a.id,
                    &ModelInput {
                        upstream_model_id: model,
                        protocols_json: protocols_json(&protocols),
                        context_window,
                        capabilities_json: loadout_provider_sdk::Capabilities::DEFAULT_MODEL.to_json(),
                        ..ModelInput::default()
                    },
                )
                .map_err(storage)?;
            Ok(model_info(&p, &a, &m))
        })
        .await?;
        self.rebuild_routes().await?;
        Ok(info)
    }

    pub async fn list_models(
        &self,
        provider: Option<String>,
        account: Option<String>,
    ) -> Result<Vec<ModelInfo>, ErrorBody> {
        let db = self.db.clone();
        blocking(move || {
            let providers = match &provider {
                Some(slug) => vec![Self::load_provider(&db, slug)?],
                None => db.providers().list().map_err(storage)?,
            };
            let mut out = Vec::new();
            for p in &providers {
                for a in db.accounts().list(Some(&p.id)).map_err(storage)? {
                    if account.as_ref().is_some_and(|x| *x != a.alias) {
                        continue;
                    }
                    for m in db.account_models().list(&a.id).map_err(storage)? {
                        out.push(model_info(p, &a, &m));
                    }
                }
            }
            Ok(out)
        })
        .await
    }

    /// Applies `f` to the model named `provider/account/model`, rebuilds routes and returns it.
    pub(crate) async fn update_model<F>(&self, name: String, f: F) -> Result<ModelInfo, ErrorBody>
    where
        F: FnOnce(&Database, &AccountModel) -> Result<(), StorageError> + Send + 'static,
    {
        let db = self.db.clone();
        let info = blocking(move || {
            let (ps, al, model) = split_exact(&name)?;
            let p = Self::load_provider(&db, ps)?;
            let a = Self::load_account(&db, &p, al)?;
            let m = db
                .account_models()
                .find(&a.id, model)
                .map_err(storage)?
                .ok_or_else(|| ErrorBody::not_found(format!("model {name} not found")))?;
            f(&db, &m).map_err(storage)?;
            let m = db
                .account_models()
                .get(&m.id)
                .map_err(storage)?
                .ok_or_else(|| ErrorBody::not_found("model"))?;
            Ok(model_info(&p, &a, &m))
        })
        .await?;
        self.rebuild_routes().await?;
        Ok(info)
    }

    pub async fn set_model_enabled(
        &self,
        name: String,
        enabled: bool,
    ) -> Result<ModelInfo, ErrorBody> {
        self.update_model(name, move |db, m| {
            db.account_models().set_enabled(&m.id, enabled).map(|_| ())
        })
        .await
    }

    pub async fn set_model_price(
        &self,
        name: String,
        input: Option<f64>,
        output: Option<f64>,
        cache_read: Option<f64>,
        cache_write: Option<f64>,
    ) -> Result<ModelInfo, ErrorBody> {
        for v in [input, output, cache_read, cache_write]
            .into_iter()
            .flatten()
        {
            if !v.is_finite() || v < 0.0 {
                return Err(ErrorBody::invalid_params(
                    "prices must be non-negative numbers",
                ));
            }
        }
        self.update_model(name, move |db, m| {
            db.account_models()
                .set_prices(&m.id, input, output, cache_read, cache_write)
                .map(|_| ())
        })
        .await
    }

    pub async fn remove_model(&self, name: String) -> Result<bool, ErrorBody> {
        let db = self.db.clone();
        let removed = blocking(move || {
            let (ps, al, model) = split_exact(&name)?;
            let p = Self::load_provider(&db, ps)?;
            let a = Self::load_account(&db, &p, al)?;
            match db.account_models().find(&a.id, model).map_err(storage)? {
                Some(m) => db.account_models().delete(&m.id).map_err(storage),
                None => Ok(false),
            }
        })
        .await?;
        if removed {
            self.rebuild_routes().await?;
        }
        Ok(removed)
    }

    // ---- status ---------------------------------------------------------------------------

    pub fn route_status(&self) -> Vec<TargetStatus> {
        let now = now_ms();
        self.routes
            .load()
            .targets()
            .iter()
            .map(|t| {
                let h = t.health.snapshot();
                let key = loadout_router::account_model_key(&t.id);
                let stats = self.latency.stats(key, now);
                let rank = self.latency.rank(key);
                TargetStatus {
                    name: t.exact_name(),
                    protocol: t.protocol.as_str().to_owned(),
                    trust: t.trust.as_str().to_owned(),
                    state: if h.until_ms.is_some_and(|u| u <= now)
                        && h.state != loadout_router::HealthState::Healthy
                    {
                        "RECOVERING".to_owned()
                    } else if h.degraded {
                        "DEGRADED".to_owned()
                    } else {
                        h.state.as_str().to_owned()
                    },
                    unavailable_until_ms: h.until_ms.filter(|u| *u > now),
                    consecutive_failures: h.consecutive_failures,
                    in_flight: h.in_flight,
                    successes: h.successes,
                    failures: h.failures,
                    last_error: h.last_error,
                    tier: rank.tier.as_str().to_owned(),
                    tier_pinned: rank.pinned,
                    ttft_p50_ms: stats.ttft_p50_ms,
                    ttft_p90_ms: stats.ttft_p90_ms,
                    tps: stats.tps_p50,
                    samples: stats.samples,
                    last_sample_ms: stats.last_sample_ms,
                }
            })
            .collect()
    }

    /// Filtered request log; client ids are shown as client names.
    pub async fn recent_requests(
        &self,
        p: loadout_ipc::api::RecentParams,
    ) -> Result<Vec<RequestSummary>, ErrorBody> {
        let db = self.db.clone();
        let (rows, names) = blocking(move || {
            let clients = db.clients().list().map_err(storage)?;
            let client_id = match &p.client {
                Some(name) => Some(
                    clients
                        .iter()
                        .find(|c| &c.name == name)
                        .map(|c| c.id.to_string())
                        .ok_or_else(|| ErrorBody::not_found(format!("client {name} not found")))?,
                ),
                None => None,
            };
            let rows = db
                .find_requests(&loadout_storage::RequestFilter {
                    limit: p.limit.clamp(1, 1000),
                    client_id,
                    search: p.search,
                    status: p.status,
                    since_ms: p.since_ms,
                })
                .map_err(storage)?;
            let names: HashMap<String, String> = clients
                .into_iter()
                .map(|c| (c.id.to_string(), c.name))
                .collect();
            Ok((rows, names))
        })
        .await?;
        Ok(rows
            .into_iter()
            .map(|e| RequestSummary {
                ts_ms: e.ts,
                client: e.client_id.map(|id| names.get(&id).cloned().unwrap_or(id)),
                requested_model: e.requested_model,
                route: match (&e.provider, &e.account, &e.upstream_model) {
                    (Some(p), Some(a), Some(m)) => Some(format!("{p}/{a}/{m}")),
                    _ => None,
                },
                protocol_in: e.protocol_in,
                protocol_out: e.protocol_out,
                mode: e.mode,
                stream: e.stream,
                status: e.status,
                error_category: e.error_category,
                attempts: e.attempts,
                input_tokens: e.input_tokens,
                output_tokens: e.output_tokens,
                ttft_ms: e.ttft_ms,
                duration_ms: e.duration_ms,
                cost_usd: e.estimated_cost_usd,
            })
            .collect())
    }

    /// `stats.summary`: aggregates grouped by model / route / provider / client / day.
    pub async fn stats(
        &self,
        since_ms: i64,
        group_by: String,
    ) -> Result<Vec<loadout_ipc::api::StatsEntry>, ErrorBody> {
        let group = loadout_storage::StatsGroup::parse(&group_by).ok_or_else(|| {
            ErrorBody::invalid_params("group_by must be model, route, provider, client or day")
        })?;
        let db = self.db.clone();
        let (rows, names) = blocking(move || {
            let rows = db.request_stats(since_ms, group).map_err(storage)?;
            let names: HashMap<String, String> = db
                .clients()
                .list()
                .map_err(storage)?
                .into_iter()
                .map(|c| (c.id.to_string(), c.name))
                .collect();
            Ok((rows, names))
        })
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| loadout_ipc::api::StatsEntry {
                key: if group == loadout_storage::StatsGroup::Client {
                    names.get(&r.key).cloned().unwrap_or(r.key)
                } else {
                    r.key
                },
                requests: r.requests,
                errors: r.errors,
                input_tokens: r.input_tokens,
                output_tokens: r.output_tokens,
                cost_usd: r.cost_usd,
                avg_ttft_ms: r.avg_ttft_ms,
                avg_tps: r.avg_tps,
            })
            .collect())
    }
}

fn delete_secrets(secrets: &dyn SecretStore, refs: &[String]) {
    for r in refs {
        if let Ok(parsed) = r.parse::<SecretRef>()
            && let Err(e) = secrets.delete(&parsed)
        {
            tracing::warn!(secret_ref = %r, error = %e, "could not delete credential");
        }
    }
}

fn model_input(m: &CatalogModel, default_protocols: &[Protocol]) -> ModelInput {
    let protocols = if m.protocols.is_empty() {
        default_protocols.to_vec()
    } else {
        m.protocols.clone()
    };
    ModelInput {
        upstream_model_id: m.upstream_id.clone(),
        protocols_json: protocols_json(&protocols),
        context_window: m.context_window.map(i64::from),
        max_output_tokens: m.max_output_tokens.map(i64::from),
        capabilities_json: m.capabilities.to_json(),
        input_price: m.input_price,
        output_price: m.output_price,
        cache_read_price: m.cache_read_price,
        cache_write_price: m.cache_write_price,
    }
}

fn model_info(p: &Provider, a: &Account, m: &AccountModel) -> ModelInfo {
    ModelInfo {
        name: format!("{}/{}/{}", p.slug, a.alias, m.upstream_model_id),
        provider: p.slug.clone(),
        account: a.alias.clone(),
        upstream_model: m.upstream_model_id.clone(),
        protocols: parse_protocols(&m.protocols_json)
            .iter()
            .map(|p| p.as_str().to_owned())
            .collect(),
        enabled: m.enabled,
        source: m.source.clone(),
        context_window: m.context_window,
        input_price: m.input_price,
        output_price: m.output_price,
        tier_override: m.tier_override.clone(),
    }
}
