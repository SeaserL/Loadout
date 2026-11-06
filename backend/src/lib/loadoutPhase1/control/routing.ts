// SPDX-License-Identifier: Apache-2.0
// Health and latency tiers per route, pools as native combos, built-in virtual models,
// sub-pools and dedicated endpoints. Selection itself stays in OmniRoute's execution chain.
import { getDbInstance } from "../../db/core";
import { runSingleModelTest } from "../../api/modelTestRunner";
import { listModels, overrides, type ModelRow } from "./models";
import {
  findProvider,
  INTERNAL_MODELS,
  invalid,
  isSubscription,
  kvGet,
  kvSet,
  missing,
  snapshot,
  splitRoute,
  type Native,
  type Provider,
} from "./shared";

export const ultraFastMs = () => Number(kvGet("setting.latency.ultra_fast_ms", 800));
export const fastMs = () => Number(kvGet("setting.latency.fast_ms", 2000));

type Bench = { ttft_ms: number | null; duration_ms: number; ok: boolean; at: number };
const benches = () => kvGet<Record<string, Bench>>("bench", {});

function percentile(sorted: number[], p: number) {
  if (!sorted.length) return null;
  return sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * p))];
}
export function tierOf(ttft: number | null, pinned: string | null | undefined) {
  if (pinned) return pinned;
  if (ttft == null) return "UNMEASURED";
  if (ttft <= ultraFastMs()) return "ULTRA_FAST";
  if (ttft <= fastMs()) return "FAST";
  return "NORMAL";
}

export async function routeStatus(providers?: Provider[], rows?: ModelRow[]) {
  const list = providers ?? (await snapshot());
  const models = rows ?? (await listModels(list));
  const since = new Date(Date.now() - 7 * 86400000).toISOString();
  const logs = getDbInstance()
    .prepare(
      `SELECT connection_id AS id, model, status, ttft_ms AS ttft, duration, tokens_out AS out, error_summary AS error, timestamp FROM call_logs WHERE timestamp >= ? AND connection_id IS NOT NULL AND model NOT IN ${INTERNAL_MODELS} ORDER BY timestamp DESC LIMIT 20000`
    )
    .all(since) as any[];
  const byRoute = new Map<string, any[]>();
  for (const l of logs) {
    const key = `${l.id}\0${l.model}`;
    if (!byRoute.has(key)) byRoute.set(key, []);
    byRoute.get(key)?.push(l);
  }
  const bench = benches();
  const now = Date.now();
  const out = [];
  for (const p of list)
    for (const conn of p.conns) {
      const until = conn.rateLimitedUntil ? Date.parse(conn.rateLimitedUntil) : NaN;
      const limited = Number.isFinite(until) && until > now;
      const errType = String(conn.lastErrorType ?? "");
      for (const m of models) {
        if (m.provider !== p.slug || m.account !== conn.name) continue;
        const samples = byRoute.get(`${conn.id}\0${m.upstream_model}`) ?? [];
        const ok = samples.filter((s) => s.status > 0 && s.status < 400);
        const fail = samples.filter((s) => s.status >= 400);
        const ttfts = ok
          .map((s) => s.ttft)
          .filter((v) => typeof v === "number")
          .sort((a, b) => a - b);
        const b = bench[m.name];
        const p50 = percentile(ttfts, 0.5) ?? b?.ttft_ms ?? null;
        const p90 = percentile(ttfts, 0.9) ?? b?.ttft_ms ?? null;
        const rates = ok
          .filter((s) => s.out > 0 && s.duration > 0)
          .map((s) => (s.out * 1000) / Math.max(1, s.duration - (s.ttft ?? 0)))
          .sort((x, y) => x - y);
        const lastFail = fail[0];
        const state =
          conn.isActive === false
            ? "DISABLED"
            : limited
              ? errType.includes("quota")
                ? "QUOTA_EXHAUSTED"
                : "RATE_LIMITED"
              : /auth|expired|invalid/.test(errType) || conn.testStatus === "expired"
                ? "AUTH_FAILED"
                : samples.length
                  ? "HEALTHY"
                  : "UNKNOWN";
        const pinned = m.tier_override;
        out.push({
          name: m.name,
          protocol: m.protocols[0] ?? "chat",
          trust: isSubscription(conn) ? "subscription" : "official_api",
          state,
          unavailable_until_ms: limited ? until : null,
          consecutive_failures: Number(conn.backoffLevel ?? 0),
          in_flight: 0,
          successes: ok.length,
          failures: fail.length,
          last_error: lastFail?.error ?? (state === "AUTH_FAILED" ? (conn.lastError ?? null) : null),
          last_failure_ms: lastFail ? Date.parse(lastFail.timestamp) : null,
          recovered_at_ms: null,
          tier: tierOf(p50, pinned),
          tier_pinned: !!pinned,
          ttft_p50_ms: p50,
          ttft_p90_ms: p90,
          tps: percentile(rates, 0.5),
          samples: samples.length,
          last_sample_ms: samples[0] ? Date.parse(samples[0].timestamp) : (b?.at ?? null),
        });
      }
    }
  return out;
}

// ---- measuring -----------------------------------------------------------------------------

export async function benchTargets(providers: Provider[], names: string[]) {
  const results: any[] = [];
  const queue = [...names];
  const store = benches();
  const worker = async () => {
    for (;;) {
      const name = queue.shift();
      if (!name) return;
      const { conn, model } = splitRoute(providers, name);
      const started = Date.now();
      const r: any = await runSingleModelTest({
        providerId: conn.provider,
        modelId: `${conn.provider}/${model}`,
        connectionId: conn.id,
        prompt: "Reply with the single word OK.",
        timeoutMs: 60000,
        streamChat: true,
      }).catch((e: Error) => ({ status: "error", error: e.message, latencyMs: 0 }));
      const ok = r.status === "ok";
      const elapsed = Date.now() - started;
      const ttft = ok ? (r.firstTokenLatencyMs ?? r.ttftMs ?? r.latencyMs ?? elapsed) : null;
      if (ok) store[name] = { ttft_ms: ttft, duration_ms: elapsed, ok, at: Date.now() };
      results.push({
        name,
        protocol: "chat",
        ok,
        ttft_ms: ttft,
        duration_ms: elapsed,
        output_tokens: null,
        error: ok ? null : String(r.error ?? "测速失败"),
        tier: tierOf(ttft, overrides()[name]?.tier),
      });
    }
  };
  await Promise.all(Array.from({ length: Math.min(3, names.length) }, worker));
  kvSet("bench", store);
  return results.sort((a, b) => a.name.localeCompare(b.name));
}

export async function benchRun(native: Native, names: string[]) {
  const providers = await snapshot();
  const rows = await listModels(providers);
  let targets = names ?? [];
  const known = new Set(rows.map((m) => m.name));
  const virtual = await listPools(native);
  targets = targets.flatMap((n) => {
    if (known.has(n)) return [n];
    const pool = virtual.find((p) => `loadout/${p.name}` === n);
    return pool?.members ?? [];
  });
  if (!names?.length) {
    const bench = benches();
    targets = rows
      .filter((m) => m.enabled && !bench[m.name])
      .map((m) => m.name)
      .filter((n) => {
        const { conn } = splitRoute(providers, n);
        return conn.isActive !== false && !isSubscription(conn);
      });
  }
  const results = await benchTargets(providers, [...new Set(targets)]);
  await refreshVirtuals(native);
  await resortLatencyPools(native);
  return results;
}

// ---- pools ---------------------------------------------------------------------------------

const POOL_NAME = /^[a-z0-9][a-z0-9._-]{0,60}$/;
const PREFIX = "loadout/";
// cheap (1.1): every enabled API-key route, the lowest input price first; unknown prices last.
export const VIRTUAL = ["realtime", "fast", "default", "cheap"];
const NATIVE_STRATEGY: Record<string, string> = { manual: "priority", latency: "priority" };
type PoolMeta = { ordering: string };
const poolMeta = () => kvGet<Record<string, PoolMeta>>("pools", {});

function comboRows() {
  return (
    getDbInstance().prepare("SELECT id, name, data FROM combos ORDER BY sort_order").all() as any[]
  ).map((r) => ({ id: r.id, name: r.name, ...JSON.parse(r.data) }));
}
function mappingRows() {
  return getDbInstance()
    .prepare("SELECT id, pattern, combo_id AS comboId, enabled FROM model_combo_mappings")
    .all() as any[];
}

export function mappedComboNames() {
  const combos = comboRows();
  return new Set(
    mappingRows().flatMap((m) => {
      const name = combos.find((c) => c.id === m.comboId)?.name;
      return name ? [name] : [];
    })
  );
}

function memberName(providers: Provider[], entry: any) {
  if (typeof entry === "string") return entry;
  const conn = providers.flatMap((p) => p.conns).find((c) => c.id === entry.connectionId);
  if (!conn) return String(entry.model);
  const p = providers.find((x) => x.id === conn.provider);
  const model = String(entry.model);
  const slash = model.indexOf("/");
  return `${p?.slug ?? conn.provider}/${conn.name}/${slash >= 0 ? model.slice(slash + 1) : model}`;
}

export async function listPools(_native?: Native) {
  const providers = await snapshot();
  const combos = comboRows();
  const meta = poolMeta();
  return mappingRows()
    .filter(
      (m) =>
        String(m.pattern).startsWith(PREFIX) && !VIRTUAL.includes(m.pattern.slice(PREFIX.length))
    )
    .flatMap((m) => {
      const combo = combos.find((c) => c.id === m.comboId);
      if (!combo) return [];
      const name = m.pattern.slice(PREFIX.length);
      const strategy = String(combo.strategy ?? "priority");
      return [
        {
          name,
          ordering: meta[name]?.ordering ?? (strategy === "priority" ? "manual" : strategy),
          members: (combo.models ?? []).map((e: any) => memberName(providers, e)),
          enabled: !!m.enabled,
          combo: combo.name,
        },
      ];
    });
}

function comboEntries(providers: Provider[], members: string[]) {
  return members.map((name) => {
    const { provider, conn, model } = splitRoute(providers, name);
    return { kind: "model", model: `${provider.id}/${model}`, connectionId: conn.id };
  });
}
async function saveCombo(
  native: Native,
  comboName: string,
  pattern: string,
  strategy: string,
  models: unknown[]
) {
  const combos = comboRows();
  const existing =
    combos.find((c) => c.id === mappingRows().find((m) => m.pattern === pattern)?.comboId) ??
    combos.find((c) => c.name === comboName);
  const body = { strategy, models };
  const id = existing
    ? (await native(`/api/combos/${existing.id}`, "PUT", body), existing.id)
    : ((await native<any>("/api/combos", "POST", { name: comboName, ...body })).id ??
      comboRows().find((c) => c.name === comboName)?.id);
  const mapping = mappingRows().find((m) => m.pattern === pattern);
  if (mapping) {
    if (mapping.comboId !== id || !mapping.enabled)
      await native(`/api/model-combo-mappings/${mapping.id}`, "PUT", {
        comboId: id,
        enabled: true,
      });
  } else
    await native("/api/model-combo-mappings", "POST", {
      pattern,
      comboId: id,
      enabled: true,
      priority: 100,
      description: "",
    });
  return id;
}

async function byLatency(providers: Provider[], members: string[]) {
  const status = await routeStatus(providers);
  const ttft = new Map(status.map((s) => [s.name, s.ttft_p50_ms]));
  return [...members].sort(
    (a, b) => (ttft.get(a) ?? Number.MAX_SAFE_INTEGER) - (ttft.get(b) ?? Number.MAX_SAFE_INTEGER)
  );
}

export async function poolSave(native: Native, name: string, ordering: string, members: string[]) {
  if (!POOL_NAME.test(name)) throw invalid("名称只能使用小写字母、数字、点、下划线和连字符");
  if (VIRTUAL.includes(name)) throw invalid(`loadout/${name} 是内置虚拟模型`);
  if (!Array.isArray(members) || members.length === 0) throw invalid("至少选择一个模型");
  const providers = await snapshot();
  const ordered = ordering === "latency" ? await byLatency(providers, members) : members;
  await saveCombo(
    native,
    "pool-" + name,
    PREFIX + name,
    NATIVE_STRATEGY[ordering] ?? ordering,
    comboEntries(providers, ordered)
  );
  const meta = poolMeta();
  meta[name] = { ordering };
  kvSet("pools", meta);
  return (await listPools()).find((p) => p.name === name);
}

export async function poolRemove(native: Native, name: string) {
  const mapping = mappingRows().find((m) => m.pattern === PREFIX + name);
  if (!mapping) throw missing(`没有找到 ${name}`);
  await native(`/api/model-combo-mappings/${mapping.id}`, "DELETE");
  await native(`/api/combos/${mapping.comboId}`, "DELETE");
  const meta = poolMeta();
  delete meta[name];
  kvSet("pools", meta);
  return { revoked: true };
}

async function resortLatencyPools(native: Native) {
  const providers = await snapshot();
  for (const pool of await listPools()) {
    if (pool.ordering !== "latency") continue;
    const ordered = await byLatency(providers, pool.members);
    if (ordered.join("\n") !== pool.members.join("\n"))
      await saveCombo(native, pool.combo, PREFIX + pool.name, "priority", comboEntries(providers, ordered));
  }
}

// ---- built-in virtual models ---------------------------------------------------------------

const virtualFlags = () => kvGet<Record<string, boolean>>("virtuals", {});
const MAX_VIRTUAL_MEMBERS = 30;

async function virtualMembers(providers: Provider[], name: string) {
  const status = await routeStatus(providers);
  const rows = await listModels(providers);
  const eligible = new Set(
    rows
      .filter((m) => {
        if (!m.enabled) return false;
        const { conn } = splitRoute(providers, m.name);
        return conn.isActive !== false && !isSubscription(conn);
      })
      .map((m) => m.name)
  );
  if (name === "cheap") {
    const price = (m: { input_price: number | null; output_price: number | null }) =>
      m.input_price ?? Number.MAX_SAFE_INTEGER;
    return rows
      .filter((m) => eligible.has(m.name))
      .sort((a, b) => price(a) - price(b) || (a.output_price ?? Number.MAX_SAFE_INTEGER) - (b.output_price ?? Number.MAX_SAFE_INTEGER))
      .map((m) => m.name)
      .slice(0, MAX_VIRTUAL_MEMBERS);
  }
  const accepted: Record<string, string[]> = {
    realtime: ["ULTRA_FAST"],
    fast: ["ULTRA_FAST", "FAST"],
    default: ["ULTRA_FAST", "FAST", "NORMAL", "UNMEASURED"],
  };
  return status
    .filter((s) => eligible.has(s.name) && accepted[name].includes(s.tier))
    .sort(
      (a, b) =>
        (a.ttft_p50_ms ?? Number.MAX_SAFE_INTEGER) - (b.ttft_p50_ms ?? Number.MAX_SAFE_INTEGER)
    )
    .map((s) => s.name)
    .slice(0, MAX_VIRTUAL_MEMBERS);
}

async function refreshVirtuals(native: Native) {
  const flags = virtualFlags();
  const providers = await snapshot();
  for (const name of VIRTUAL) {
    if (!flags[name]) continue;
    const members = await virtualMembers(providers, name);
    if (members.length)
      await saveCombo(native, "virtual-" + name, PREFIX + name, "priority", comboEntries(providers, members));
  }
}

export async function virtualSet(native: Native, name: string, enabled: boolean) {
  const short = name.replace(PREFIX, "");
  if (!VIRTUAL.includes(short)) throw invalid("不是内置虚拟模型");
  const flags = virtualFlags();
  if (enabled) {
    const providers = await snapshot();
    const members = await virtualMembers(providers, short);
    if (!members.length)
      throw invalid(short === "cheap" ? "还没有可用的模型" : "还没有符合这个档位的路由，先测速。");
    await saveCombo(native, "virtual-" + short, PREFIX + short, "priority", comboEntries(providers, members));
  } else {
    const mapping = mappingRows().find((m) => m.pattern === PREFIX + short);
    if (mapping) await native(`/api/model-combo-mappings/${mapping.id}`, "PUT", { enabled: false });
  }
  flags[short] = enabled;
  kvSet("virtuals", flags);
  return virtuals(native);
}

export async function virtuals(native: Native) {
  const providers = await snapshot();
  const flags = virtualFlags();
  const out: any[] = [];
  for (const name of VIRTUAL)
    out.push({
      name: PREFIX + name,
      kind: "builtin",
      ordering: name === "cheap" ? "cost-optimized" : "latency",
      routes: await virtualMembers(providers, name),
      enabled: !!flags[name],
    });
  for (const pool of await listPools(native))
    out.push({
      name: PREFIX + pool.name,
      kind: "pool",
      ordering: pool.ordering,
      routes: pool.members,
      enabled: pool.enabled,
    });
  return out;
}

// ---- sub-pools -----------------------------------------------------------------------------

type SubpoolStore = { active: string | null; remember: boolean; pools: Record<string, string[]> };
const subpools = () =>
  kvGet<SubpoolStore>("subpools", { active: null, remember: true, pools: {} });

export function subpoolState() {
  const s = subpools();
  return {
    active: s.active,
    remember: s.remember,
    subpools: Object.entries(s.pools)
      .map(([name, accounts]) => ({
        name,
        active: s.active === name,
        accounts: [...accounts].sort(),
      }))
      .sort((a, b) => a.name.localeCompare(b.name)),
  };
}
export function subpoolSave(name: string, accounts: string[], renameFrom: string | null) {
  if (!name.trim()) throw invalid("需要名称");
  const s = subpools();
  if (renameFrom && renameFrom !== name) {
    delete s.pools[renameFrom];
    if (s.active === renameFrom) s.active = name;
  }
  s.pools[name] = accounts;
  kvSet("subpools", s);
  return subpoolState().subpools.find((p) => p.name === name);
}
export function subpoolRemove(name: string) {
  const s = subpools();
  delete s.pools[name];
  if (s.active === name) s.active = null;
  kvSet("subpools", s);
  return { revoked: true };
}
export function subpoolActivate(name: string | null) {
  const s = subpools();
  if (name && !s.pools[name]) throw missing(`没有找到号池 ${name}`);
  s.active = name;
  kvSet("subpools", s);
  return subpoolState();
}
export function subpoolRemember(on: boolean) {
  const s = subpools();
  s.remember = on;
  kvSet("subpools", s);
}

// ---- dedicated endpoints -------------------------------------------------------------------

export type Endpoint = {
  name: string;
  provider: string;
  enabled: boolean;
  members: string[] | null;
  default_model: string | null;
  wait_secs: number;
  rotate: boolean;
};
export const endpoints = () => kvGet<Endpoint[]>("endpoints", []);

export async function endpointInfos() {
  const providers = await snapshot();
  const rows = await listModels(providers);
  const base = `http://127.0.0.1:${process.env.PORT}`;
  return endpoints().flatMap((e) => {
    const p = providers.find((x) => x.slug === e.provider);
    if (!p) return [];
    const active = p.conns
      .filter((c) => c.isActive !== false && (!e.members || e.members.includes(c.name)))
      .map((c) => c.name);
    const models = [
      ...new Set(
        rows
          .filter((m) => m.provider === p.slug && m.enabled && active.includes(m.account))
          .map((m) => m.upstream_model)
      ),
    ].sort();
    return [
      {
        ...e,
        active_accounts: active,
        models,
        openai_base_url: `${base}/e/${e.name}/v1`,
      },
    ];
  });
}

export async function endpointSave(native: Native, p: any) {
  const providers = await snapshot();
  const provider = findProvider(providers, p.provider);
  const name = String(p.name ?? provider.slug);
  if (!/^[a-z0-9][a-z0-9-]{0,40}$/.test(name)) throw invalid("名称无效");
  const list = endpoints().filter((e) => e.name !== name);
  list.push({
    name,
    provider: provider.slug,
    enabled: p.enabled !== false,
    members: Array.isArray(p.members) ? p.members : null,
    default_model: p.default_model ?? null,
    wait_secs: Number(p.wait_secs ?? 0),
    rotate: p.rotate === true,
  });
  kvSet("endpoints", list);
  const settings = await native<any>("/api/settings");
  await native("/api/settings", "PATCH", {
    providerStrategies: {
      ...(settings.providerStrategies ?? {}),
      [provider.id]: p.rotate
        ? { fallbackStrategy: "round-robin", stickyRoundRobinLimit: 1 }
        : { fallbackStrategy: "fill-first" },
    },
  });
  return (await endpointInfos()).find((e) => e.name === name);
}

export function endpointRemove(name: string) {
  kvSet(
    "endpoints",
    endpoints().filter((e) => e.name !== name)
  );
  return { revoked: true };
}
