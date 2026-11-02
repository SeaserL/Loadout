// SPDX-License-Identifier: Apache-2.0
import { requireManagementAuth } from "@/lib/api/requireManagementAuth";
import { enabled } from "@/lib/loadoutPhase1/secretStore";
import { exportVault, restoreVault } from "@/lib/loadoutPhase1/vaultLifecycle";
export async function POST(request: Request) {
  if (!enabled()) return new Response(null, { status: 404 });
  // Keep this explicit secret-transfer surface on authenticated dashboard sessions.
  if (request.headers.has("authorization")) return new Response(null, { status: 403 });
  const rejection = await requireManagementAuth(request);
  if (rejection) return rejection;
  try {
    const raw = await request.text();
    if (raw.length > 1500000) return new Response(null, { status: 413 });
    const body = JSON.parse(raw);
    if (body.operation === "export") return Response.json(await exportVault(body.password));
    if (body.operation === "restore")
      return Response.json(await restoreVault(body.password, body.backup));
    return Response.json({ error: "INVALID_VAULT_OPERATION" }, { status: 400 });
  } catch {
    return Response.json({ error: "VAULT_OPERATION_REJECTED" }, { status: 400 });
  }
}
