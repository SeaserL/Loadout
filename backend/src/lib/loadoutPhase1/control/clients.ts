// SPDX-License-Identifier: Apache-2.0
// Client keys are native hash-only api_keys; the raw key is shown once, on creation.
import { updateApiKeyPermissions } from "../../db/apiKeys";
import { getDbInstance } from "../../db/core";
import { listModels, priceOf } from "./models";
import { VIRTUAL } from "./routing";
import { invalid, missing, snapshot, splitRoute, type Native, type Provider } from "./shared";

const POOL = "loadout/";

function usedToday(providers: Provider[]) {
  const start = new Date();
  start.setHours(0, 0, 0, 0);
  return getDbInstance()
    .prepare(
      `SELECT api_key_id AS id, provider, model, SUM(COALESCE(tokens_in,0)) AS input, SUM(COALESCE(tokens_out,0)) AS output
       FROM call_logs WHERE timestamp >= ? AND api_key_id IS NOT NULL GROUP BY 1, 2, 3`
    )
    .all(start.toISOString()) as any[];
}

/** A key's own rule list wins over the per-minute column at enforcement; migrated keys carry one. */
function perMinute(k: any): number | null {
  const rule = (k.rateLimits ?? []).find((r: any) => r.window === 60);
  return rule?.limit ?? k.maxRequestsPerMinute ?? null;
}

export async function clientInfos(native: Native) {
  const providers = await snapshot();
  const { keys } = await native<{ keys: any[] }>("/api/keys");
  const used = usedToday(providers);
  const byId = new Map(providers.map((p) => [p.id, p]));
  const out = [];
  for (const k of keys) {
    let tokens = 0;
    let cost = 0;
    for (const u of used) {
      if (u.id !== k.id) continue;
      tokens += u.input + u.output;
      const p = byId.get(u.provider);
      const price = await priceOf(
        p
          ? { id: p.id, slug: p.slug, kindName: p.kind.kind }
          : { id: u.provider, slug: u.provider, kindName: u.provider },
        u.model
      );
      if (price) cost += (u.input * (price.input ?? 0) + u.output * (price.output ?? 0)) / 1e6;
    }
    out.push({
      id: k.id,
      name: k.name,
      key_prefix: k.keyPrefix ?? String(k.key ?? "").slice(0, 12),
      enabled: k.isActive !== false && !k.revokedAt,
      created_at_ms: Date.parse(k.createdAt) || 0,
      revoked_at_ms: k.revokedAt ? Date.parse(k.revokedAt) : null,
      policy: {
        allowed_models: (k.allowedModels ?? []).filter((m: string) => !m.startsWith("openai-compatible-")),
        allowed_accounts: (k.allowedConnections ?? []).flatMap((id: string) => {
          for (const p of providers) {
            const c = p.conns.find((x) => x.id === id);
            if (c) return [`${p.slug}/${c.name}`];
          }
          return [];
        }),
        daily_cost_limit_usd: k.dailyUsageLimitUsd ?? null,
        weekly_cost_limit_usd: k.weeklyUsageLimitUsd ?? null,
        requests_per_minute: perMinute(k),
      },
      tokens_today: tokens,
      cost_today_usd: cost,
    });
  }
  return out.sort((a, b) => b.created_at_ms - a.created_at_ms);
}

async function byName(native: Native, name: string) {
  const { keys } = await native<{ keys: any[] }>("/api/keys");
  const key = keys.find((k) => k.name === name);
  if (!key) throw missing(`没有找到客户端 ${name}`);
  return key;
}

export async function clientCreate(native: Native, name: string) {
  const created = await native<any>("/api/keys", "POST", { name, noLog: true });
  const list = await clientInfos(native);
  const client = list.find((c) => c.id === created.id) ?? list.find((c) => c.name === name);
  return { client, key: created.key };
}

export async function clientRevoke(native: Native, name: string) {
  const key = await byName(native, name);
  await native(`/api/keys/${key.id}`, "DELETE");
  return { revoked: true };
}

/**
 * Allowed names are what clients send. Native checks compare the rewritten form too, so each
 * `provider/account/model` also grants its node form, and each pool grants its combo.
 */
export async function clientPolicy(native: Native, name: string, policy: any) {
  const key = await byName(native, name);
  const providers = await snapshot();
  const allowed: string[] = (policy.allowed_models ?? []).map((m: string) => m.trim()).filter(Boolean);
  const models = new Set<string>();
  const combos = new Set<string>();
  const mappings = getDbInstance()
    .prepare("SELECT pattern, combo_id AS comboId FROM model_combo_mappings")
    .all() as any[];
  const comboNames = new Map(
    (getDbInstance().prepare("SELECT id, name FROM combos").all() as any[]).map((c) => [c.id, c.name])
  );
  // A name nothing answers to would lock the client out without saying why.
  const sendable = new Set<string>([
    ...comboNames.values(),
    ...VIRTUAL.map((v) => POOL + v),
    ...mappings.map((m) => m.pattern),
    ...(await listModels(providers)).flatMap((m) => [m.name, `${m.provider}/${m.upstream_model}`]),
  ]);
  const unknown = allowed.filter((name) => !sendable.has(name));
  if (unknown.length > 0) throw invalid(`没有这些模型：${unknown.join("、")}`);
  for (const name of allowed) {
    models.add(name);
    const mapped = mappings.find((m) => m.pattern === name);
    if (mapped && comboNames.get(mapped.comboId)) combos.add(comboNames.get(mapped.comboId));
    if (name.startsWith(POOL)) continue;
    const parts = name.split("/");
    if (parts.length >= 3) {
      try {
        const { provider, model } = splitRoute(providers, name);
        models.add(`${provider.id}/${model}`);
      } catch {
        continue;
      }
    } else if (parts.length === 2) {
      const provider = providers.find((p) => p.slug === parts[0]);
      if (provider) models.add(`${provider.id}/${parts[1]}`);
    }
  }
  const connections: string[] = [];
  for (const address of policy.allowed_accounts ?? []) {
    const [slug, alias] = String(address).split("/");
    const conn = providers.find((p) => p.slug === slug)?.conns.find((c) => c.name === alias);
    if (conn) connections.push(conn.id);
  }
  await native(`/api/keys/${key.id}`, "PATCH", {
    modelAccessMode: allowed.length ? "restricted" : "all",
    allowedModels: allowed.length ? [...models] : [],
    allowedCombos: [...combos],
    allowedConnections: connections,
    usageLimitEnabled:
      policy.daily_cost_limit_usd != null || policy.weekly_cost_limit_usd != null,
    dailyUsageLimitUsd: policy.daily_cost_limit_usd ?? null,
    weeklyUsageLimitUsd: policy.weekly_cost_limit_usd ?? null,
  });
  // The native route's schema drops the per-minute field; the column and its enforcement exist.
  const limit = policy.requests_per_minute ?? null;
  const rules: any[] = key.rateLimits ?? [];
  await updateApiKeyPermissions(
    key.id,
    rules.length > 0
      ? {
          rateLimits: [
            ...rules.filter((r) => r.window !== 60),
            ...(limit ? [{ limit, window: 60 }] : []),
          ],
        }
      : { maxRequestsPerMinute: limit }
  );
  return (await clientInfos(native)).find((c) => c.id === key.id);
}
