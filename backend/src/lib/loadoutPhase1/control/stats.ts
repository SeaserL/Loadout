// SPDX-License-Identifier: Apache-2.0
// Metadata only: call_logs never carries prompts or replies here (ENABLE_REQUEST_LOGS=false).
import { getProviderConnectionById } from "../../db/providers";
import { getDbInstance } from "../../db/core";
import { getPendingById } from "../../usage/usageHistory";
import { getUsageForProvider } from "@omniroute/open-sse/services/usage.ts";
import { priceOf } from "./models";
import { clientName, INTERNAL_MODELS, kvGet, kvSet, snapshot, type Provider } from "./shared";

const iso = (ms: number) => new Date(ms).toISOString();
const KEY: Record<string, string> = {
  model: "model",
  provider: "provider",
  route: "provider || '/' || COALESCE(account, '?') || '/' || model",
  client: "COALESCE(api_key_name, '')",
  day: "substr(timestamp, 1, 10)",
  account: "provider || '/' || COALESCE(account, '?')",
};

async function priceIndex(providers: Provider[]) {
  const byId = new Map(providers.map((p) => [p.id, p]));
  return (id: string, model: string) => {
    const p = byId.get(id);
    return priceOf(
      p
        ? { id: p.id, slug: p.slug, kindName: p.kind.kind }
        : { id: String(id), slug: String(id), kindName: String(id) },
      String(model)
    );
  };
}
const costOf = (price: any, input: number, output: number, cached: number) => {
  if (!price) return null;
  const plainInput = Math.max(0, input - cached);
  return (
    (plainInput * (price.input ?? 0) +
      cached * (price.cached ?? price.input ?? 0) +
      output * (price.output ?? 0)) /
    1e6
  );
};

/**
 * `tzOffsetMin` is the viewer's offset from UTC: a day or an hour here means theirs, so a request
 * made at 07:00 in Beijing belongs to today, not to yesterday's UTC date.
 */
export async function statsSummary(
  sinceMs: number,
  groupBy: string,
  untilMs?: number | null,
  tzOffsetMin?: number | null
) {
  const shift = Math.max(-840, Math.min(840, Math.trunc(Number(tzOffsetMin) || 0)));
  const local = `datetime(timestamp, '${shift >= 0 ? "+" : ""}${shift} minutes')`;
  const expr = {
    ...KEY,
    day: `substr(${local}, 1, 10)`,
    hour: `substr(${local}, 12, 2)`,
  }[groupBy] ?? KEY.model;
  const providers = await snapshot();
  const price = await priceIndex(providers);
  const slugOf = new Map(providers.map((p) => [p.id, p.slug]));
  const rows = getDbInstance()
    .prepare(
      `SELECT ${expr} AS key, provider, model, COUNT(*) AS requests,
         SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END) AS errors,
         SUM(COALESCE(tokens_in, 0)) AS input, SUM(COALESCE(tokens_out, 0)) AS output,
         SUM(COALESCE(tokens_cache_read, 0)) AS cacheRead,
         SUM(COALESCE(tokens_cache_creation, 0)) AS cacheWrite,
         AVG(ttft_ms) AS ttft, SUM(CASE WHEN ttft_ms IS NOT NULL THEN 1 ELSE 0 END) AS ttftN,
         SUM(COALESCE(duration, 0)) AS duration
       FROM call_logs WHERE timestamp >= ? AND timestamp < ? AND model NOT IN ${INTERNAL_MODELS} GROUP BY 1, provider, model`
    )
    .all(iso(sinceMs || 0), untilMs ? iso(untilMs) : "9999") as any[];
  const groups = new Map<string, any>();
  for (const r of rows) {
    let key = String(r.key ?? "");
    if (groupBy === "provider") key = slugOf.get(key) ?? key;
    if (groupBy === "client") key = clientName(key);
    if (groupBy === "route" || groupBy === "account") {
      const slash = key.indexOf("/");
      key = (slugOf.get(key.slice(0, slash)) ?? key.slice(0, slash)) + key.slice(slash);
    }
    const g = groups.get(key) ?? {
      key,
      requests: 0,
      errors: 0,
      input_tokens: 0,
      output_tokens: 0,
      cost_usd: 0,
      cache_read_tokens: 0,
      cache_write_tokens: 0,
      ttftSum: 0,
      ttftN: 0,
      tokensForRate: 0,
      durationForRate: 0,
    };
    g.requests += r.requests;
    g.errors += r.errors;
    g.input_tokens += r.input;
    g.output_tokens += r.output;
    g.cache_read_tokens += r.cacheRead;
    g.cache_write_tokens += r.cacheWrite;
    g.ttftSum += (r.ttft ?? 0) * r.ttftN;
    g.ttftN += r.ttftN;
    g.tokensForRate += r.output;
    g.durationForRate += r.duration;
    g.cost_usd += costOf(await price(r.provider, r.model), r.input, r.output, r.cacheRead) ?? 0;
    groups.set(key, g);
  }
  return [...groups.values()]
    .map((g) => ({
      key: g.key,
      requests: g.requests,
      errors: g.errors,
      input_tokens: g.input_tokens,
      output_tokens: g.output_tokens,
      cost_usd: g.cost_usd,
      avg_ttft_ms: g.ttftN ? g.ttftSum / g.ttftN : null,
      avg_tps: g.durationForRate > 0 ? (g.tokensForRate * 1000) / g.durationForRate : null,
      cache_read_tokens: g.cache_read_tokens,
      cache_write_tokens: g.cache_write_tokens,
      cache_hit_rate:
        g.input_tokens > 0 ? Math.min(1, g.cache_read_tokens / g.input_tokens) : null,
      cache_savings_usd: 0,
    }))
    .sort((a, b) =>
      groupBy === "day" || groupBy === "hour" ? a.key.localeCompare(b.key) : b.requests - a.requests
    );
}

/** Real client → model → account links, for the ribbon view. Nothing here is inferred. */
export async function statsFlow(sinceMs: number) {
  const providers = await snapshot();
  const price = await priceIndex(providers);
  const slugOf = new Map(providers.map((p) => [p.id, p.slug]));
  const rows = getDbInstance()
    .prepare(
      `SELECT COALESCE(api_key_name, '') AS client, provider, COALESCE(account, '') AS account, model,
         COALESCE(combo_name, '') AS combo, COUNT(*) AS requests,
         SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END) AS errors,
         SUM(COALESCE(tokens_in, 0)) AS input, SUM(COALESCE(tokens_out, 0)) AS output,
         SUM(COALESCE(tokens_cache_read, 0)) AS cacheRead
       FROM call_logs WHERE timestamp >= ? AND connection_id IS NOT NULL AND model NOT IN ${INTERNAL_MODELS}
       GROUP BY 1, 2, 3, 4, 5`
    )
    .all(iso(sinceMs || 0)) as any[];
  const links = [];
  for (const r of rows)
    links.push({
      client: clientName(r.client),
      provider: slugOf.get(r.provider) ?? r.provider,
      account: r.account,
      model: r.model,
      combo: r.combo,
      requests: r.requests,
      errors: r.errors,
      input_tokens: r.input,
      output_tokens: r.output,
      cache_read_tokens: r.cacheRead,
      cost_usd: costOf(await price(r.provider, r.model), r.input, r.output, r.cacheRead),
    });
  return { links, since_ms: sinceMs };
}

/**
 * What is moving right now: requests in flight (in memory) and the ones logged since `after`.
 * `after` is a call_logs rowid; the first call only learns the cursor, so opening the page never
 * replays old traffic. Metadata only: no prompt or reply text leaves this function.
 */
export async function statsLive(after: number | null) {
  const db = getDbInstance();
  const pending = [...getPendingById().values()].filter((d) => !d.stale);
  const cursor = (db.prepare("SELECT MAX(rowid) AS id FROM call_logs").get() as any)?.id ?? 0;
  const logged =
    after == null
      ? []
      : (db
          .prepare(
            `SELECT rowid AS seq, COALESCE(api_key_name, '') AS client, provider, COALESCE(account, '') AS account,
               connection_id AS connectionId, model, status, COALESCE(tokens_in, 0) AS input,
               COALESCE(tokens_out, 0) AS output, COALESCE(duration, 0) AS duration
             FROM call_logs WHERE rowid > ? AND model NOT IN ${INTERNAL_MODELS} ORDER BY rowid LIMIT 200`
          )
          .all(after) as any[]);
  if (pending.length === 0 && logged.length === 0) return { now_ms: Date.now(), cursor, active: [], done: [] };
  const providers = await snapshot();
  const slugOf = new Map(providers.map((p) => [p.id, p.slug]));
  const aliasOf = new Map(providers.flatMap((p) => p.conns.map((c) => [c.id, c.name] as const)));
  return {
    now_ms: Date.now(),
    cursor,
    active: pending.map((d) => ({
      id: d.id,
      client: clientName(d.apiKeyName),
      provider: slugOf.get(d.provider) ?? d.provider,
      account: (d.connectionId && aliasOf.get(d.connectionId)) || "",
      model: d.model,
      started_ms: d.startedAt,
      answering: d.stage === "provider_response_started" || (d.streamedBytes ?? 0) > 0,
      bytes: d.streamedBytes ?? 0,
    })),
    done: logged.map((r) => ({
      seq: r.seq,
      client: clientName(r.client),
      provider: slugOf.get(r.provider) ?? r.provider ?? "",
      account: r.account || (r.connectionId && aliasOf.get(r.connectionId)) || "",
      model: r.model,
      ok: r.status > 0 && r.status < 400,
      input_tokens: r.input,
      output_tokens: r.output,
      duration_ms: r.duration,
    })),
  };
}

export async function requestsRecent(filter: any) {
  const where: string[] = [`model NOT IN ${INTERNAL_MODELS}`];
  const args: unknown[] = [];
  if (filter?.since_ms) {
    where.push("timestamp >= ?");
    args.push(iso(filter.since_ms));
  }
  if (filter?.client) {
    where.push("api_key_name = ?");
    args.push(filter.client);
  }
  if (filter?.status === "error") where.push("status >= 400");
  if (filter?.search) {
    where.push("(model LIKE ? OR requested_model LIKE ? OR account LIKE ? OR api_key_name LIKE ?)");
    const like = `%${filter.search}%`;
    args.push(like, like, like, like);
  }
  const limit = Math.min(1000, Math.max(1, Number(filter?.limit ?? 100)));
  const providers = await snapshot();
  const price = await priceIndex(providers);
  const slugOf = new Map(providers.map((p) => [p.id, p.slug]));
  const rows = getDbInstance()
    .prepare(
      `SELECT * FROM call_logs ${where.length ? "WHERE " + where.join(" AND ") : ""} ORDER BY timestamp DESC LIMIT ?`
    )
    .all(...args, limit) as any[];
  // Migrated pools keep an internal combo name; clients know them by the mapped model name.
  const poolName = new Map(
    (
      getDbInstance()
        .prepare(
          "SELECT c.name AS combo, m.pattern AS pattern FROM model_combo_mappings m JOIN combos c ON c.id = m.combo_id"
        )
        .all() as any[]
    ).map((m) => [m.combo, m.pattern])
  );
  const out = [];
  for (const r of rows) {
    const slug = slugOf.get(r.provider) ?? r.provider ?? null;
    const actions = (() => {
      try {
        return JSON.parse(r.resilience_actions ?? "[]");
      } catch {
        return [];
      }
    })();
    const requested = String(r.requested_model ?? r.model ?? "");
    out.push({
      id: r.id,
      ts_ms: Date.parse(r.timestamp),
      client: r.api_key_name ? clientName(r.api_key_name) : null,
      requested_model: requested.replace(/^fusion-/, ""),
      route: r.account ? `${slug}/${r.account}/${r.model}` : null,
      protocol_in: r.source_format ?? "chat",
      protocol_out: r.target_format ?? null,
      mode: r.combo_name ? "pool" : "direct",
      stream: false,
      status: r.status >= 400 ? "error" : r.status > 0 ? "ok" : "running",
      error_category: r.error_type ?? (r.status >= 400 ? String(r.status) : null),
      attempts: Array.isArray(actions) ? actions.length + 1 : 1,
      input_tokens: r.tokens_in ?? null,
      output_tokens: r.tokens_out ?? null,
      ttft_ms: r.ttft_ms ?? null,
      duration_ms: r.duration ?? 0,
      cost_usd: costOf(
        await price(r.provider, r.model),
        r.tokens_in ?? 0,
        r.tokens_out ?? 0,
        r.tokens_cache_read ?? 0
      ),
      decision: r.combo_name
        ? `${poolName.get(r.combo_name) ?? r.combo_name}${Array.isArray(actions) && actions.length ? `：${actions.length} 次切换` : ""}`
        : null,
    });
  }
  return out;
}

function spend(connId: string, sinceMs: number) {
  const r = getDbInstance()
    .prepare(
      `SELECT provider, model, COUNT(*) AS requests, SUM(COALESCE(tokens_in,0)) AS input,
         SUM(COALESCE(tokens_out,0)) AS output, SUM(COALESCE(tokens_cache_read,0)) AS cached
       FROM call_logs WHERE connection_id = ? AND timestamp >= ? GROUP BY provider, model`
    )
    .all(connId, iso(sinceMs)) as any[];
  return r;
}

type UsageEntry = {
  fetched_at_ms: number;
  error: string | null;
  plan: string | null;
  windows: any[];
  balances: any[];
  supported: boolean;
};

function mapUsage(usage: any): UsageEntry {
  const windows: any[] = [];
  const balances: any[] = [];
  for (const [key, q] of Object.entries<any>(usage?.quotas ?? {})) {
    if (q?.currency || key.startsWith("credits")) {
      balances.push({
        currency: q.currency ?? key.replace("credits_", "").toUpperCase(),
        amount: Number(q.remaining ?? 0),
        included: null,
        topup: q.toppedUpBalance ?? null,
        granted: q.grantedBalance ?? null,
      });
      continue;
    }
    windows.push({
      scope: key,
      label: key,
      unit: q.total ? "count" : "percent",
      used: q.used ?? null,
      limit: q.total ?? null,
      remaining_percent:
        typeof q.remainingPercentage === "number" ? q.remainingPercentage : null,
      window_seconds: null,
      resets_at_ms: q.resetAt ? Date.parse(q.resetAt) || null : null,
    });
  }
  return {
    fetched_at_ms: Date.now(),
    error: usage?.message ?? null,
    plan: typeof usage?.plan === "string" ? usage.plan : null,
    windows,
    balances,
    supported: !usage?.message || windows.length + balances.length > 0,
  };
}

export async function usageList(refresh: boolean) {
  const providers = await snapshot();
  const price = await priceIndex(providers);
  const cache = kvGet<Record<string, UsageEntry>>("usageCache", {});
  const plans = kvGet<Record<string, any>>("plans", {});
  const now = new Date();
  const day = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const month = new Date(now.getFullYear(), now.getMonth(), 1).getTime();
  const conns = providers.flatMap((p) => p.conns.map((conn) => ({ p, conn })));
  if (refresh) {
    const queue = conns.filter(({ p, conn }) => p.kind.usage && conn.isActive !== false);
    const worker = async () => {
      for (;;) {
        const next = queue.shift();
        if (!next) return;
        try {
          const full = (await getProviderConnectionById(next.conn.id)) as any;
          // A named native fetcher reads the platform's own quota API with this account's key.
          const usage = await getUsageForProvider(
            { ...full, provider: next.p.kind.usage },
            { forceRefresh: true }
          );
          cache[next.conn.id] = mapUsage(usage);
        } catch (e) {
          cache[next.conn.id] = {
            fetched_at_ms: Date.now(),
            error: (e as Error).message,
            plan: null,
            windows: [],
            balances: [],
            supported: true,
          };
        }
      }
    };
    await Promise.all(Array.from({ length: 4 }, worker));
    kvSet("usageCache", cache);
  }
  const total = async (id: string, since: number) => {
    const out = { requests: 0, input_tokens: 0, output_tokens: 0, cost_usd: 0 };
    for (const r of spend(id, since)) {
      out.requests += r.requests;
      out.input_tokens += r.input;
      out.output_tokens += r.output;
      out.cost_usd += costOf(await price(r.provider, r.model), r.input, r.output, r.cached) ?? 0;
    }
    return out;
  };
  const out = [];
  for (const { p, conn } of conns) {
    const entry = cache[conn.id];
    const own = plans[`${p.slug}/${conn.name}`];
    out.push({
      provider: p.slug,
      alias: conn.name,
      account_type: conn.authType === "oauth" ? "subscription" : "api_key",
      supported: !!p.kind.usage && (entry?.supported ?? true),
      fetched_at_ms: entry?.fetched_at_ms ?? null,
      source: p.kind.usage ? "OFFICIAL_API" : null,
      confidence: p.kind.usage ? "EXACT" : null,
      error: entry?.error ?? null,
      account_label: null,
      plan: own ?? (entry?.plan ? { name: entry.plan } : null),
      windows: entry?.windows ?? [],
      balances: entry?.balances ?? [],
      spend_today: await total(conn.id, day),
      spend_month: await total(conn.id, month),
    });
  }
  return out;
}
