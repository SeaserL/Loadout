// SPDX-License-Identifier: Apache-2.0
// `provider/account/model` catalog: native synced lists, pool members, what actually served
// requests, and models added by hand. Loadout's per-model switches live beside it.
import { getDbInstance } from "../../db/core";
import { getSyncedAvailableModelsForConnection } from "../../db/models";
import { getPricing, getPricingForModel, updatePricing } from "../../db/settings/pricing";
import {
  findAccount,
  invalid,
  kvGet,
  kvSet,
  INTERNAL_MODELS,
  missing,
  snapshot,
  splitRoute,
  type Native,
  type Provider,
} from "./shared";

type Override = {
  enabled?: boolean;
  tier?: string | null;
  disabled_reason?: string | null;
  input_price?: number | null;
  output_price?: number | null;
};
type Manual = {
  provider: string;
  account: string;
  model: string;
  protocols: string[];
  context_window: number | null;
};
export const overrides = () => kvGet<Record<string, Override>>("modelOverrides", {});
const manuals = () => kvGet<Manual[]>("manualModels", []);

const CHAT_ONLY = /(embed|whisper|tts|image|dall-e|rerank|moderation|ocr|audio|vision-only)/i;

function comboModels() {
  const out = new Map<string, Set<string>>();
  for (const row of getDbInstance().prepare("SELECT data FROM combos").all() as { data: string }[]) {
    try {
      for (const entry of JSON.parse(row.data).models ?? []) {
        if (!entry?.connectionId || typeof entry.model !== "string") continue;
        const model = entry.model.slice(entry.model.indexOf("/") + 1);
        if (!out.has(entry.connectionId)) out.set(entry.connectionId, new Set());
        out.get(entry.connectionId)?.add(model);
      }
    } catch {
      continue;
    }
  }
  return out;
}
function observedModels() {
  const out = new Map<string, Set<string>>();
  const rows = getDbInstance()
    .prepare(
      `SELECT DISTINCT connection_id AS id, model FROM call_logs WHERE status < 400 AND status > 0 AND connection_id IS NOT NULL AND model IS NOT NULL AND model NOT IN ${INTERNAL_MODELS}`
    )
    .all() as { id: string; model: string }[];
  for (const row of rows) {
    if (!out.has(row.id)) out.set(row.id, new Set());
    out.get(row.id)?.add(row.model);
  }
  return out;
}

export type ModelRow = {
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
  disabled_reason: string | null;
  price_source: string | null;
};

const priceCache = new Map<string, { at: number; value: any }>();
export async function priceOf(p: { id: string; slug: string; kindName: string }, model: string) {
  const key = `${p.id}\0${model}`;
  const hit = priceCache.get(key);
  if (hit && Date.now() - hit.at < 60000) return hit.value;
  const usable = (v: any) => v && (typeof v.input === "number" || typeof v.output === "number");
  let value: any = null;
  for (const name of [p.id, p.kindName, p.slug]) {
    const found = await getPricingForModel(name, model).catch(() => null);
    if (usable(found)) {
      value = { ...found, source: name === p.id ? "你设置的价格" : "OmniRoute 价格表" };
      break;
    }
  }
  if (!value) {
    const table = (await getPricing().catch(() => ({}))) as Record<string, Record<string, any>>;
    for (const models of Object.values(table)) {
      const found = models?.[model];
      if (usable(found)) {
        value = { ...found, source: "同名模型价格" };
        break;
      }
    }
  }
  priceCache.set(key, { at: Date.now(), value });
  return value;
}

export async function listModels(providers?: Provider[]): Promise<ModelRow[]> {
  const list = providers ?? (await snapshot());
  const over = overrides();
  const manual = manuals();
  const fromCombos = comboModels();
  const fromLogs = observedModels();
  const rows: ModelRow[] = [];
  for (const p of list) {
    const protocols = p.apiType === "responses" ? ["responses"] : ["chat"];
    for (const conn of p.conns) {
      const entries = new Map<string, { source: string; context: number | null }>();
      const synced = (await getSyncedAvailableModelsForConnection(p.id, conn.id)) as any[];
      for (const m of synced) {
        const id = String(m.id ?? m.name ?? "");
        if (id && !CHAT_ONLY.test(id))
          entries.set(id, {
            source: "synced",
            context: m.contextLength ?? m.inputTokenLimit ?? null,
          });
      }
      for (const id of fromCombos.get(conn.id) ?? [])
        if (!entries.has(id)) entries.set(id, { source: "pool", context: null });
      for (const id of fromLogs.get(conn.id) ?? [])
        if (!entries.has(id)) entries.set(id, { source: "observed", context: null });
      for (const m of manual)
        if (m.provider === p.slug && m.account === conn.name)
          entries.set(m.model, { source: "manual", context: m.context_window });
      for (const [model, entry] of entries) {
        const name = `${p.slug}/${conn.name}/${model}`;
        const o = over[name] ?? {};
        const price = await priceOf({ id: p.id, slug: p.slug, kindName: p.kind.kind }, model);
        rows.push({
          name,
          provider: p.slug,
          account: conn.name,
          upstream_model: model,
          protocols,
          enabled: o.enabled ?? true,
          source: entry.source,
          context_window: entry.context,
          input_price: o.input_price ?? price?.input ?? null,
          output_price: o.output_price ?? price?.output ?? null,
          tier_override: o.tier ?? null,
          disabled_reason: o.disabled_reason ?? null,
          price_source: o.input_price != null ? "你设置的价格" : (price?.source ?? null),
        });
      }
    }
  }
  return rows;
}

export async function syncAccountModels(native: Native, slug: string, alias: string) {
  const providers = await snapshot();
  const { conn, provider } = findAccount(providers, slug, alias);
  const before = new Set(
    ((await getSyncedAvailableModelsForConnection(provider.id, conn.id)) as any[]).map((m) => m.id)
  );
  const result = await native<any>(`/api/providers/${conn.id}/sync-models`, "POST", {});
  const after = ((await getSyncedAvailableModelsForConnection(provider.id, conn.id)) as any[]).map(
    (m) => m.id
  );
  const added = after.filter((id) => !before.has(id)).length;
  const removed = [...before].filter((id) => !after.includes(id)).length;
  return {
    added,
    updated: Math.max(0, after.length - added),
    disabled: removed,
    total: result?.syncedModels ?? after.length,
    ambiguous: [],
  };
}

function setOverride(name: string, patch: Override) {
  const all = overrides();
  all[name] = { ...(all[name] ?? {}), ...patch };
  kvSet("modelOverrides", all);
}
async function row(name: string) {
  const found = (await listModels()).find((m) => m.name === name);
  if (!found) throw missing(`没有找到模型 ${name}`);
  return found;
}
export async function modelEnabled(name: string, enabled: boolean) {
  setOverride(name, { enabled, disabled_reason: null });
  return row(name);
}
/** Many at once: the catalog is built once and the overrides are written once. */
export async function modelEnabledMany(names: unknown, enabled: boolean) {
  if (!Array.isArray(names) || names.length === 0 || names.length > 5000 || names.some((n) => typeof n !== "string"))
    throw invalid("需要模型列表");
  const known = new Set((await listModels()).map((m) => m.name));
  const unknown = (names as string[]).find((n) => !known.has(n));
  if (unknown) throw missing(`没有找到模型 ${unknown}`);
  const all = overrides();
  for (const name of names as string[]) all[name] = { ...(all[name] ?? {}), enabled, disabled_reason: null };
  kvSet("modelOverrides", all);
  return { updated: names.length };
}
export async function modelTier(name: string, tier: string | null) {
  const map: Record<string, string> = {
    "ultra-fast": "ULTRA_FAST",
    fast: "FAST",
    normal: "NORMAL",
  };
  setOverride(name, { tier: tier ? (map[tier] ?? tier) : null });
  return row(name);
}
export async function modelPrice(name: string, input: number | null, output: number | null) {
  const { provider, model } = splitRoute(await snapshot(), name);
  setOverride(name, { input_price: input, output_price: output });
  // Written to the native price table too, so gateway cost statistics follow it.
  if (input != null || output != null)
    await updatePricing({
      [provider.id]: { [model]: { input: input ?? 0, output: output ?? 0 } },
    } as any);
  priceCache.clear();
  return row(name);
}
export async function modelAdd(p: any) {
  const model = String(p.model ?? "").trim();
  if (!model) throw invalid("需要填写模型 ID");
  const providers = await snapshot();
  findAccount(providers, p.provider, p.account);
  const list = manuals().filter(
    (m) => !(m.provider === p.provider && m.account === p.account && m.model === model)
  );
  list.push({
    provider: p.provider,
    account: p.account,
    model,
    protocols: p.protocols ?? ["chat"],
    context_window: p.context_window ?? null,
  });
  kvSet("manualModels", list);
  return row(`${p.provider}/${p.account}/${model}`);
}
export async function modelRemove(name: string) {
  const [provider, account, ...rest] = name.split("/");
  const model = rest.join("/");
  kvSet(
    "manualModels",
    manuals().filter((m) => !(m.provider === provider && m.account === account && m.model === model))
  );
  return { revoked: true };
}

const normalize = (model: string) =>
  model
    .toLowerCase()
    .replace(/^.*\//, "")
    .replace(/[._]/g, "-");

/** Same model across accounts, priced at the user's own token mix; unknown is never free. */
export async function priceCompare(filter: string | null) {
  const rows = (await listModels()).filter(
    (m) => !filter || normalize(m.upstream_model).includes(normalize(filter))
  );
  const usage = getDbInstance()
    .prepare(
      `SELECT model, COUNT(*) AS requests, SUM(tokens_in) AS input, SUM(tokens_out) AS output, SUM(tokens_cache_read) AS cacheRead, SUM(tokens_cache_creation) AS cacheWrite FROM call_logs WHERE model NOT IN ${INTERNAL_MODELS} GROUP BY model`
    )
    .all() as any[];
  const usageOf = new Map<string, any>();
  const overall = { requests: 0, input: 0, output: 0, cacheRead: 0, cacheWrite: 0 };
  for (const u of usage) {
    const key = normalize(String(u.model ?? ""));
    const cur = usageOf.get(key) ?? { requests: 0, input: 0, output: 0, cacheRead: 0, cacheWrite: 0 };
    for (const f of Object.keys(overall) as (keyof typeof overall)[]) {
      cur[f] += Number(u[f] ?? 0);
      overall[f] += Number(u[f] ?? 0);
    }
    usageOf.set(key, cur);
  }
  const groups = new Map<string, ModelRow[]>();
  for (const r of rows) {
    const key = normalize(r.upstream_model);
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key)?.push(r);
  }
  const blended = (input: number | null, output: number | null, mix: any) => {
    if (input == null && output == null) return null;
    const total = mix.input + mix.output;
    if (total <= 0) return ((input ?? 0) + (output ?? 0)) / 2;
    return ((input ?? 0) * mix.input + (output ?? 0) * mix.output) / total;
  };
  const out = [];
  for (const [key, list] of groups) {
    const own = usageOf.get(key);
    const mix = own?.requests ? own : overall.requests ? overall : null;
    const source = own?.requests ? "model" : overall.requests ? "all" : "default";
    const m = mix ?? { requests: 0, input: 3, output: 1, cacheRead: 0, cacheWrite: 0 };
    out.push({
      key,
      mix: {
        source,
        requests: m.requests,
        input_tokens: m.input,
        output_tokens: m.output,
        cache_read_tokens: m.cacheRead,
        cache_write_tokens: m.cacheWrite,
      },
      offers: list
        .map((r) => ({
          name: r.name,
          provider: r.provider,
          account: r.account,
          upstream_model: r.upstream_model,
          enabled: r.enabled,
          input_price: r.input_price,
          output_price: r.output_price,
          cache_read_price: null,
          cache_write_price: null,
          price_source: r.price_source,
          price_updated_at_ms: null,
          blended_per_million: blended(r.input_price, r.output_price, m),
        }))
        .sort(
          (a, b) => (a.blended_per_million ?? Infinity) - (b.blended_per_million ?? Infinity)
        ),
    });
  }
  return out.sort((a, b) => b.offers.length - a.offers.length || a.key.localeCompare(b.key));
}
