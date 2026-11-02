// SPDX-License-Identifier: Apache-2.0
import { requireManagementAuth } from "@/lib/api/requireManagementAuth";
import { startLogin, loginStatus, transport, refresh, listSubscriptions } from "@/lib/loadoutPhase1/subscription";
export async function POST(request: Request) {
  if (request.headers.has("authorization")) return new Response(null, { status: 403 });
  const denied = await requireManagementAuth(request);
  if (denied) return denied;
  try {
    const text = await request.text();
    if (text.length > 8192) return new Response(null, { status: 413 });
    const body = JSON.parse(text);
    let result;
    switch (body.operation) {
      case "start": result = await startLogin(body); break;
      case "status": result = loginStatus(body.id); break;
      case "cancel": result = loginStatus(body.id, true); break;
      case "list": result = await listSubscriptions(); break;
      case "transport":
        if (typeof body.enabled !== "boolean") throw Error("INVALID_OPERATION");
        result = await transport(body.id, body.enabled, body.confirmation); break;
      case "refresh": result = await refresh(body.id); break;
      default: throw Error("INVALID_OPERATION");
    }
    return Response.json(result);
  } catch { return Response.json({ error: "SUBSCRIPTION_OPERATION_REJECTED" }, { status: 400 }); }
}
