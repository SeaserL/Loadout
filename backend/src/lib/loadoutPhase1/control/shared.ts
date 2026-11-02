// SPDX-License-Identifier: Apache-2.0
// Loadout control plane on OmniRoute: reads SQLite, writes through native REST so that
// validation and the vault boundary stay in one place. Personal state lives in key_value.
import { getDbInstance } from "../../db/core";
import { getRawProviderConnections } from "../../db/providers";
import { getProviderNodes } from "../../db/providers/nodes";
import { kindOf, type Kind } from "./kinds";

const NAMESPACE = "loadout";

export class RpcError extends Error {
  constructor(
    public code: string,
    message: string
  ) {
    super(message);
  }
}
export const invalid = (message: string) => new RpcError("invalid_params", message);
export const missing = (message: string) => new RpcError("not_found", message);

export function kvGet<T>(key: string, fallback: T): T {
  const row = getDbInstance()
    .prepare("SELECT value FROM key_value WHERE namespace = ? AND key = ?")
    .get(NAMESPACE, key) as { value?: string } | undefined;
  if (!row?.value) return fallback;
  try {
    return JSON.parse(row.value) as T;
  } catch {
    return fallback;
  }
}
export function kvSet(key: string, value: unknown) {
  getDbInstance()
    .prepare("INSERT OR REPLACE INTO key_value (namespace, key, value) VALUES (?, ?, ?)")
    .run(NAMESPACE, key, JSON.stringify(value));
}

export type Native = <T = any>(path: string, method?: string, body?: unknown) => Promise<T>;
export function nativeClient(request: Request): Native {
  const base = `http://127.0.0.1:${process.env.PORT}`;
  const cookie = request.headers.get("cookie") ?? "";
  const csrf = request.headers.get("x-omniroute-csrf") ?? "";
  return async <T>(path: string, method = "GET", body?: unknown) => {
    const response = await fetch(base + path, {
      method,
      headers: {
        "content-type": "application/json",
        cookie,
        origin: base,
        "x-omniroute-csrf": csrf,
      },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(120000),
    });
    const text = await response.text();
    let data: any = null;
    try {
      data = text ? JSON.parse(text) : null;
    } catch {
      data = null;
    }
    if (!response.ok) {
      const message =
        (typeof data?.error === "string" && data.error) ||
        data?.error?.message ||
        data?.message ||
        `HTTP ${response.status}`;
      throw new RpcError(response.status === 404 ? "not_found" : "native", String(message));
    }
    return data as T;
  };
}

export type Conn = Record<string, any> & {
  id: string;
  provider: string;
  name: string;
  priority?: number;
  isActive?: boolean;
};
export type Provider = {
  slug: string;
  id: string;
  kind: Kind;
  display: string;
  baseUrl: string | null;
  apiType: string;
  node: Record<string, any> | null;
  settings: Record<string, unknown>;
  conns: Conn[];
};
type ProviderMeta = { kind?: string; display_name?: string; settings?: Record<string, unknown> };

export const providerMeta = () => kvGet<Record<string, ProviderMeta>>("providers", {});
export const providerOrder = () => kvGet<string[]>("providerOrder", []);

/** Providers (one per native node or built-in id) with their accounts in failover order. */
export async function snapshot(): Promise<Provider[]> {
  const nodes = (await getProviderNodes()) as Record<string, any>[];
  const conns = (await getRawProviderConnections({})) as Conn[];
  const meta = providerMeta();
  const seen = new Map<string, Provider>();
  for (const node of nodes) {
    const slug = String(node.name);
    const m = meta[slug] ?? {};
    const kind = kindOf(m.kind ?? slug, node.baseUrl);
    seen.set(node.id, {
      slug,
      id: node.id,
      kind,
      display: m.display_name ?? kind.display_name ?? slug,
      baseUrl: node.baseUrl ?? null,
      apiType: node.apiType ?? "chat",
      node,
      settings: m.settings ?? {},
      conns: [],
    });
  }
  for (const conn of conns) {
    let p = seen.get(conn.provider);
    if (!p) {
      const m = meta[conn.provider] ?? {};
      const kind = kindOf(m.kind ?? conn.provider, null);
      p = {
        slug: conn.provider,
        id: conn.provider,
        kind,
        display: m.display_name ?? kind.display_name ?? conn.provider,
        baseUrl: kind.default_base_url,
        apiType: "chat",
        node: null,
        settings: m.settings ?? {},
        conns: [],
      };
      seen.set(conn.provider, p);
    }
    p.conns.push(conn);
  }
  const order = providerOrder();
  const rank = (slug: string) => {
    const i = order.indexOf(slug);
    return i < 0 ? order.length : i;
  };
  const list = [...seen.values()];
  for (const p of list)
    p.conns.sort((a, b) => (a.priority ?? 0) - (b.priority ?? 0));
  return list.sort((a, b) => rank(a.slug) - rank(b.slug) || a.display.localeCompare(b.display));
}

export function findProvider(providers: Provider[], slug: string) {
  const p = providers.find((x) => x.slug === slug);
  if (!p) throw missing(`没有找到服务商 ${slug}`);
  return p;
}
export function findAccount(providers: Provider[], slug: string, alias: string) {
  const p = findProvider(providers, slug);
  const conn = p.conns.find((c) => c.name === alias);
  if (!conn) throw missing(`没有找到账号 ${slug}/${alias}`);
  return { provider: p, conn };
}

/** Splits `provider/alias/model`; the model part may itself contain slashes. */
export function splitRoute(providers: Provider[], name: string) {
  const [slug, alias, ...rest] = name.split("/");
  if (!slug || !alias || rest.length === 0) throw invalid(`无效的路由名 ${name}`);
  const { provider, conn } = findAccount(providers, slug, alias);
  return { provider, conn, model: rest.join("/") };
}

// OmniRoute records its own connection tests and model syncs in call_logs under these names.
export const INTERNAL_MODELS = "('connection-test', 'model-sync')";
export const PROBE_CLIENT = "Internal self-loop";
export const clientName = (name: string | null | undefined) =>
  name === PROBE_CLIENT ? "Loadout 测速" : (name ?? "");

export const isSubscription = (conn: Conn) =>
  conn.authType === "oauth" || conn.providerSpecificData?.credentialProfile === "loadout-codex-v1";

export const instanceDir = () => {
  const data = process.env.DATA_DIR;
  if (!data) throw new RpcError("internal", "DATA_DIR missing");
  return data.replace(/[\\/]data[\\/]?$/, "");
};
