// SPDX-License-Identifier: Apache-2.0
// One entry point for the desktop UI, named after the daemon's IPC methods.
import * as accounts from "./accounts";
import * as autostart from "./autostart";
import * as clients from "./clients";
import * as clientConfig from "./clientConfig";
import * as features from "./features";
import * as keys from "./keys";
import * as models from "./models";
import * as routing from "./routing";
import * as stats from "./stats";
import * as system from "./system";
import * as updates from "./updates";
import { nativeClient, RpcError, snapshot, type Native } from "./shared";
import { listSubscriptions, loginStatus, startLogin, transport } from "../subscription";

const TRANSPORT_RISK = "I understand";

type Handler = (p: any, native: Native) => unknown;
const METHODS: Record<string, Handler> = {
  "daemon.status": () => system.daemonStatus(),
  "daemon.stop": () => system.daemonControl("stop"),
  "daemon.restart": () => system.daemonControl("restart"),

  "provider.kinds": () => accounts.providerKinds(),
  "provider.list": () => accounts.providerList(),
  "provider.add": (p, n) => accounts.providerAdd(n, p),
  "provider.remove": (p, n) => accounts.providerRemove(n, p.slug),
  "provider.set_settings": (p) => accounts.providerSettings(p.slug, p.settings),
  "provider.reorder": (p) => accounts.providerReorder(p.slugs),

  "account.list": () => accounts.accountList(),
  "account.add": (p, n) => accounts.accountAdd(n, p),
  "account.reorder": (p, n) => accounts.accountReorder(n, p.provider, p.aliases),
  "account.set_enabled": (p, n) => accounts.accountEnabled(n, p.provider, p.alias, p.enabled),
  "account.set_key": (p, n) => accounts.accountKey(n, p.provider, p.alias, p.api_key),
  "account.remove": (p, n) => accounts.accountRemove(n, p.provider, p.alias),
  "account.set_plan": (p) => accounts.accountPlan(p),
  "account.check": (p, n) => accounts.accountCheck(n, p.provider, p.alias, !!p.models),
  "account.oauth_start": async (p, n) => {
    // The subscription provider is an ordinary Responses node, created on first sign-in.
    let provider = (await snapshot()).find((x) => x.slug === "codex-sub");
    if (!provider) {
      await accounts.providerAdd(n, { kind: "codex-subscription" });
      provider = (await snapshot()).find((x) => x.slug === "codex-sub");
    }
    const r: any = await startLogin({ provider: provider?.id, alias: p.alias });
    return {
      login_id: r.id,
      verification_url: r.verificationUrl,
      user_code: r.userCode,
      expires_in_secs: Math.max(0, Math.round((r.expiresAt - Date.now()) / 1000)),
    };
  },
  "account.oauth_status": async (p) => {
    const r: any = loginStatus(p.login_id);
    if (r.state !== "complete")
      return {
        state: r.state === "pending" ? "pending" : "failed",
        account: null,
        message: r.failure ?? (r.state === "pending" ? null : r.state),
      };
    const providers = await snapshot();
    const owner = providers.find((x) => x.conns.some((c) => c.id === r.connectionId));
    const alias = owner?.conns.find((c) => c.id === r.connectionId)?.name;
    const list = await accounts.accountInfos(providers);
    return {
      state: "done",
      account: list.find((a) => a.provider === owner?.slug && a.alias === alias) ?? null,
      message: null,
    };
  },
  "account.set_transport": async (p) => {
    const id = await accounts.subscriptionAccountId(p.provider, p.alias);
    await transport(id, p.enabled, { risk: TRANSPORT_RISK, alias: p.alias });
    return (await accounts.accountList()).find((a) => a.provider === p.provider && a.alias === p.alias);
  },
  "subscription.list": () => listSubscriptions(),

  "model.list": () => models.listModels(),
  "model.sync": (p, n) => models.syncAccountModels(n, p.provider, p.alias),
  "model.add": (p) => models.modelAdd(p),
  "model.set_enabled": (p) => models.modelEnabled(p.name, p.enabled),
  "model.set_enabled_many": (p) => models.modelEnabledMany(p?.names, p?.enabled === true),
  "model.set_price": (p) => models.modelPrice(p.name, p.input, p.output),
  "model.set_tier": (p) => models.modelTier(p.name, p.tier),
  "model.remove": (p) => models.modelRemove(p.name),
  "price.compare": (p) => models.priceCompare(p?.model ?? null),

  "route.status": () => routing.routeStatus(),
  "route.virtuals": (_p, n) => routing.virtuals(n),
  "route.virtual_set": (p, n) => routing.virtualSet(n, p.name, p.enabled),
  "bench.run": (p, n) => routing.benchRun(n, p?.names ?? []),
  "pool.list": () => routing.listPools(),
  "pool.save": (p, n) => routing.poolSave(n, p.name, p.ordering, p.members),
  "pool.remove": (p, n) => routing.poolRemove(n, p.name),

  "subpool.list": () => routing.subpoolState(),
  "subpool.save": (p) => routing.subpoolSave(p.name, p.accounts, p.rename_from ?? null),
  "subpool.remove": (p) => routing.subpoolRemove(p.name),
  "subpool.activate": (p) => routing.subpoolActivate(p.name ?? null),

  "endpoint.list": () => routing.endpointInfos(),
  "endpoint.save": (p, n) => routing.endpointSave(n, p),
  "endpoint.remove": (p) => routing.endpointRemove(p.name),

  "client.list": (_p, n) => clients.clientInfos(n),
  "client.create": (p, n) => clients.clientCreate(n, p.name),
  "client.revoke": (p, n) => clients.clientRevoke(n, p.name),
  "client.set_policy": (p, n) => clients.clientPolicy(n, p.name, p.policy),
  "client.set_mcp": (p, n) => clients.clientMcp(n, p.name, p.enabled === true),
  "client.tools": () => clientConfig.clientTools(),
  "client.connect_tool": (p, n) => clientConfig.connectClientTool(n, String(p?.tool), String(p?.model ?? ""), p?.mcp === true),

  "stats.summary": (p) => stats.statsSummary(p.since_ms, p.group_by, p.until_ms, p.tz_offset_min),
  "stats.flow": (p) => stats.statsFlow(p?.since_ms ?? 0),
  "stats.live": (p) => stats.statsLive(p?.after ?? null),
  "requests.recent": (p) => stats.requestsRecent(p),
  "usage.list": (p) => stats.usageList(!!p?.refresh),

  "features.list": () => features.featureList(),
  "features.set": (p) => features.featureSet(String(p?.key), p?.enabled === true),
  "features.set_option": (p) => features.featureOption(String(p?.key), String(p?.option), String(p?.value)),

  "settings.list": () => system.settingsList(),
  "settings.set": (p, n) => system.settingSet(n, p.key, p.value),
  "settings.reset": (p, n) => system.settingReset(n, p.key),
  "backup.database": (p) => system.backupDatabase(p),
  "update.status": () => updates.updateStatus(),
  "update.check": () => updates.updateStart("check"),
  "update.download": () => updates.updateStart("download"),
  "update.install": () => updates.updateStart("install"),
  "update.set_auto_check": (p) => updates.updateAutoCheck(p.enabled),
  "update.open_notes": () => updates.updateOpenNotes(),
  "update.install_pack": async () => {
    const { aiPackState } = await import("../aiPack");
    return updates.packInstall(aiPackState().installed);
  },
  "system.open_url": (p) => system.openSignInPage(p.url),
  "system.autostart": () => autostart.autostartStatus(),
  "system.set_autostart": (p) => autostart.autostartSet(p?.enabled === true),

  "discovery.scan": () => system.scanTools(),
  "discovery.connect": (p) => system.connectTool(p.kind),

  "keys.export": (p) => keys.exportKeys(p),
  "keys.import": (p, n) => keys.importKeys(n, p),
};

export async function dispatch(request: Request, method: string, params: unknown) {
  const handler = METHODS[method];
  if (!handler) throw new RpcError("unknown_method", `未知方法 ${method}`);
  return handler(params ?? {}, nativeClient(request));
}
