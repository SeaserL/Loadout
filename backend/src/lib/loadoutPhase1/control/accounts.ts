// SPDX-License-Identifier: Apache-2.0
// Providers and accounts. Order of a provider's accounts is native `priority`: the failover order.
import { getProviderConnectionById } from "../../db/providers";
import { KINDS } from "./kinds";
import { listModels, syncAccountModels } from "./models";
import {
  findAccount,
  findProvider,
  invalid,
  isSubscription,
  kvGet,
  kvSet,
  providerMeta,
  providerOrder,
  snapshot,
  type Conn,
  type Native,
  type Provider,
} from "./shared";
import { benchTargets } from "./routing";

const SLUG = /^[a-z0-9][a-z0-9-]{0,40}$/;
const ALIAS = /^[A-Za-z0-9][A-Za-z0-9._-]{0,60}$/;

export const providerInfo = (p: Provider) => ({
  slug: p.slug,
  kind: p.kind.kind,
  display_name: p.display,
  base_url: p.baseUrl,
  settings: p.settings,
  accounts: p.conns.length,
});

function accountType(conn: Conn) {
  return isSubscription(conn) ? "subscription" : "api_key";
}
export async function accountInfos(providers: Provider[]) {
  const models = await listModels(providers);
  const out: any[] = [];
  const plans = kvGet<Record<string, unknown>>("plans", {});
  for (const p of providers)
    p.conns.forEach((conn, index) => {
      const prefix = `${p.slug}/${conn.name}/`;
      const mine = models.filter((m) => m.name.startsWith(prefix));
      const psd = conn.providerSpecificData ?? {};
      out.push({
        provider: p.slug,
        alias: conn.name,
        display_name: conn.displayName ?? null,
        account_type: accountType(conn),
        enabled: conn.isActive !== false,
        position: index,
        has_key: !!conn.apiKey || !!conn.accessToken,
        models: mine.length,
        enabled_models: mine.filter((m) => m.enabled).length,
        transport_opt_in: psd.transportOptIn === true,
        login_expires_at_ms: conn.tokenExpiresAt ? Date.parse(conn.tokenExpiresAt) || null : null,
        unavailable_models: mine
          .filter((m) => m.disabled_reason === "not_in_plan")
          .map((m) => m.upstream_model),
        plan: plans[`${p.slug}/${conn.name}`] ?? null,
      });
    });
  return out;
}

async function info(slug: string, alias: string) {
  const providers = await snapshot();
  const all = await accountInfos(providers);
  return all.find((a) => a.provider === slug && a.alias === alias);
}

export const providerKinds = () =>
  KINDS.map(({ usage: _usage, ...kind }) => ({
    ...kind,
    usage_sources: kind.usage_sources,
  }));

export async function providerList() {
  const providers = await snapshot();
  return providers.map(providerInfo);
}

export async function providerAdd(native: Native, p: any) {
  const kind = KINDS.find((k) => k.kind === p.kind);
  if (!kind) throw invalid("未知的服务商类型");
  const slug = String(kind.fixed_slug ?? p.slug ?? "");
  if (!SLUG.test(slug)) throw invalid("名称只能使用小写字母、数字和连字符");
  const baseUrl = String(p.base_url ?? kind.default_base_url ?? "");
  if (!/^https?:\/\//.test(baseUrl)) throw invalid("需要填写 Base URL");
  const providers = await snapshot();
  if (providers.some((x) => x.slug === slug)) throw invalid(`${slug} 已存在`);
  await native("/api/provider-nodes", "POST", {
    type: "openai-compatible",
    name: slug,
    prefix: "fusion-" + slug,
    apiType: kind.api_type ?? "chat",
    baseUrl,
  });
  const meta = providerMeta();
  meta[slug] = {
    kind: kind.kind,
    display_name: p.display_name ?? (kind.fixed_slug ? kind.display_name : slug),
    settings: p.settings ?? {},
  };
  kvSet("providers", meta);
  return providerInfo(findProvider(await snapshot(), slug));
}

export async function providerRemove(native: Native, slug: string) {
  const p = findProvider(await snapshot(), slug);
  for (const conn of p.conns) await native(`/api/providers/${conn.id}`, "DELETE");
  if (p.node) await native(`/api/provider-nodes/${p.node.id}`, "DELETE");
  const meta = providerMeta();
  delete meta[slug];
  kvSet("providers", meta);
  return { revoked: true };
}

export async function providerSettings(slug: string, settings: Record<string, unknown>) {
  const p = findProvider(await snapshot(), slug);
  const meta = providerMeta();
  meta[slug] = { ...(meta[slug] ?? {}), settings };
  kvSet("providers", meta);
  return providerInfo({ ...p, settings });
}

export async function providerReorder(slugs: string[]) {
  if (!Array.isArray(slugs)) throw invalid("顺序无效");
  kvSet("providerOrder", slugs);
  return providerList();
}

export async function accountList() {
  return accountInfos(await snapshot());
}

export async function accountAdd(native: Native, p: any) {
  const alias = String(p.alias ?? "");
  if (!ALIAS.test(alias)) throw invalid("账号别名只能使用字母、数字、点、下划线和连字符");
  const providers = await snapshot();
  const provider = findProvider(providers, p.provider);
  if (provider.conns.some((c) => c.name === alias)) throw invalid(`${alias} 已存在`);
  if (!p.api_key && provider.kind.auth === "api_key") throw invalid("需要填写 API key");
  const body: Record<string, unknown> = {
    provider: provider.id,
    name: alias,
    apiKey: p.api_key ?? undefined,
    priority: Math.min(provider.conns.length + 1, 100),
  };
  if (provider.node)
    body.providerSpecificData = {
      baseUrl: provider.baseUrl,
      apiType: provider.apiType,
      prefix: provider.node.prefix,
      nodeName: provider.slug,
    };
  const created = await native<any>("/api/providers", "POST", body);
  const id = created?.connection?.id ?? created?.id;
  await native(`/api/providers/${id}`, "PUT", {
    isActive: true,
    testStatus: "active",
    healthCheckInterval: 0,
    ...(p.display_name ? { displayName: p.display_name } : {}),
  });
  let sync = null;
  let sync_error = null;
  try {
    if (p.sync_models !== false) sync = await syncAccountModels(native, provider.slug, alias);
  } catch (e) {
    sync_error = (e as Error).message;
  }
  return { account: await info(provider.slug, alias), sync, sync_error };
}

export async function accountReorder(native: Native, slug: string, aliases: string[]) {
  const p = findProvider(await snapshot(), slug);
  for (const [index, alias] of aliases.entries()) {
    const conn = p.conns.find((c) => c.name === alias);
    if (conn && conn.priority !== index + 1)
      await native(`/api/providers/${conn.id}`, "PUT", { priority: index + 1 });
  }
  return accountList();
}

export async function accountEnabled(native: Native, slug: string, alias: string, enabled: boolean) {
  const { conn } = findAccount(await snapshot(), slug, alias);
  await native(`/api/providers/${conn.id}`, "PUT", { isActive: enabled });
  return info(slug, alias);
}

export async function accountKey(native: Native, slug: string, alias: string, apiKey: string) {
  if (!apiKey) throw invalid("需要填写 API key");
  const { conn } = findAccount(await snapshot(), slug, alias);
  await native(`/api/providers/${conn.id}`, "PUT", { apiKey });
  return info(slug, alias);
}

export async function accountRemove(native: Native, slug: string, alias: string) {
  const { conn } = findAccount(await snapshot(), slug, alias);
  await native(`/api/providers/${conn.id}`, "DELETE");
  return { revoked: true };
}

export async function accountPlan(p: any) {
  const plans = kvGet<Record<string, unknown>>("plans", {});
  const key = `${p.provider}/${p.alias}`;
  if (p.name == null && p.price_usd_monthly == null && p.renews_at_ms == null) delete plans[key];
  else
    plans[key] = {
      name: p.name ?? null,
      price_usd_monthly: p.price_usd_monthly ?? null,
      renews_at_ms: p.renews_at_ms ?? null,
    };
  kvSet("plans", plans);
  return info(p.provider, p.alias);
}

const CHECK_STATUS: [RegExp, string][] = [
  [/401|403|unauthor|forbidden|invalid.*key|api key|expired|auth/i, "invalid_key"],
  [/429|rate.?limit|too many/i, "rate_limited"],
  [/quota|credit|balance|insufficient/i, "no_credit"],
  [/timeout|timed out|fetch failed|econn|enotfound|network|egress/i, "unreachable"],
];

export async function accountCheck(native: Native, slug: string, alias: string, models: boolean) {
  const providers = await snapshot();
  const { conn } = findAccount(providers, slug, alias);
  const started = Date.now();
  let status = "ok";
  let message: string | null = null;
  try {
    const r = await native<any>(`/api/providers/${conn.id}/test`, "POST", {});
    if (r?.valid === false || r?.error) {
      message = String(r.error ?? r.diagnosis?.message ?? "检查未通过");
      status = CHECK_STATUS.find(([re]) => re.test(message as string))?.[1] ?? "upstream_error";
    }
  } catch (e) {
    message = (e as Error).message;
    status = CHECK_STATUS.find(([re]) => re.test(message as string))?.[1] ?? "upstream_error";
  }
  const checks: any[] = [];
  if (models) {
    const prefix = `${slug}/${alias}/`;
    const names = (await listModels(providers))
      .filter((m) => m.enabled && m.name.startsWith(prefix))
      .map((m) => m.name);
    for (const r of await benchTargets(providers, names))
      checks.push({
        name: r.name,
        protocol: r.protocol,
        ok: r.ok,
        ttft_ms: r.ttft_ms,
        duration_ms: r.duration_ms,
        error: r.error,
        stream: null,
      });
  }
  return {
    account: `${slug}/${alias}`,
    status,
    http_status: null,
    message,
    models_listed: null,
    duration_ms: Date.now() - started,
    cleared: status === "ok",
    models: checks,
  };
}

export async function subscriptionAccountId(slug: string, alias: string) {
  const { conn } = findAccount(await snapshot(), slug, alias);
  return conn.id;
}

/** Plaintext key for one account: only the encrypted export and usage fetchers use this. */
export async function readAccountSecret(id: string) {
  const conn = (await getProviderConnectionById(id)) as Conn | null;
  return conn?.apiKey ?? null;
}

export const orderedSlugs = providerOrder;
