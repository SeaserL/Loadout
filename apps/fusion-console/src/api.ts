// SPDX-License-Identifier: Apache-2.0
//
// Typed wrappers over the daemon IPC (types mirror crates/ipc/src/api.rs). The web UI
// never talks to providers or the gateway directly (Harness §22).

export class IpcFailure extends Error {
  readonly code: string;
  constructor(code: string, message: string) {
    super(message);
    this.code = code;
  }
}

let csrf: string | null = null;
async function csrfToken() {
  if (!csrf) {
    const r = await fetch("/api/auth/csrf", { credentials: "same-origin", cache: "no-store" });
    if (r.ok) csrf = ((await r.json()) as { token?: string }).token ?? null;
  }
  return csrf;
}

export async function call<T>(method: string, params: unknown = null): Promise<T> {
  for (let attempt = 0; ; attempt++) {
    let response: Response;
    try {
      response = await fetch("/api/loadout/rpc", {
        method: "POST",
        credentials: "same-origin",
        cache: "no-store",
        redirect: "error",
        headers: { "content-type": "application/json", "x-omniroute-csrf": (await csrfToken()) ?? "" },
        body: JSON.stringify({ method, params }),
      });
    } catch {
      throw new IpcFailure("not_running", "连不上本机网关。");
    }
    if (response.status === 401) throw new IpcFailure("no_session", "请从桌面应用打开 Loadout。");
    if (response.status === 403 && attempt === 0) {
      csrf = null;
      continue;
    }
    if (!response.ok) throw new IpcFailure("ipc", `网关返回 ${response.status}`);
    const data = (await response.json()) as { result?: T; error?: { code: string; message: string } };
    if (data.error) throw new IpcFailure(data.error.code, data.error.message);
    return data.result as T;
  }
}

/** A native OmniRoute management read or write, for the resource pages. */
export async function native<T>(path: string, method = "GET", body?: unknown): Promise<T> {
  const response = await fetch(path, {
    method,
    credentials: "same-origin",
    cache: "no-store",
    redirect: "error",
    headers: { "content-type": "application/json", "x-omniroute-csrf": (await csrfToken()) ?? "" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!response.ok) {
    let message = `网关返回 ${response.status}`;
    try {
      const data = (await response.json()) as { error?: string | { message?: string } };
      message = (typeof data.error === "string" ? data.error : data.error?.message) ?? message;
    } catch {
      // Keep the status text.
    }
    throw new IpcFailure("native", message);
  }
  return (await response.json()) as T;
}

// ---- types --------------------------------------------------------------------------------

export interface UpdateStatus {
  current_version: string;
  latest_version: string | null;
  phase: string;
  progress: { done: number; total: number } | null;
  size: number | null;
  release_url: string | null;
  checked_at_ms: number | null;
  error: string | null;
  last_result: { status: "installed" | "failed"; from: string; to: string; at_ms: number } | null;
  auto_check: boolean;
}

export interface DaemonStatus {
  version: string;
  pid: number;
  started_at_ms: number;
  data_dir: string;
  schema_version: number;
  gateway: { addr: string; listening: boolean };
  recovered_from?: string | null;
  network?: string;
  log_retention_days?: number;
}

export interface ClientPolicy {
  allowed_models?: string[];
  allowed_accounts?: string[];
  daily_cost_limit_usd?: number | null;
  weekly_cost_limit_usd?: number | null;
  requests_per_minute?: number | null;
}

export interface ClientInfo {
  id: string;
  name: string;
  key_prefix: string;
  enabled: boolean;
  created_at_ms: number;
  revoked_at_ms: number | null;
  policy: ClientPolicy;
  tokens_today: number;
  cost_today_usd: number;
  /** This key may open Loadout's MCP endpoint. */
  mcp: boolean;
}

export interface ClientTool {
  id: string;
  name: string;
  installed: boolean;
  config_path: string;
  usage: string | null;
}

export interface ToolConnected {
  client: ClientInfo;
  tool: string;
  written: string;
  backup: string | null;
  usage: string | null;
}

export interface StatsEntry {
  key: string;
  requests: number;
  errors: number;
  input_tokens: number;
  output_tokens: number;
  cost_usd: number;
  avg_ttft_ms: number | null;
  avg_tps: number | null;
  // Demo: transparency (absent from older daemons)
  cache_read_tokens?: number;
  cache_write_tokens?: number;
  cache_hit_rate?: number | null;
  cache_savings_usd?: number;
}

export interface LogFilter {
  limit: number;
  client?: string | null;
  search?: string | null;
  status?: string | null;
  since_ms?: number | null;
}

export interface ClientCreated {
  client: ClientInfo;
  key: string;
}

export interface Setting {
  key: string;
  name?: string;
  value: string;
  default: string;
  is_default: boolean;
  description: string;
  restart_required: boolean;
}

export type FeatureGroup = "save" | "safety" | "extend";
export interface FeatureOption {
  key: string;
  /** "model": pick one of this install's models; "choice": a short fixed set. */
  kind: "choice" | "model";
  value: string;
  choices: { value: string; available?: boolean }[];
}
export interface Feature {
  key: string;
  group: FeatureGroup;
  enabled: boolean;
  /** False when the feature needs the AI pack and this install does not have it. */
  available: boolean;
  /** True for features that keep request or reply text on this computer (ADR-008). */
  stores_content: boolean;
  options: FeatureOption[];
}
export interface FeatureList {
  edition: "full" | "lite";
  ai_pack: {
    installed: boolean;
    version: string | null;
    phase: "absent" | "downloading" | "installing" | "installed" | "error";
    progress: { done: number; total: number } | null;
    error: string | null;
  };
  features: Feature[];
}

export interface ProviderKind {
  kind: string;
  display_name: string;
  fixed_slug: string | null;
  default_base_url: string | null;
  auth: "api_key" | "optional_api_key" | "external_managed" | "oauth" | "browser_login";
  protocols: string[];
  requires_transport_opt_in?: boolean;
  drivers: string[];
  /** Values `usage_source` accepts besides "auto" and "off". */
  usage_sources: string[];
  group: "platform" | "relay" | "subscription" | "local" | "custom";
  hint?: string;
}

export interface ProviderInfo {
  slug: string;
  kind: string;
  display_name: string;
  base_url: string | null;
  settings: Record<string, unknown>;
  accounts: number;
}

export interface AccountInfo {
  provider: string;
  alias: string;
  display_name: string | null;
  account_type: string;
  enabled: boolean;
  position: number;
  has_key: boolean;
  models: number;
  enabled_models: number;
  /** The owner turned on a lower-trust transport (ADR-012). */
  transport_opt_in: boolean;
  /** Expiry of a Loadout-managed sign-in (refreshed automatically). */
  login_expires_at_ms: number | null;
  /** Models Loadout turned off because the plan does not include them (ExecPlan 022). */
  unavailable_models?: string[];
}

export interface OAuthStart {
  login_id: string;
  verification_url: string;
  user_code: string;
  expires_in_secs: number;
}

export interface OAuthStatus {
  state: "pending" | "done" | "failed";
  account: AccountInfo | null;
  message: string | null;
}

export interface SyncResult {
  added: number;
  updated: number;
  disabled: number;
  total: number;
  ambiguous: string[];
}

export interface AccountAdded {
  account: AccountInfo;
  sync: SyncResult | null;
  sync_error: string | null;
}

export interface ModelInfo {
  name: string;
  provider: string;
  account: string;
  upstream_model: string;
  protocols: string[];
  enabled: boolean;
  source: string;
  context_window: number | null;
  input_price: number | null;
  output_price: number | null;
  tier_override: string | null;
  /** Why Loadout turned the model off itself, e.g. "not_in_plan" (ExecPlan 022). */
  disabled_reason?: string | null;
}

export interface TargetStatus {
  name: string;
  protocol: string;
  trust: string;
  state: string;
  unavailable_until_ms: number | null;
  consecutive_failures: number;
  in_flight: number;
  successes: number;
  failures: number;
  last_error: string | null;
  /** Last failure, and last recovery from a blocking state (ExecPlan 021). */
  last_failure_ms?: number | null;
  recovered_at_ms?: number | null;
  tier: string;
  tier_pinned: boolean;
  ttft_p50_ms: number | null;
  ttft_p90_ms: number | null;
  tps: number | null;
  samples: number;
  last_sample_ms: number | null;
}

export interface VirtualInfo {
  name: string;
  kind: "builtin" | "pool";
  ordering: string | null;
  routes: string[];
  enabled?: boolean;
}

export interface BenchResult {
  name: string;
  protocol: string;
  ok: boolean;
  ttft_ms: number | null;
  duration_ms: number;
  output_tokens: number | null;
  error: string | null;
  tier: string;
}

export interface ModelCheck {
  name: string;
  protocol: string;
  ok: boolean;
  ttft_ms: number | null;
  duration_ms: number;
  error: string | null;
  stream: string | null;
}

export interface AccountCheckResult {
  account: string;
  /** ok | invalid_key | account_disabled | rate_limited | no_credit | unreachable | upstream_error | unsupported | missing_key */
  status: string;
  http_status: number | null;
  message: string | null;
  models_listed: number | null;
  duration_ms: number;
  cleared: boolean;
  models: ModelCheck[];
}

export interface PoolInfo {
  name: string;
  /** manual, latency or a native OmniRoute strategy. */
  ordering: string;
  members: string[];
  enabled?: boolean;
}

/** A request in flight. `bytes` is what has reached the client so far. */
export interface LiveRequest {
  id: string;
  client: string;
  provider: string;
  account: string;
  model: string;
  started_ms: number;
  answering: boolean;
  bytes: number;
}

/** A request that was just logged. */
export interface LiveDone {
  seq: number;
  client: string;
  provider: string;
  account: string;
  model: string;
  ok: boolean;
  input_tokens: number;
  output_tokens: number;
  duration_ms: number;
}

export interface FlowLink {
  client: string;
  provider: string;
  account: string;
  model: string;
  combo: string;
  requests: number;
  errors: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cost_usd: number | null;
}

export interface RequestSummary {
  id?: string;
  ts_ms: number;
  client: string | null;
  requested_model: string;
  route: string | null;
  protocol_in: string;
  protocol_out: string | null;
  mode: string | null;
  stream: boolean;
  status: string;
  error_category: string | null;
  attempts: number;
  input_tokens: number | null;
  output_tokens: number | null;
  ttft_ms: number | null;
  duration_ms: number;
  cost_usd: number | null;
  /** Routing decision (metadata only), e.g. "a/b/m: skipped (quota) → c/d/m: ok". */
  decision?: string | null;
}

export interface UsageWindow {
  scope: string;
  label: string;
  unit: string;
  used: number | null;
  limit: number | null;
  remaining_percent: number | null;
  window_seconds: number | null;
  resets_at_ms: number | null;
}

export interface UsageBalance {
  currency: string;
  amount: number;
  included: number | null;
  topup: number | null;
  granted: number | null;
}

export interface UsagePlan {
  name: string | null;
  price_usd_monthly: number | null;
  renews_at_ms: number | null;
  auto_renew: boolean | null;
}

export interface SpendInfo {
  requests: number;
  input_tokens: number;
  output_tokens: number;
  cost_usd: number;
}

export interface AccountUsage {
  provider: string;
  alias: string;
  account_type: string;
  supported: boolean;
  fetched_at_ms: number | null;
  source: string | null;
  confidence: string | null;
  error: string | null;
  account_label: string | null;
  plan: UsagePlan | null;
  windows: UsageWindow[];
  balances: UsageBalance[];
  spend_today: SpendInfo;
  spend_month: SpendInfo;
}

export interface DiscoveredTool {
  kind: string;
  display_name: string;
  installed: boolean;
  executable: string | null;
  logged_in: boolean;
  auth: string | null;
  detail: string | null;
  expired: boolean;
  config_path: string | null;
  imported: boolean;
  inference: boolean;
}

export interface Revoked {
  revoked: boolean;
}

// ---- methods ------------------------------------------------------------------------------

export const api = {
  status: () => call<DaemonStatus>("daemon.status"),

  clients: () => call<ClientInfo[]>("client.list"),
  createClient: (name: string) => call<ClientCreated>("client.create", { name }),
  revokeClient: (name: string) => call<Revoked>("client.revoke", { name }),
  setClientMcp: (name: string, enabled: boolean) => call<{ mcp: boolean }>("client.set_mcp", { name, enabled }),
  clientTools: () => call<ClientTool[]>("client.tools"),
  connectClientTool: (tool: string, model: string, mcp: boolean) =>
    call<ToolConnected>("client.connect_tool", { tool, model, mcp }),
  setClientPolicy: (name: string, policy: ClientPolicy) =>
    call<ClientInfo>("client.set_policy", { name, policy }),
  /** Days and hours are the viewer's own, not UTC. */
  updateStatus: () => call<UpdateStatus>("update.status"),
  updateCheck: () => call<UpdateStatus>("update.check"),
  updateDownload: () => call<UpdateStatus>("update.download"),
  updateInstall: () => call<UpdateStatus>("update.install"),
  updateSetAutoCheck: (enabled: boolean) => call<UpdateStatus>("update.set_auto_check", { enabled }),
  updateOpenNotes: () => call<unknown>("update.open_notes"),
  autostart: () => call<{ enabled: boolean }>("system.autostart"),
  setAutostart: (enabled: boolean) => call<{ enabled: boolean }>("system.set_autostart", { enabled }),
  stats: (since_ms: number, group_by: string, until_ms?: number) =>
    call<StatsEntry[]>("stats.summary", { since_ms, group_by, until_ms, tz_offset_min: -new Date().getTimezoneOffset() }),

  features: () => call<FeatureList>("features.list"),
  installAiPack: () => call<FeatureList["ai_pack"]>("update.install_pack"),
  setFeature: (key: string, enabled: boolean) => call<Feature>("features.set", { key, enabled }),
  setFeatureOption: (key: string, option: string, value: string) =>
    call<Feature>("features.set_option", { key, option, value }),
  settings: () => call<Setting[]>("settings.list"),
  setSetting: (key: string, value: string) => call<Setting>("settings.set", { key, value }),
  resetSetting: (key: string) => call<Setting>("settings.reset", { key }),

  providerKinds: () => call<ProviderKind[]>("provider.kinds"),
  providers: () => call<ProviderInfo[]>("provider.list"),
  addProvider: (p: {
    slug: string;
    kind: string;
    base_url: string | null;
    settings: Record<string, unknown>;
    display_name: string | null;
  }) => call<ProviderInfo>("provider.add", p),
  removeProvider: (slug: string) => call<Revoked>("provider.remove", { slug }),
  setProviderSettings: (slug: string, settings: Record<string, unknown>) =>
    call<ProviderInfo>("provider.set_settings", { slug, settings }),

  accounts: () => call<AccountInfo[]>("account.list", { provider: null }),
  addAccount: (p: { provider: string; alias: string; api_key: string | null; display_name: string | null }) =>
    call<AccountAdded>("account.add", { ...p, sync_models: true }),
  /** New order of a provider's accounts, also their failover order (ExecPlan 022). */
  /** Display order of the provider cards (ExecPlan 023); routing is unaffected. */
  reorderProviders: (slugs: string[]) => call<ProviderInfo[]>("provider.reorder", { slugs }),
  reorderAccounts: (provider: string, aliases: string[]) =>
    call<AccountInfo[]>("account.reorder", { provider, aliases }),
  setAccountEnabled: (provider: string, alias: string, enabled: boolean) =>
    call<AccountInfo>("account.set_enabled", { provider, alias, enabled }),
  setAccountKey: (provider: string, alias: string, api_key: string) =>
    call<AccountInfo>("account.set_key", { provider, alias, api_key }),
  removeAccount: (provider: string, alias: string) => call<Revoked>("account.remove", { provider, alias }),
  oauthStart: (provider: string, alias: string) => call<OAuthStart>("account.oauth_start", { provider, alias }),
  oauthStatus: (login_id: string) => call<OAuthStatus>("account.oauth_status", { login_id }),
  setTransport: (provider: string, alias: string, enabled: boolean, confirm: string | null) =>
    call<AccountInfo>("account.set_transport", { provider, alias, enabled, confirm }),
  setPlan: (p: {
    provider: string;
    alias: string;
    name: string | null;
    price_usd_monthly: number | null;
    renews_at_ms: number | null;
  }) => call<AccountInfo>("account.set_plan", p),

  models: () => call<ModelInfo[]>("model.list", { provider: null, account: null }),
  syncModels: (provider: string, alias: string) => call<SyncResult>("model.sync", { provider, alias }),
  checkAccount: (provider: string, alias: string, models = false) =>
    call<AccountCheckResult>("account.check", { provider, alias, models }),
  addModel: (p: { provider: string; account: string; model: string; protocols: string[]; context_window: number | null }) =>
    call<ModelInfo>("model.add", p),
  setModelEnabled: (name: string, enabled: boolean) => call<ModelInfo>("model.set_enabled", { name, enabled }),
  setModelsEnabled: (names: string[], enabled: boolean) =>
    call<{ updated: number }>("model.set_enabled_many", { names, enabled }),
  setModelPrice: (name: string, input: number | null, output: number | null) =>
    call<ModelInfo>("model.set_price", { name, input, output, cache_read: null, cache_write: null }),
  setModelTier: (name: string, tier: string | null) => call<ModelInfo>("model.set_tier", { name, tier }),
  removeModel: (name: string) => call<Revoked>("model.remove", { name }),

  routeStatus: () => call<TargetStatus[]>("route.status"),
  virtuals: () => call<VirtualInfo[]>("route.virtuals"),
  bench: (names: string[]) => call<BenchResult[]>("bench.run", { names }),
  pools: () => call<PoolInfo[]>("pool.list"),
  savePool: (name: string, ordering: string, members: string[]) =>
    call<PoolInfo>("pool.save", { name, ordering, members }),
  removePool: (name: string) => call<Revoked>("pool.remove", { name }),

  recent: (filter: LogFilter) => call<RequestSummary[]>("requests.recent", filter),
  usage: (refresh: boolean) => call<AccountUsage[]>("usage.list", { provider: null, refresh }),
  scan: () => call<DiscoveredTool[]>("discovery.scan"),
  connectTool: (kind: string) => call<{ restarting: boolean }>("discovery.connect", { kind }),
  flow: (since_ms: number) => call<{ links: FlowLink[]; since_ms: number }>("stats.flow", { since_ms }),
  live: (after: number | null) =>
    call<{ now_ms: number; cursor: number; active: LiveRequest[]; done: LiveDone[] }>("stats.live", { after }),
  setVirtual: (name: string, enabled: boolean) => call<VirtualInfo[]>("route.virtual_set", { name, enabled }),
  backupDatabase: (path: string | null) => call<{ path: string }>("backup.database", { path }),
  openSignInPage: (url: string) => call<void>("system.open_url", { url }),
  stopDaemon: () => call<void>("daemon.stop"),
  restartDaemon: () => call<void>("daemon.restart"),
};
