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

use crate::settings;

const MAX_CLIENT_NAME: usize = 64;

#[derive(Debug)]
pub(crate) struct DaemonState {
    pub db: Arc<Database>,
    pub keys: Arc<ClientKeyIndex>,
    pub events: broadcast::Sender<Event>,
    pub shutdown: CancellationToken,
    pub started_at_ms: i64,
    pub gateway_addr: SocketAddr,
    pub data_dir: PathBuf,
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
        let entries = self.db(|db| db.clients().active_key_hashes()).await?;
        self.keys.replace(
            entries
                .into_iter()
                .map(|(hash, id)| (hash, ClientId::from(id.as_str()))),
        );
        Ok(())
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

fn client_info(c: Client) -> ClientInfo {
    ClientInfo {
        id: c.id.to_string(),
        name: c.name,
        key_prefix: c.key_prefix,
        enabled: c.enabled,
        created_at_ms: c.created_at,
        revoked_at_ms: c.revoked_at,
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
                    client: client_info(client),
                    key: key.key,
                })
            }

            api::ClientList::NAME => {
                let clients = s.db(|db| db.clients().list()).await?;
                encode_result::<api::ClientList>(&clients.into_iter().map(client_info).collect())
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
                encode_result::<api::SettingsReset>(&setting)
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
        Arc::new(DaemonState {
            db: Arc::new(Database::open_in_memory().expect("db")),
            keys: Arc::default(),
            events,
            shutdown: CancellationToken::new(),
            started_at_ms: 1,
            gateway_addr: SocketAddr::from(([127, 0, 0, 1], 8317)),
            data_dir: PathBuf::from("x"),
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
}
