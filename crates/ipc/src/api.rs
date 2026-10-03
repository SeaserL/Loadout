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
    /// Set when the database was damaged at startup and moved to this path; the
    /// configuration can be restored from `<data dir>\backups\auto`.
    #[serde(default)]
    pub recovered_from: Option<String>,
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
    #[serde(default)]
    pub policy: ClientPolicy,
    /// Usage today (UTC), counted for the policy's daily limits.
    #[serde(default)]
    pub tokens_today: u64,
    #[serde(default)]
    pub cost_today_usd: f64,
}

/// What a client key may do (ADR-011 §2, ExecPlan 006). Empty = no restrictions.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ClientPolicy {
    /// Model used when the request names no Loadout model (e.g. a tool that always sends
    /// `gpt-4o`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    /// Exact model names or prefixes ending in `*`; empty allows all.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_models: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily_token_limit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily_cost_limit_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requests_per_minute: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClientPolicyParams {
    pub name: String,
    pub policy: ClientPolicy,
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
method!(
    ClientSetPolicy,
    "client.set_policy",
    ClientPolicyParams,
    ClientInfo
);

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
    /// Values the `usage_source` setting accepts besides `auto` and `off` (ExecPlan 008).
    #[serde(default)]
    pub usage_sources: Vec<String>,
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

/// Merges `settings` into the provider's settings; a `null` value removes the key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderSettingsParams {
    pub slug: String,
    pub settings: serde_json::Value,
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
    /// The owner confirmed a lower-trust transport for this account (ADR-012).
    #[serde(default)]
    pub transport_opt_in: bool,
    /// Expiry of a Loadout-managed OAuth login (refreshed automatically).
    #[serde(default)]
    pub login_expires_at_ms: Option<i64>,
}

/// Starts a Loadout-managed device-code login (ADR-012). The account is created (or its
/// tokens replaced) once the user approves in the browser.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OAuthStartParams {
    pub provider: String,
    pub alias: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OAuthStart {
    pub login_id: String,
    /// Open this page and enter `user_code`.
    pub verification_url: String,
    pub user_code: String,
    pub expires_in_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OAuthStatusParams {
    pub login_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OAuthStatus {
    /// `pending`, `done` or `failed`.
    pub state: String,
    pub account: Option<AccountInfo>,
    pub message: Option<String>,
}

/// Turns a lower-trust transport on or off. Turning it on needs `confirm` equal to the
/// account alias (the second confirmation step, ADR-012 §1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountTransportParams {
    pub provider: String,
    pub alias: String,
    pub enabled: bool,
    #[serde(default)]
    pub confirm: Option<String>,
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
    /// Latency tier pinned by the user (`ULTRA_FAST`, `FAST`, `NORMAL`).
    #[serde(default)]
    pub tier_override: Option<String>,
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
    /// `ULTRA_FAST`, `FAST`, `NORMAL` or `UNMEASURED` (ExecPlan 004).
    #[serde(default)]
    pub tier: String,
    #[serde(default)]
    pub tier_pinned: bool,
    #[serde(default)]
    pub ttft_p50_ms: Option<u32>,
    #[serde(default)]
    pub ttft_p90_ms: Option<u32>,
    /// Median output tokens per second.
    #[serde(default)]
    pub tps: Option<f32>,
    #[serde(default)]
    pub samples: usize,
    #[serde(default)]
    pub last_sample_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VirtualInfo {
    /// `loadout/<name>`.
    pub name: String,
    /// `builtin` or `pool`.
    pub kind: String,
    /// Pools: `manual` or `latency`.
    pub ordering: Option<String>,
    /// Routes in selection order (`provider/account/model`).
    pub routes: Vec<String>,
}

// ---- latency (ExecPlan 004) --------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BenchParams {
    /// Model names (`provider/account/model`, `provider/model`, `loadout/<name>`); empty =
    /// every route that is not measured yet.
    #[serde(default)]
    pub names: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BenchResult {
    pub name: String,
    pub protocol: String,
    pub ok: bool,
    pub ttft_ms: Option<u64>,
    pub duration_ms: u64,
    pub output_tokens: Option<u64>,
    pub error: Option<String>,
    /// Tier after this run.
    pub tier: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelTierParams {
    pub name: String,
    /// `ultra-fast`, `fast`, `normal`; `None` returns to measured.
    pub tier: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PoolSaveParams {
    pub name: String,
    /// `manual` (members in order) or `latency`.
    pub ordering: String,
    /// `provider/account/model` names.
    pub members: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PoolRef {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PoolInfo {
    /// Without the `loadout/` prefix.
    pub name: String,
    pub ordering: String,
    /// Members as `provider/account/model`, in the user's order.
    pub members: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecentParams {
    pub limit: usize,
    /// Client name.
    #[serde(default)]
    pub client: Option<String>,
    /// Substring of the requested model or route.
    #[serde(default)]
    pub search: Option<String>,
    /// `ok` or `error`.
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub since_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StatsParams {
    pub since_ms: i64,
    /// `model`, `route`, `provider`, `client` or `day` (UTC).
    pub group_by: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StatsEntry {
    /// Model / route / provider / client name / `YYYY-MM-DD`.
    pub key: String,
    pub requests: i64,
    pub errors: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_usd: f64,
    pub avg_ttft_ms: Option<f64>,
    pub avg_tps: Option<f64>,
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
method!(
    ProviderSetSettings,
    "provider.set_settings",
    ProviderSettingsParams,
    ProviderInfo
);
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
method!(
    AccountOAuthStart,
    "account.oauth_start",
    OAuthStartParams,
    OAuthStart
);
method!(
    AccountOAuthStatus,
    "account.oauth_status",
    OAuthStatusParams,
    OAuthStatus
);
method!(
    AccountSetTransport,
    "account.set_transport",
    AccountTransportParams,
    AccountInfo
);
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
method!(RouteVirtuals, "route.virtuals", (), Vec<VirtualInfo>);
method!(BenchRun, "bench.run", BenchParams, Vec<BenchResult>);
method!(ModelSetTier, "model.set_tier", ModelTierParams, ModelInfo);
method!(PoolSave, "pool.save", PoolSaveParams, PoolInfo);
method!(PoolList, "pool.list", (), Vec<PoolInfo>);
method!(PoolRemove, "pool.remove", PoolRef, Revoked);
method!(StatsSummary, "stats.summary", StatsParams, Vec<StatsEntry>);

// ---- backup (ExecPlan 006 §3.4) -----------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BackupPathParams {
    /// Target file; default: `<data dir>\backups\…`.
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BackupResult {
    pub path: String,
    pub providers: usize,
    pub accounts: usize,
    pub models: usize,
    pub pools: usize,
    pub clients: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImportParams {
    pub path: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ImportReport {
    pub settings: usize,
    pub providers: usize,
    pub accounts: usize,
    pub models: usize,
    pub pools: usize,
    pub clients: usize,
    /// Items that already existed or could not be imported.
    pub skipped: Vec<String>,
    /// `provider/account` names whose API key must be entered again.
    pub accounts_needing_keys: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DatabaseBackup {
    pub path: String,
}

method!(
    BackupExport,
    "backup.export",
    BackupPathParams,
    BackupResult
);
method!(BackupImport, "backup.import", ImportParams, ImportReport);
method!(
    BackupDatabase,
    "backup.database",
    BackupPathParams,
    DatabaseBackup
);
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

// ---- price comparison (ExecPlan 011) ---------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PriceCompareParams {
    /// Upstream model id or a fragment of its normalized key; `None` compares every model.
    #[serde(default)]
    pub model: Option<String>,
}

/// Token mix used to blend prices. `input_tokens` includes cached tokens.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceTokenMix {
    /// `model` (your last 30 days on this model), `all` (your last 30 days on every
    /// model) or `default` (75% input / 25% output, no history).
    pub source: String,
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
}

/// USD per million tokens; `None` = unknown (never treated as free).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PriceOffer {
    /// `provider/account/model`.
    pub name: String,
    pub provider: String,
    pub account: String,
    pub upstream_model: String,
    /// Both the account and the model are enabled.
    pub enabled: bool,
    pub input_price: Option<f64>,
    pub output_price: Option<f64>,
    pub cache_read_price: Option<f64>,
    pub cache_write_price: Option<f64>,
    /// `user`, `catalog` or `None` (unknown).
    pub price_source: Option<String>,
    pub price_updated_at_ms: Option<i64>,
    /// Cost of one million tokens at the group's token mix; `None` when input or output
    /// price is unknown.
    pub blended_per_million: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PriceGroup {
    /// Normalized model key shared by every offer.
    pub key: String,
    pub mix: PriceTokenMix,
    /// Cheapest first; offers with unknown prices last.
    pub offers: Vec<PriceOffer>,
}

method!(
    PriceCompare,
    "price.compare",
    PriceCompareParams,
    Vec<PriceGroup>
);

// ---- notifications (demo, ExecPlan 012) --------------------------------------------------

/// Event carrying a [`Notification`] for the tray app to show as a Windows toast.
pub const NOTIFY_EVENT: &str = "notify";

/// A condition the user should hear about. The daemon emits each `(kind, key)` at most once
/// per de-duplication period.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    /// `quota_low`, `balance_low`, `auth_failed`, `login_expiring` or `client_cost_limit`.
    pub kind: String,
    /// What the condition is about (`provider/account`, `provider/account/window`, client).
    pub key: String,
    /// English text; clients may localize from `kind`, `subject`, `detail` and `value`.
    pub title: String,
    pub body: String,
    /// `provider/account` or the client name.
    pub subject: String,
    /// Usage window label, when relevant.
    #[serde(default)]
    pub detail: Option<String>,
    /// Percent left, balance, hours left or cost limit, depending on `kind`.
    #[serde(default)]
    pub value: Option<f64>,
}

// ---- account checks (ExecPlan 014) ----------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountCheckParams {
    pub provider: String,
    pub alias: String,
    /// Also send a 1-token request to every enabled model of the account (uses a little
    /// quota).
    #[serde(default)]
    pub models: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountCheckResult {
    /// `provider/account`.
    pub account: String,
    /// Result of the catalog (model list) call: `ok`, `invalid_key`, `account_disabled`,
    /// `rate_limited`, `no_credit`, `unreachable`, `upstream_error`, `unsupported` (the
    /// provider has no catalog call) or `missing_key`.
    pub status: String,
    pub http_status: Option<u16>,
    /// Sanitized upstream message or hint.
    pub message: Option<String>,
    /// Models the catalog listed (status `ok`).
    pub models_listed: Option<usize>,
    pub duration_ms: u64,
    /// The account's auth failures (including terminal ones) were cleared by this check.
    pub cleared: bool,
    /// Per-model probes (only with `models: true`).
    pub models: Vec<ModelCheck>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelCheck {
    /// `provider/account/model`.
    pub name: String,
    pub protocol: String,
    /// The model answered (possibly without visible output within the token budget).
    pub ok: bool,
    pub ttft_ms: Option<u64>,
    pub duration_ms: u64,
    pub error: Option<String>,
    /// `clean`, `truncated`, `malformed` or `empty`; `None` when no stream was received.
    pub stream: Option<String>,
}

method!(
    /// Tests an account's credentials off the hot path (ExecPlan 014).
    AccountCheck,
    "account.check",
    AccountCheckParams,
    AccountCheckResult
);
