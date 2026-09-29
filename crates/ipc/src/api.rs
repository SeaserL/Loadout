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

// ---- providers / accounts / models (ExecPlan 002 §5.6) ---------------------------------

/// An API key in transit from the CLI/UI to the daemon. `Debug` never shows it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ApiKey(pub String);

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiKey([REDACTED])")
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderKind {
    pub kind: String,
    pub display_name: String,
    pub fixed_slug: Option<String>,
    pub default_base_url: Option<String>,
    /// `api_key`, `optional_api_key` or `external_managed`.
    pub auth: String,
    pub protocols: Vec<String>,
    pub drivers: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub slug: String,
    pub kind: String,
    pub display_name: String,
    pub base_url: Option<String>,
    pub settings: serde_json::Value,
    pub accounts: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderAddParams {
    pub slug: String,
    pub kind: String,
    pub base_url: Option<String>,
    #[serde(default)]
    pub settings: serde_json::Value,
    pub display_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlugParams {
    pub slug: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountInfo {
    pub provider: String,
    pub alias: String,
    pub display_name: Option<String>,
    pub account_type: String,
    pub enabled: bool,
    pub position: i64,
    pub has_key: bool,
    pub models: usize,
    pub enabled_models: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountAddParams {
    pub provider: String,
    pub alias: String,
    pub api_key: Option<ApiKey>,
    pub display_name: Option<String>,
    /// Fetch the model list right away (default true).
    #[serde(default = "default_true")]
    pub sync_models: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountAdded {
    pub account: AccountInfo,
    /// Result of the initial model sync, or why it failed.
    pub sync: Option<SyncResult>,
    pub sync_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountRef {
    pub provider: String,
    pub alias: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountListParams {
    pub provider: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountEnableParams {
    pub provider: String,
    pub alias: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountKeyParams {
    pub provider: String,
    pub alias: String,
    pub api_key: ApiKey,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SyncResult {
    pub added: usize,
    pub updated: usize,
    pub disabled: usize,
    pub total: usize,
    /// Models whose id starts with another account's alias (ADR-010 §3).
    pub ambiguous: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    /// `provider/account/model`.
    pub name: String,
    pub provider: String,
    pub account: String,
    pub upstream_model: String,
    pub protocols: Vec<String>,
    pub enabled: bool,
    pub source: String,
    pub context_window: Option<i64>,
    pub input_price: Option<f64>,
    pub output_price: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelListParams {
    pub provider: Option<String>,
    pub account: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelAddParams {
    pub provider: String,
    pub account: String,
    pub model: String,
    #[serde(default)]
    pub protocols: Vec<String>,
    pub context_window: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelRef {
    /// `provider/account/model`.
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelEnableParams {
    pub name: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelPriceParams {
    pub name: String,
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TargetStatus {
    pub name: String,
    pub protocol: String,
    pub trust: String,
    pub state: String,
    pub unavailable_until_ms: Option<i64>,
    pub consecutive_failures: u32,
    pub in_flight: u32,
    pub successes: u64,
    pub failures: u64,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecentParams {
    pub limit: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RequestSummary {
    pub ts_ms: i64,
    pub client: Option<String>,
    pub requested_model: String,
    pub route: Option<String>,
    pub protocol_in: String,
    pub protocol_out: Option<String>,
    pub mode: Option<String>,
    pub stream: bool,
    pub status: String,
    pub error_category: Option<String>,
    pub attempts: i64,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub ttft_ms: Option<i64>,
    pub duration_ms: i64,
    pub cost_usd: Option<f64>,
}

// ---- usage (ExecPlan 003) ----------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageWindow {
    pub scope: String,
    pub label: String,
    /// `percent`, `requests`, `tokens`, `usd` or `credits`.
    pub unit: String,
    pub used: Option<f64>,
    pub limit: Option<f64>,
    /// 0–100, when computable.
    pub remaining_percent: Option<f64>,
    pub window_seconds: Option<i64>,
    pub resets_at_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageBalance {
    pub currency: String,
    pub amount: f64,
    pub included: Option<f64>,
    pub topup: Option<f64>,
    pub granted: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsagePlan {
    pub name: Option<String>,
    pub price_usd_monthly: Option<f64>,
    /// Billing renewal — not the quota reset (product doc §35).
    pub renews_at_ms: Option<i64>,
    pub auto_renew: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SpendInfo {
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_usd: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountUsage {
    pub provider: String,
    pub alias: String,
    pub account_type: String,
    /// The provider can report usage (otherwise only Loadout's own spend is shown).
    pub supported: bool,
    pub fetched_at_ms: Option<i64>,
    pub source: Option<String>,
    pub confidence: Option<String>,
    pub error: Option<String>,
    pub account_label: Option<String>,
    pub plan: Option<UsagePlan>,
    pub windows: Vec<UsageWindow>,
    pub balances: Vec<UsageBalance>,
    /// Estimated from Loadout's own request records (since local midnight / month start).
    pub spend_today: SpendInfo,
    pub spend_month: SpendInfo,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageListParams {
    pub provider: Option<String>,
    /// Refresh from upstreams before answering.
    #[serde(default)]
    pub refresh: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountPlanParams {
    pub provider: String,
    pub alias: String,
    pub name: Option<String>,
    pub price_usd_monthly: Option<f64>,
    pub renews_at_ms: Option<i64>,
}

// ---- discovery (Harness §12) --------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiscoveredTool {
    pub kind: String,
    pub display_name: String,
    pub installed: bool,
    pub executable: Option<String>,
    pub logged_in: bool,
    pub auth: Option<String>,
    /// Non-secret detail such as a plan name.
    pub detail: Option<String>,
    pub expired: bool,
    pub config_path: Option<String>,
    /// An account for this tool's login already exists.
    pub imported: bool,
    /// Loadout can route requests through this provider (false for subscriptions whose
    /// credentials only allow usage reading, ADR-009).
    pub inference: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiscoveryImportParams {
    pub kind: String,
    pub alias: Option<String>,
}

method!(DiscoveryScan, "discovery.scan", (), Vec<DiscoveredTool>);
method!(
    DiscoveryImport,
    "discovery.import",
    DiscoveryImportParams,
    AccountInfo
);

method!(UsageList, "usage.list", UsageListParams, Vec<AccountUsage>);
method!(
    AccountSetPlan,
    "account.set_plan",
    AccountPlanParams,
    AccountInfo
);

method!(ProviderKinds, "provider.kinds", (), Vec<ProviderKind>);
method!(ProviderAdd, "provider.add", ProviderAddParams, ProviderInfo);
method!(ProviderList, "provider.list", (), Vec<ProviderInfo>);
method!(ProviderRemove, "provider.remove", SlugParams, Revoked);
method!(AccountAdd, "account.add", AccountAddParams, AccountAdded);
method!(
    AccountList,
    "account.list",
    AccountListParams,
    Vec<AccountInfo>
);
method!(
    AccountSetEnabled,
    "account.set_enabled",
    AccountEnableParams,
    AccountInfo
);
method!(
    AccountSetKey,
    "account.set_key",
    AccountKeyParams,
    AccountInfo
);
method!(AccountRemove, "account.remove", AccountRef, Revoked);
method!(ModelSync, "model.sync", AccountRef, SyncResult);
method!(ModelAdd, "model.add", ModelAddParams, ModelInfo);
method!(ModelList, "model.list", ModelListParams, Vec<ModelInfo>);
method!(
    ModelSetEnabled,
    "model.set_enabled",
    ModelEnableParams,
    ModelInfo
);
method!(
    ModelSetPrice,
    "model.set_price",
    ModelPriceParams,
    ModelInfo
);
method!(ModelRemove, "model.remove", ModelRef, Revoked);
method!(RouteStatus, "route.status", (), Vec<TargetStatus>);
method!(
    RequestsRecent,
    "requests.recent",
    RecentParams,
    Vec<RequestSummary>
);

// ---- events ----------------------------------------------------------------------------

pub mod events {
    /// The daemon is about to stop.
    pub const DAEMON_STOPPING: &str = "daemon.stopping";
    /// The client list changed.
    pub const CLIENTS_CHANGED: &str = "clients.changed";
    /// A setting changed.
    pub const SETTINGS_CHANGED: &str = "settings.changed";
    /// Providers, accounts or models changed (the route table was rebuilt).
    pub const ROUTES_CHANGED: &str = "routes.changed";
}
