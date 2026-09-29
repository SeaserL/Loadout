// SPDX-License-Identifier: Apache-2.0
//
// Typed wrappers over the daemon IPC (types mirror crates/ipc/src/api.rs). The web UI
// never talks to providers or the gateway directly (Harness §22).

import { invoke } from "@tauri-apps/api/core";

export class IpcFailure extends Error {
  readonly code: string;
  constructor(code: string, message: string) {
    super(message);
    this.code = code;
  }
}

function toFailure(e: unknown): IpcFailure {
  if (e && typeof e === "object" && "code" in e && "message" in e) {
    const err = e as { code: string; message: string };
    return new IpcFailure(err.code, err.message);
  }
  return new IpcFailure("ipc", String(e));
}

export async function call<T>(method: string, params: unknown = null): Promise<T> {
  try {
    return await invoke<T>("ipc_call", { method, params });
  } catch (e) {
    throw toFailure(e);
  }
}

async function command<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(name, args);
  } catch (e) {
    throw toFailure(e);
  }
}

// ---- types --------------------------------------------------------------------------------

export interface DaemonStatus {
  version: string;
  pid: number;
  started_at_ms: number;
  data_dir: string;
  schema_version: number;
  gateway: { addr: string; listening: boolean };
}

export interface ClientInfo {
  id: string;
  name: string;
  key_prefix: string;
  enabled: boolean;
  created_at_ms: number;
  revoked_at_ms: number | null;
}

export interface ClientCreated {
  client: ClientInfo;
  key: string;
}

export interface Setting {
  key: string;
  value: string;
  default: string;
  is_default: boolean;
  description: string;
  restart_required: boolean;
}

export interface ProviderKind {
  kind: string;
  display_name: string;
  fixed_slug: string | null;
  default_base_url: string | null;
  auth: "api_key" | "optional_api_key" | "external_managed";
  protocols: string[];
  drivers: string[];
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

export interface PoolInfo {
  name: string;
  ordering: "manual" | "latency";
  members: string[];
}

export interface RequestSummary {
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

  accounts: () => call<AccountInfo[]>("account.list", { provider: null }),
  addAccount: (p: { provider: string; alias: string; api_key: string | null; display_name: string | null }) =>
    call<AccountAdded>("account.add", { ...p, sync_models: true }),
  setAccountEnabled: (provider: string, alias: string, enabled: boolean) =>
    call<AccountInfo>("account.set_enabled", { provider, alias, enabled }),
  setAccountKey: (provider: string, alias: string, api_key: string) =>
    call<AccountInfo>("account.set_key", { provider, alias, api_key }),
  removeAccount: (provider: string, alias: string) => call<Revoked>("account.remove", { provider, alias }),
  setPlan: (p: {
    provider: string;
    alias: string;
    name: string | null;
    price_usd_monthly: number | null;
    renews_at_ms: number | null;
  }) => call<AccountInfo>("account.set_plan", p),

  models: () => call<ModelInfo[]>("model.list", { provider: null, account: null }),
  syncModels: (provider: string, alias: string) => call<SyncResult>("model.sync", { provider, alias }),
  addModel: (p: { provider: string; account: string; model: string; protocols: string[]; context_window: number | null }) =>
    call<ModelInfo>("model.add", p),
  setModelEnabled: (name: string, enabled: boolean) => call<ModelInfo>("model.set_enabled", { name, enabled }),
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

  recent: (limit: number) => call<RequestSummary[]>("requests.recent", { limit }),
  usage: (refresh: boolean) => call<AccountUsage[]>("usage.list", { provider: null, refresh }),
  scan: () => call<DiscoveredTool[]>("discovery.scan"),
  importTool: (kind: string, alias: string | null) => call<AccountInfo>("discovery.import", { kind, alias }),

  startDaemon: () => command<DaemonStatus>("daemon_start"),
  stopDaemon: () => command<void>("daemon_stop"),
  autostart: () => command<boolean>("autostart_get"),
  setAutostart: (enabled: boolean) => command<boolean>("autostart_set", { enabled }),
};
