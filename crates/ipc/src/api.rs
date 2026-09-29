// SPDX-License-Identifier: Apache-2.0

//! Typed IPC methods. Adding a method: define a unit struct, implement [`Method`], handle it
//! in the daemon dispatcher, and (if needed) expose it in the CLI.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub trait Method {
    const NAME: &'static str;
    type Params: Serialize + DeserializeOwned + Send + Sync;
    type Result: Serialize + DeserializeOwned + Send;
}

macro_rules! method {
    ($(#[$meta:meta])* $ty:ident, $name:literal, $params:ty, $result:ty) => {
        $(#[$meta])*
        #[derive(Debug)]
        pub struct $ty;
        impl Method for $ty {
            const NAME: &'static str = $name;
            type Params = $params;
            type Result = $result;
        }
    };
}

// ---- handshake -------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HelloParams {
    pub client_name: String,
    pub client_version: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HelloResult {
    pub protocol: u32,
    pub daemon_version: String,
}

method!(Hello, "hello", HelloParams, HelloResult);

method!(
    /// Subscribes this connection to daemon events.
    EventsSubscribe,
    "events.subscribe",
    (),
    ()
);

// ---- daemon ----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GatewayStatus {
    /// Address the gateway is bound to, e.g. `127.0.0.1:8317`.
    pub addr: String,
    pub listening: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub version: String,
    pub pid: u32,
    pub started_at_ms: i64,
    pub data_dir: String,
    pub schema_version: usize,
    pub gateway: GatewayStatus,
}

method!(Status, "daemon.status", (), DaemonStatus);
method!(Shutdown, "daemon.shutdown", (), ());

// ---- clients (ADR-011) -----------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClientInfo {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub enabled: bool,
    pub created_at_ms: i64,
    pub revoked_at_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClientCreateParams {
    pub name: String,
}

/// Returned once at creation; `key` is never retrievable again.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct ClientCreated {
    pub client: ClientInfo,
    pub key: String,
}

impl std::fmt::Debug for ClientCreated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientCreated")
            .field("client", &self.client)
            .field("key", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClientNameParams {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Revoked {
    pub revoked: bool,
}

method!(
    ClientCreate,
    "client.create",
    ClientCreateParams,
    ClientCreated
);
method!(ClientList, "client.list", (), Vec<ClientInfo>);
method!(ClientRevoke, "client.revoke", ClientNameParams, Revoked);

// ---- settings --------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Setting {
    pub key: String,
    pub value: String,
    pub default: String,
    pub is_default: bool,
    pub description: String,
    /// Changing this setting takes effect only after the daemon restarts.
    pub restart_required: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettingSetParams {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettingKeyParams {
    pub key: String,
}

method!(SettingsList, "settings.list", (), Vec<Setting>);
method!(SettingsSet, "settings.set", SettingSetParams, Setting);
method!(SettingsReset, "settings.reset", SettingKeyParams, Setting);

// ---- events ----------------------------------------------------------------------------

pub mod events {
    /// The daemon is about to stop.
    pub const DAEMON_STOPPING: &str = "daemon.stopping";
    /// The client list changed.
    pub const CLIENTS_CHANGED: &str = "clients.changed";
    /// A setting changed.
    pub const SETTINGS_CHANGED: &str = "settings.changed";
}
