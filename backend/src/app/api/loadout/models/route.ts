// SPDX-License-Identifier: Apache-2.0
// `GET /v1/models` for clients: the names a client can actually send, within its key's scope.
import { getApiKeyMetadata } from "@/lib/db/apiKeys";
import { listModels } from "@/lib/loadoutPhase1/control/models";
import { endpoints, mappedComboNames, virtuals } from "@/lib/loadoutPhase1/control/routing";
import { snapshot } from "@/lib/loadoutPhase1/control/shared";

export async function GET(request: Request) {
  const bearer = (request.headers.get("authorization") ?? "").replace(/^Bearer\s+/i, "");
  const key = bearer || request.headers.get("x-api-key") || "";
  const meta = (await getApiKeyMetadata(key)) as Record<string, any> | null;
  if (!meta)
    return Response.json(
      { error: { message: "Invalid API key", type: "invalid_api_key" } },
      { status: 401 }
    );
  const providers = await snapshot();
  const rows = (await listModels(providers)).filter((m) => m.enabled);
  const endpointName = request.headers.get("x-loadout-endpoint");
  const created = Math.floor(Date.now() / 1000);
  const entry = (id: string, owned_by: string) => ({ id, object: "model", created, owned_by });
  if (endpointName) {
    const endpoint = endpoints().find((e) => e.name === endpointName && e.enabled);
    if (!endpoint) return Response.json({ error: { message: "Unknown endpoint" } }, { status: 404 });
    const names = rows
      .filter(
        (m) =>
          m.provider === endpoint.provider &&
          (!endpoint.members || endpoint.members.includes(m.account))
      )
      .map((m) => m.upstream_model);
    return Response.json({
      object: "list",
      data: [...new Set(names)].sort().map((id) => entry(id, endpoint.provider)),
    });
  }
  const pools = (await virtuals(async () => ({}) as never)).filter((v) => v.enabled);
  const scoped = Array.isArray(meta.allowedConnections) && meta.allowedConnections.length > 0;
  const allowedConns = new Set<string>(meta.allowedConnections ?? []);
  const inScope = (provider: string, account: string) => {
    if (!scoped) return true;
    const conn = providers.find((p) => p.slug === provider)?.conns.find((c) => c.name === account);
    return !!conn && allowedConns.has(conn.id);
  };
  const restricted = meta.modelAccessMode === "restricted";
  const allowedModels = new Set<string>(meta.allowedModels ?? []);
  const names = new Map<string, string>();
  for (const pool of pools) names.set(pool.name, "loadout");
  for (const m of rows) if (inScope(m.provider, m.account)) names.set(m.name, m.provider);
  // A native combo with no `loadout/...` name is asked for by its own name.
  const allowedCombos: string[] = meta.allowedCombos ?? [];
  const mapped = mappedComboNames();
  const data = [
    ...[...names.entries()]
      .filter(([id]) => !restricted || allowedModels.has(id))
      .map(([id, owner]) => entry(id, owner)),
    ...allowedCombos.filter((name) => !mapped.has(name)).map((name) => entry(name, "loadout")),
  ];
  return Response.json({ object: "list", data });
}
