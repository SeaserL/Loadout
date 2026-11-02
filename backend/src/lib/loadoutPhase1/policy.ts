// SPDX-License-Identifier: Apache-2.0
// Compile Loadout constraints; OmniRoute remains the only account selector/executor.
import { enabled } from "./secretStore";
type Account = { id: string; provider: string; name: string; isActive?: boolean };
const pins = new WeakMap<Request, string>();
const preferred = new WeakMap<Request, string>();
const scopes = new WeakMap<Request, string[]>();
export const preferredAccount = (request: Request) => preferred.get(request);
function accountAliases(accounts: Account[]) {
  const next = new Map<string, string>();
  for (const a of accounts) {
    const address = `${a.provider}/${a.name}`;
    if (next.has(address)) throw new Error("DUPLICATE_ACCOUNT_ALIAS");
    next.set(address, a.id);
  }
  return next;
}
export function compileAccountScope(ids: string[] | null) {
  // OmniRoute [] means unscoped. Explicit Loadout empty sets must stay empty.
  return ids?.length === 0 ? ["__loadout_no_eligible_account__"] : ids;
}
const reject = (status: number, message: string) =>
  Response.json({ error: { message, type: "invalid_request_error" } }, { status });
function intersect(scope: string[] | null, next: string[]) {
  return scope ? scope.filter((id) => next.includes(id)) : next;
}

export async function prepareRequest(request: Request): Promise<Request | Response> {
  if (!enabled()) return request;
  const { getRawProviderConnections } = await import("../db/providers");
  const { getProviderNodes } = await import("../db/providers/nodes");
  const { kvGet } = await import("./control/shared");
  const accounts = (await getRawProviderConnections({})) as Account[];
  const aliases = accountAliases(accounts);
  const nodes = (await getProviderNodes()) as { id: string; name: string; prefix?: string }[];
  // Loadout addresses use the provider's slug; native ids keep working.
  const providerIds = new Map<string, string>();
  for (const a of accounts) providerIds.set(a.provider, a.provider);
  for (const n of nodes) {
    providerIds.set(n.id, n.id);
    providerIds.set(n.name, n.id);
  }
  const slugOf = new Map(nodes.map((n) => [n.id, n.name]));
  let body: any;
  try {
    body = await request.clone().json();
  } catch {
    return request;
  }
  if (body.previous_response_id || body.conversation)
    return reject(400, "Stateful Responses are not enabled in this trial");
  const headers = new Headers(request.headers);
  // Do not let a caller bypass the named-account policy with an engine-specific header.
  headers.delete("x-omniroute-connection");
  const endpointName = headers.get("x-loadout-endpoint");
  headers.delete("x-loadout-endpoint");

  // A sub-pool is an account scope for every entry point, exact names included.
  let scope: string[] | null = null;
  const subpools = kvGet<{ active: string | null; pools: Record<string, string[]> }>("subpools", {
    active: null,
    pools: {},
  });
  if (subpools.active) {
    const members = new Set(subpools.pools[subpools.active] ?? []);
    scope = accounts
      .filter((a) => members.has(`${slugOf.get(a.provider) ?? a.provider}/${a.name}`))
      .map((a) => a.id);
  }

  const disabled = kvGet<Record<string, { enabled?: boolean }>>("modelOverrides", {});
  const switchedOff = (provider: string, alias: string, model: string) =>
    disabled[`${slugOf.get(provider) ?? provider}/${alias}/${model}`]?.enabled === false;

  let requested = typeof body.model === "string" ? body.model : "";
  if (endpointName) {
    const endpoint = kvGet<any[]>("endpoints", []).find((e) => e.name === endpointName && e.enabled);
    if (!endpoint) return reject(404, "Unknown or disabled endpoint");
    const providerId = providerIds.get(endpoint.provider);
    if (!providerId) return reject(404, "Endpoint provider is gone");
    const members = accounts.filter(
      (a) =>
        a.provider === providerId &&
        (!endpoint.members || endpoint.members.includes(a.name))
    );
    scope = intersect(
      scope,
      members.map((a) => a.id)
    );
    if (!requested) requested = endpoint.default_model ?? "";
    if (!requested) return reject(400, "This endpoint needs a model name");
    if (requested.startsWith(`${endpoint.provider}/`))
      requested = requested.slice(endpoint.provider.length + 1);
    body.model = `${endpoint.provider}/${requested}`;
    requested = body.model;
  }

  const address = requested.split("/");
  let id: string | undefined;
  const { resolveComboForModel } = await import("../db/modelComboMappings");
  const mapped = requested ? await resolveComboForModel(requested) : null;
  if (!mapped && address.length >= 2) {
    const providerId = providerIds.get(address[0]);
    if (address.length >= 3) {
      id = providerId ? aliases.get(`${providerId}/${address[1]}`) : undefined;
      if (!id && aliases.size && providerId && !endpointName)
        return reject(400, "Unknown account alias");
      if (id) {
        const model = address.slice(2).join("/");
        if (switchedOff(providerId as string, address[1], model))
          return reject(400, "This model is switched off in Loadout");
        body.model = `${providerId}/${model}`;
      } else if (providerId) body.model = `${providerId}/${address.slice(1).join("/")}`;
    } else if (providerId) {
      const model = address[1];
      body.model = `${providerId}/${model}`;
      // Provider-wide names still fail over, but never to an account that turned the model off.
      const off = accounts.filter(
        (a) => a.provider === providerId && switchedOff(providerId, a.name, model)
      );
      if (off.length)
        scope = intersect(
          scope,
          accounts.filter((a) => !off.includes(a)).map((a) => a.id)
        );
    }
  }
  if (id && scope && !scope.includes(id))
    return reject(400, "This account is outside the active sub-pool");
  const strict = body.allow_account_failover === false;
  if (strict && mapped && address.length >= 3) id = (mapped as any).models?.[0]?.connectionId;
  delete body.allow_account_failover;
  if (strict && !id) return reject(400, "Strict pin requires an account");
  const next = new Request(request.url, {
    method: request.method,
    headers,
    body: JSON.stringify(body),
    signal: request.signal,
  });
  if (id) preferred.set(next, id);
  if (strict && id) pins.set(next, id);
  if (scope) scopes.set(next, scope);
  return next;
}
export function constrainKey<T>(request: Request, key: T): T {
  const pin = pins.get(request);
  const scope = scopes.get(request);
  if (!pin && !scope) return key;
  const current = (key as any)?.allowedConnections;
  let next: string[] | null = Array.isArray(current) && current.length > 0 ? current : null;
  if (scope) next = intersect(next, scope);
  if (pin) next = !next || next.includes(pin) ? [pin] : [];
  return { ...key, allowedConnections: compileAccountScope(next) };
}
