// SPDX-License-Identifier: Apache-2.0

//! Maps IPC methods to daemon operations. Database work runs on the blocking pool so the
//! async runtime (and the gateway sharing it) never stalls on SQLite.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use loadout_gateway::{ClientId, ClientKeyIndex, generate_client_key};
use loadout_ipc::api::{self, ClientInfo, Method, events};
use loadout_ipc::{ErrorBody, ErrorCode, Event, Handler, decode_params, encode_result};
use loadout_storage::{Client, Database, NewClient, StorageError};
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::control::ControlPlane;
use crate::settings;

const MAX_CLIENT_NAME: usize = 64;

#[derive(Debug)]
pub(crate) struct DaemonState {
    pub db: Arc<Database>,
    pub control: Arc<ControlPlane>,
    pub keys: Arc<ClientKeyIndex>,
    pub events: broadcast::Sender<Event>,
    pub shutdown: CancellationToken,
    pub started_at_ms: i64,
    pub gateway_addr: SocketAddr,
    pub data_dir: PathBuf,
    /// Where a damaged database was moved at startup (ExecPlan 007).
    pub recovered_from: Option<String>,
}

impl DaemonState {
    /// Runs blocking database work off the async threads.
    async fn db<T, F>(&self, f: F) -> Result<T, ErrorBody>
    where
        T: Send + 'static,
        F: FnOnce(&Database) -> Result<T, StorageError> + Send + 'static,
    {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || f(&db))
            .await
            .map_err(|_| ErrorBody::internal("database task failed"))?
            .map_err(storage_error)
    }

    fn emit(&self, name: &str, data: Value) {
        // No receivers is normal when no UI is connected.
        let _ = self.events.send(Event::new(name, data));
    }

    /// Rebuilds the gateway's in-memory key index from the database.
    pub(crate) async fn refresh_keys(&self) -> Result<(), ErrorBody> {
        let (entries, clients) = self
            .db(|db| Ok((db.clients().active_key_hashes()?, db.clients().list()?)))
            .await?;
        self.keys.replace(
            entries
                .into_iter()
                .map(|(hash, id)| (hash, ClientId::from(id.as_str()))),
        );
        crate::clients::publish(&self.control.policies, &clients);
        Ok(())
    }

    fn client_info(&self, c: Client) -> ClientInfo {
        let usage = self
            .control
            .policies
            .usage_today(c.id.as_str(), loadout_storage::now_ms());
        ClientInfo {
            id: c.id.to_string(),
            policy: crate::clients::parse_policy(&c.policy_json),
            name: c.name,
            key_prefix: c.key_prefix,
            enabled: c.enabled,
            created_at_ms: c.created_at,
            revoked_at_ms: c.revoked_at,
            tokens_today: usage.tokens,
            cost_today_usd: usage.cost_usd,
        }
    }
}

fn storage_error(e: StorageError) -> ErrorBody {
    match e {
        StorageError::Conflict(what) => ErrorBody::conflict(format!("{what} already exists")),
        StorageError::NotFound(what) => ErrorBody::not_found(format!("{what} not found")),
        other => {
            tracing::error!(error = %other, "database operation failed");
            ErrorBody::internal("database error; see the daemon log for details")
        }
    }
}

fn validate_client_name(name: &str) -> Result<String, ErrorBody> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_CLIENT_NAME {
        return Err(ErrorBody::invalid_params(format!(
            "client name must be 1-{MAX_CLIENT_NAME} characters"
        )));
    }
    if name.chars().any(char::is_control) {
        return Err(ErrorBody::invalid_params(
            "client name must not contain control characters",
        ));
    }
    Ok(name.to_owned())
}

#[derive(Debug)]
pub(crate) struct Dispatcher {
    pub state: Arc<DaemonState>,
}

impl Handler for Dispatcher {
    async fn handle(&self, method: &str, params: Value) -> Result<Value, ErrorBody> {
        let s = &self.state;
        match method {
            api::Status::NAME => {
                let schema_version = s.db(|db| db.schema_version()).await?;
                encode_result::<api::Status>(&api::DaemonStatus {
                    version: crate::VERSION.to_owned(),
                    pid: std::process::id(),
                    started_at_ms: s.started_at_ms,
                    data_dir: s.data_dir.display().to_string(),
                    recovered_from: s.recovered_from.clone(),
                    schema_version,
                    gateway: api::GatewayStatus {
                        addr: s.gateway_addr.to_string(),
                        listening: !s.shutdown.is_cancelled(),
                    },
                })
            }

            api::Shutdown::NAME => {
                tracing::info!("shutdown requested over IPC");
                s.emit(events::DAEMON_STOPPING, Value::Null);
                s.shutdown.cancel();
                encode_result::<api::Shutdown>(&())
            }

            api::ClientCreate::NAME => {
                let p = decode_params::<api::ClientCreate>(params)?;
                let name = validate_client_name(&p.name)?;
                let key = generate_client_key();
                let hash = key.hash;
                let prefix = key.display_prefix.clone();
                let client = s
                    .db(move |db| {
                        db.clients().create(&NewClient {
                            name: &name,
                            key_prefix: &prefix,
                            key_hash: hash,
                        })
                    })
                    .await?;
                s.refresh_keys().await?;
                tracing::info!(client = %client.name, prefix = %client.key_prefix, "client created");
                s.emit(events::CLIENTS_CHANGED, Value::Null);
                encode_result::<api::ClientCreate>(&api::ClientCreated {
                    client: s.client_info(client),
                    key: key.key,
                })
            }

            api::ClientList::NAME => {
                let clients = s.db(|db| db.clients().list()).await?;
                encode_result::<api::ClientList>(
                    &clients.into_iter().map(|c| s.client_info(c)).collect(),
                )
            }

            api::ClientSetPolicy::NAME => {
                let p = decode_params::<api::ClientSetPolicy>(params)?;
                let policy = crate::clients::validate(p.policy)?;
                let json = crate::clients::policy_json(&policy);
                let name = p.name.clone();
                let client = s
                    .db(move |db| {
                        if !db.clients().set_policy(&name, &json)? {
                            return Err(StorageError::NotFound(format!("client {name}")));
                        }
                        db.clients()
                            .get_by_name(&name)?
                            .ok_or_else(|| StorageError::NotFound(format!("client {name}")))
                    })
                    .await?;
                s.refresh_keys().await?;
                s.emit(events::CLIENTS_CHANGED, Value::Null);
                encode_result::<api::ClientSetPolicy>(&s.client_info(client))
            }

            api::ClientRevoke::NAME => {
                let p = decode_params::<api::ClientRevoke>(params)?;
                let name = p.name.trim().to_owned();
                let revoked = s.db(move |db| db.clients().revoke_by_name(&name)).await?;
                if revoked {
                    s.refresh_keys().await?;
                    tracing::info!(client = %p.name.trim(), "client revoked");
                    s.emit(events::CLIENTS_CHANGED, Value::Null);
                }
                encode_result::<api::ClientRevoke>(&api::Revoked { revoked })
            }

            api::BackupExport::NAME => {
                let p = decode_params::<api::BackupExport>(params)?;
                let (db, dir) = (s.db.clone(), s.data_dir.clone());
                let result =
                    tokio::task::spawn_blocking(move || crate::backup::export(&db, &dir, p.path))
                        .await
                        .map_err(|_| ErrorBody::internal("backup task failed"))?
                        .map_err(ErrorBody::invalid_params)?;
                encode_result::<api::BackupExport>(&result)
            }

            api::BackupDatabase::NAME => {
                let p = decode_params::<api::BackupDatabase>(params)?;
                let (db, dir) = (s.db.clone(), s.data_dir.clone());
                let path = tokio::task::spawn_blocking(move || {
                    crate::backup::backup_database(&db, &dir, p.path)
                })
                .await
                .map_err(|_| ErrorBody::internal("backup task failed"))?
                .map_err(ErrorBody::invalid_params)?;
                encode_result::<api::BackupDatabase>(&api::DatabaseBackup { path })
            }

            api::BackupImport::NAME => {
                let p = decode_params::<api::BackupImport>(params)?;
                let db = s.db.clone();
                let report =
                    tokio::task::spawn_blocking(move || crate::backup::import(&db, &p.path))
                        .await
                        .map_err(|_| ErrorBody::internal("import task failed"))?
                        .map_err(ErrorBody::invalid_params)?;
                tracing::info!(?report, "configuration imported");
                s.refresh_keys().await?;
                s.control.rebuild_routes().await?;
                s.control.evaluate_latency(true).await?;
                s.emit(events::CLIENTS_CHANGED, Value::Null);
                s.emit(events::ROUTES_CHANGED, Value::Null);
                encode_result::<api::BackupImport>(&report)
            }

            api::SettingsList::NAME => {
                let all = s.db(settings::list).await?;
                encode_result::<api::SettingsList>(&all)
            }

            api::SettingsSet::NAME => {
                let p = decode_params::<api::SettingsSet>(params)?;
                let def = settings::find(&p.key).ok_or_else(|| {
                    ErrorBody::invalid_params(format!("unknown setting {}", p.key))
                })?;
                let value = p.value.trim().to_owned();
                def.validate(&value)
                    .map_err(|e| ErrorBody::invalid_params(format!("{}: {e}", def.key)))?;
                let setting = s
                    .db(move |db| {
                        db.settings().set(def.key, &value)?;
                        settings::view(db, def)
                    })
                    .await?;
                s.emit(events::SETTINGS_CHANGED, json!({ "key": def.key }));
                if def.key.starts_with("latency.") {
                    // New tier thresholds apply right away.
                    s.control.evaluate_latency(true).await?;
                }
                if def.key.starts_with("web_session.") {
                    s.control.apply_web_settings().await?;
                    s.emit(events::ROUTES_CHANGED, Value::Null);
                }
                encode_result::<api::SettingsSet>(&setting)
            }

            api::SettingsReset::NAME => {
                let p = decode_params::<api::SettingsReset>(params)?;
                let def = settings::find(&p.key).ok_or_else(|| {
                    ErrorBody::invalid_params(format!("unknown setting {}", p.key))
                })?;
                let setting = s
                    .db(move |db| {
                        db.settings().remove(def.key)?;
                        settings::view(db, def)
                    })
                    .await?;
                s.emit(events::SETTINGS_CHANGED, json!({ "key": def.key }));
                if def.key.starts_with("latency.") {
                    // New tier thresholds apply right away.
                    s.control.evaluate_latency(true).await?;
                }
                if def.key.starts_with("web_session.") {
                    s.control.apply_web_settings().await?;
                    s.emit(events::ROUTES_CHANGED, Value::Null);
                }
                encode_result::<api::SettingsReset>(&setting)
            }

            other => self.handle_routes(other, params).await,
        }
    }
}

impl Dispatcher {
    /// Provider / account / model / status methods (ExecPlan 002 §5.6).
    async fn handle_routes(&self, method: &str, params: Value) -> Result<Value, ErrorBody> {
        let s = &self.state;
        let c = &s.control;
        let changed = |s: &DaemonState| s.emit(events::ROUTES_CHANGED, Value::Null);
        match method {
            api::ProviderKinds::NAME => encode_result::<api::ProviderKinds>(&c.kinds()),
            api::ProviderAdd::NAME => {
                let p = decode_params::<api::ProviderAdd>(params)?;
                let info = c
                    .add_provider(p.slug, p.kind, p.base_url, p.settings, p.display_name)
                    .await?;
                changed(s);
                encode_result::<api::ProviderAdd>(&info)
            }
            api::ProviderList::NAME => {
                encode_result::<api::ProviderList>(&c.list_providers().await?)
            }
            api::ProviderRemove::NAME => {
                let p = decode_params::<api::ProviderRemove>(params)?;
                let revoked = c.remove_provider(p.slug).await?;
                changed(s);
                encode_result::<api::ProviderRemove>(&api::Revoked { revoked })
            }
            api::ProviderSetSettings::NAME => {
                let p = decode_params::<api::ProviderSetSettings>(params)?;
                let info = c.set_provider_settings(p.slug, p.settings).await?;
                changed(s);
                encode_result::<api::ProviderSetSettings>(&info)
            }
            api::AccountOAuthStart::NAME => {
                let p = decode_params::<api::AccountOAuthStart>(params)?;
                encode_result::<api::AccountOAuthStart>(&c.oauth_start(p.provider, p.alias).await?)
            }
            api::AccountOAuthStatus::NAME => {
                let p = decode_params::<api::AccountOAuthStatus>(params)?;
                let status = c.oauth_status(&p.login_id)?;
                if status.state == "done" {
                    changed(s);
                }
                encode_result::<api::AccountOAuthStatus>(&status)
            }
            api::AccountWebAdd::NAME => {
                let p = decode_params::<api::AccountWebAdd>(params)?;
                let info = c
                    .web_add(p.provider, p.alias, p.confirm, p.display_name, p.sign_in)
                    .await?;
                changed(s);
                encode_result::<api::AccountWebAdd>(&info)
            }
            api::AccountWebStatus::NAME => {
                let p = decode_params::<api::AccountWebStatus>(params)?;
                encode_result::<api::AccountWebStatus>(&c.web_status(p.provider, p.alias).await?)
            }
            api::AccountWebShow::NAME => {
                let p = decode_params::<api::AccountWebShow>(params)?;
                encode_result::<api::AccountWebShow>(&c.web_show(p.provider, p.alias).await?)
            }
            api::AccountWebLogin::NAME => {
                let p = decode_params::<api::AccountWebLogin>(params)?;
                let status = c.web_login(p.provider, p.alias, p.done).await?;
                encode_result::<api::AccountWebLogin>(&status)
            }
            api::AccountWebRecheck::NAME => {
                let p = decode_params::<api::AccountWebRecheck>(params)?;
                let status = c.web_recheck(p.provider, p.alias).await?;
                encode_result::<api::AccountWebRecheck>(&status)
            }
            api::AccountWebBind::NAME => {
                let p = decode_params::<api::AccountWebBind>(params)?;
                let status = c.web_bind(p.provider, p.alias, p.conversation_url).await?;
                encode_result::<api::AccountWebBind>(&status)
            }
            api::AccountWebConfig::NAME => {
                let p = decode_params::<api::AccountWebConfig>(params)?;
                let status = c
                    .web_config(
                        p.provider,
                        p.alias,
                        p.mode,
                        p.min_interval_secs,
                        p.daily_soft_cap,
                    )
                    .await?;
                encode_result::<api::AccountWebConfig>(&status)
            }
            api::AccountSetTransport::NAME => {
                let p = decode_params::<api::AccountSetTransport>(params)?;
                let info = c
                    .set_transport(p.provider, p.alias, p.enabled, p.confirm)
                    .await?;
                changed(s);
                encode_result::<api::AccountSetTransport>(&info)
            }
            api::AccountAdd::NAME => {
                let p = decode_params::<api::AccountAdd>(params)?;
                let (account, sync) = c
                    .add_account(
                        p.provider,
                        p.alias,
                        p.api_key.map(|k| k.0),
                        p.display_name,
                        p.sync_models,
                    )
                    .await?;
                changed(s);
                let (sync, sync_error) = match sync {
                    Some(Ok(r)) => (Some(r), None),
                    Some(Err(e)) => (None, Some(e.message)),
                    None => (None, None),
                };
                encode_result::<api::AccountAdd>(&api::AccountAdded {
                    account,
                    sync,
                    sync_error,
                })
            }
            api::AccountList::NAME => {
                let p = decode_params::<api::AccountList>(params)?;
                encode_result::<api::AccountList>(&c.list_accounts(p.provider).await?)
            }
            api::AccountSetEnabled::NAME => {
                let p = decode_params::<api::AccountSetEnabled>(params)?;
                let info = c
                    .set_account_enabled(p.provider, p.alias, p.enabled)
                    .await?;
                changed(s);
                encode_result::<api::AccountSetEnabled>(&info)
            }
            api::AccountSetKey::NAME => {
                let p = decode_params::<api::AccountSetKey>(params)?;
                let info = c.set_account_key(p.provider, p.alias, p.api_key.0).await?;
                changed(s);
                encode_result::<api::AccountSetKey>(&info)
            }
            api::AccountRemove::NAME => {
                let p = decode_params::<api::AccountRemove>(params)?;
                let revoked = c.remove_account(p.provider, p.alias).await?;
                changed(s);
                encode_result::<api::AccountRemove>(&api::Revoked { revoked })
            }
            api::ModelSync::NAME => {
                let p = decode_params::<api::ModelSync>(params)?;
                let r = c.sync_models(p.provider, p.alias).await?;
                changed(s);
                encode_result::<api::ModelSync>(&r)
            }
            api::ModelAdd::NAME => {
                let p = decode_params::<api::ModelAdd>(params)?;
                let info = c
                    .add_model(
                        p.provider,
                        p.account,
                        p.model,
                        p.protocols,
                        p.context_window,
                    )
                    .await?;
                changed(s);
                encode_result::<api::ModelAdd>(&info)
            }
            api::ModelList::NAME => {
                let p = decode_params::<api::ModelList>(params)?;
                encode_result::<api::ModelList>(&c.list_models(p.provider, p.account).await?)
            }
            api::ModelSetEnabled::NAME => {
                let p = decode_params::<api::ModelSetEnabled>(params)?;
                let info = c.set_model_enabled(p.name, p.enabled).await?;
                changed(s);
                encode_result::<api::ModelSetEnabled>(&info)
            }
            api::ModelSetPrice::NAME => {
                let p = decode_params::<api::ModelSetPrice>(params)?;
                let info = c
                    .set_model_price(p.name, p.input, p.output, p.cache_read, p.cache_write)
                    .await?;
                encode_result::<api::ModelSetPrice>(&info)
            }
            api::ModelRemove::NAME => {
                let p = decode_params::<api::ModelRemove>(params)?;
                let revoked = c.remove_model(p.name).await?;
                changed(s);
                encode_result::<api::ModelRemove>(&api::Revoked { revoked })
            }
            api::RouteStatus::NAME => encode_result::<api::RouteStatus>(&c.route_status()),
            api::RouteVirtuals::NAME => encode_result::<api::RouteVirtuals>(&c.virtuals().await?),
            api::BenchRun::NAME => {
                let p = decode_params::<api::BenchRun>(params)?;
                let results = c.run_bench(p.names).await?;
                changed(s);
                encode_result::<api::BenchRun>(&results)
            }
            api::ModelSetTier::NAME => {
                let p = decode_params::<api::ModelSetTier>(params)?;
                let info = c.set_model_tier(p.name, p.tier).await?;
                changed(s);
                encode_result::<api::ModelSetTier>(&info)
            }
            api::PoolSave::NAME => {
                let p = decode_params::<api::PoolSave>(params)?;
                let info = c.save_pool(p.name, p.ordering, p.members).await?;
                changed(s);
                encode_result::<api::PoolSave>(&info)
            }
            api::PoolList::NAME => encode_result::<api::PoolList>(&c.list_pools().await?),
            api::PoolRemove::NAME => {
                let p = decode_params::<api::PoolRemove>(params)?;
                let revoked = c.remove_pool(p.name).await?;
                changed(s);
                encode_result::<api::PoolRemove>(&api::Revoked { revoked })
            }
            api::DiscoveryScan::NAME => encode_result::<api::DiscoveryScan>(&c.scan().await?),
            api::DiscoveryImport::NAME => {
                let p = decode_params::<api::DiscoveryImport>(params)?;
                let info = c.import(p.kind, p.alias).await?;
                changed(s);
                // Show usage right away instead of waiting for the next poll.
                let control = c.clone();
                let provider = info.provider.clone();
                tokio::spawn(async move {
                    let _ = control.usage_list(Some(provider), true).await;
                });
                encode_result::<api::DiscoveryImport>(&info)
            }
            api::UsageList::NAME => {
                let p = decode_params::<api::UsageList>(params)?;
                encode_result::<api::UsageList>(&c.usage_list(p.provider, p.refresh).await?)
            }
            api::AccountSetPlan::NAME => {
                let p = decode_params::<api::AccountSetPlan>(params)?;
                let info = c
                    .set_account_plan(
                        p.provider,
                        p.alias,
                        p.name,
                        p.price_usd_monthly,
                        p.renews_at_ms,
                    )
                    .await?;
                encode_result::<api::AccountSetPlan>(&info)
            }
            api::AccountSetReserve::NAME => {
                let p = decode_params::<api::AccountSetReserve>(params)?;
                let info = c
                    .set_account_reserve(p.provider, p.alias, p.reserve)
                    .await?;
                encode_result::<api::AccountSetReserve>(&info)
            }
            api::RequestsRecent::NAME => {
                let p = decode_params::<api::RequestsRecent>(params)?;
                encode_result::<api::RequestsRecent>(&c.recent_requests(p).await?)
            }
            api::StatsSummary::NAME => {
                let p = decode_params::<api::StatsSummary>(params)?;
                encode_result::<api::StatsSummary>(&c.stats(p.since_ms, p.group_by).await?)
            }
            api::PriceCompare::NAME => {
                let p = decode_params::<api::PriceCompare>(params)?;
                let now = loadout_storage::now_ms();
                let groups = s
                    .db(move |db| crate::prices::compare_from_db(db, p.model.as_deref(), now))
                    .await?;
                encode_result::<api::PriceCompare>(&groups)
            }
            api::AccountCheck::NAME => {
                let p = decode_params::<api::AccountCheck>(params)?;
                let r = c.check_account(p.provider, p.alias, p.models).await?;
                changed(s);
                encode_result::<api::AccountCheck>(&r)
            }
            other => Err(ErrorBody::new(
                ErrorCode::MethodNotFound,
                format!("unknown method {other}"),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> Arc<DaemonState> {
        let (events, _) = broadcast::channel(16);
        let db = Arc::new(Database::open_in_memory().expect("db"));
        let control = Arc::new(ControlPlane::new(
            db.clone(),
            Arc::new(loadout_secrets::InMemorySecretStore::new()),
            loadout_providers::Registry::builtin(),
            Arc::default(),
            loadout_provider_sdk::HttpClient::new(&Default::default()).expect("http"),
            std::path::PathBuf::from("."),
        ));
        Arc::new(DaemonState {
            db,
            control,
            keys: Arc::default(),
            events,
            shutdown: CancellationToken::new(),
            started_at_ms: 1,
            gateway_addr: SocketAddr::from(([127, 0, 0, 1], 8317)),
            data_dir: PathBuf::from("x"),
            recovered_from: None,
        })
    }

    async fn call<M: Method>(d: &Dispatcher, params: &M::Params) -> Result<M::Result, ErrorBody> {
        let value = d
            .handle(M::NAME, serde_json::to_value(params).expect("params"))
            .await?;
        Ok(serde_json::from_value(value).expect("result"))
    }

    #[tokio::test]
    async fn create_authenticates_and_revoke_removes_access() {
        let d = Dispatcher { state: state() };
        let mut events = d.state.events.subscribe();

        let created = call::<api::ClientCreate>(
            &d,
            &api::ClientCreateParams {
                name: "  trading  ".into(),
            },
        )
        .await
        .expect("create");
        assert_eq!(created.client.name, "trading");
        assert!(created.key.starts_with(&created.client.key_prefix));
        assert_eq!(
            events.try_recv().expect("event").name,
            events::CLIENTS_CHANGED
        );

        let headers = key_headers(&created.key);
        assert!(d.state.keys.authenticate(&headers).is_ok());

        let revoked = call::<api::ClientRevoke>(
            &d,
            &api::ClientNameParams {
                name: "trading".into(),
            },
        )
        .await
        .expect("revoke");
        assert!(revoked.revoked);
        assert!(d.state.keys.authenticate(&headers).is_err());

        let list = call::<api::ClientList>(&d, &()).await.expect("list");
        assert_eq!(list.len(), 1);
        assert!(!list[0].enabled);
    }

    fn key_headers(key: &str) -> axum::http::HeaderMap {
        let mut h = axum::http::HeaderMap::new();
        h.insert("x-api-key", key.parse().expect("header"));
        h
    }

    #[tokio::test]
    async fn rejects_bad_client_names_and_duplicates() {
        let d = Dispatcher { state: state() };
        for bad in ["", "   ", "a\nb", &"x".repeat(65)] {
            let err = call::<api::ClientCreate>(&d, &api::ClientCreateParams { name: bad.into() })
                .await
                .expect_err("invalid");
            assert_eq!(err.code, ErrorCode::InvalidParams, "{bad:?}");
        }
        let params = api::ClientCreateParams { name: "dup".into() };
        call::<api::ClientCreate>(&d, &params).await.expect("first");
        let err = call::<api::ClientCreate>(&d, &params)
            .await
            .expect_err("duplicate");
        assert_eq!(err.code, ErrorCode::Conflict);
    }

    #[tokio::test]
    async fn settings_validate_and_reset() {
        let d = Dispatcher { state: state() };
        let set = call::<api::SettingsSet>(
            &d,
            &api::SettingSetParams {
                key: settings::GATEWAY_PORT.into(),
                value: " 9100 ".into(),
            },
        )
        .await
        .expect("set");
        assert_eq!(set.value, "9100");
        assert!(set.restart_required);

        let bad = call::<api::SettingsSet>(
            &d,
            &api::SettingSetParams {
                key: settings::GATEWAY_PORT.into(),
                value: "70000".into(),
            },
        )
        .await
        .expect_err("invalid");
        assert_eq!(bad.code, ErrorCode::InvalidParams);

        let unknown = call::<api::SettingsSet>(
            &d,
            &api::SettingSetParams {
                key: "no.such".into(),
                value: "1".into(),
            },
        )
        .await
        .expect_err("unknown");
        assert_eq!(unknown.code, ErrorCode::InvalidParams);

        let reset = call::<api::SettingsReset>(
            &d,
            &api::SettingKeyParams {
                key: settings::GATEWAY_PORT.into(),
            },
        )
        .await
        .expect("reset");
        assert!(reset.is_default);
        assert_eq!(reset.value, "8317");
    }

    #[tokio::test]
    async fn shutdown_cancels_token_and_unknown_methods_fail() {
        let d = Dispatcher { state: state() };
        call::<api::Shutdown>(&d, &()).await.expect("shutdown");
        assert!(d.state.shutdown.is_cancelled());

        let err = d.handle("nope", Value::Null).await.expect_err("unknown");
        assert_eq!(err.code, ErrorCode::MethodNotFound);
    }

    #[tokio::test]
    async fn price_compare_lists_priced_models() {
        let d = Dispatcher { state: state() };
        let db = &d.state.db;
        let p = db
            .providers()
            .create(&loadout_storage::NewProvider {
                slug: "acme",
                kind: "openai-compatible",
                display_name: "Acme",
                base_url: None,
                config_json: "{}",
            })
            .expect("provider");
        let a = db
            .accounts()
            .create(&loadout_storage::NewAccount {
                provider_id: &p.id,
                alias: "main",
                display_name: None,
                account_type: "api_key",
            })
            .expect("account");
        db.account_models()
            .add_manual(
                &a.id,
                &loadout_storage::ModelInput {
                    upstream_model_id: "vendor/m-20250101".into(),
                    protocols_json: r#"["chat"]"#.into(),
                    capabilities_json: "[]".into(),
                    input_price: Some(1.0),
                    output_price: Some(5.0),
                    ..Default::default()
                },
            )
            .expect("model");

        let groups = call::<api::PriceCompare>(
            &d,
            &api::PriceCompareParams {
                model: Some("m".into()),
            },
        )
        .await
        .expect("compare");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].key, "m");
        let offer = &groups[0].offers[0];
        assert_eq!(offer.name, "acme/main/vendor/m-20250101");
        assert_eq!(offer.price_source.as_deref(), Some("user"));
        // Default mix 75/25: 0.75 * 1 + 0.25 * 5 = 2.
        assert!((offer.blended_per_million.expect("known") - 2.0).abs() < 1e-9);
    }
}
