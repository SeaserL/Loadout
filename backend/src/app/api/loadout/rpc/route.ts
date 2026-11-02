// SPDX-License-Identifier: Apache-2.0
import { requireManagementAuth } from "@/lib/api/requireManagementAuth";
import { sanitizeErrorMessage } from "@omniroute/open-sse/utils/error.ts";
import { dispatch } from "@/lib/loadoutPhase1/control";
import { RpcError } from "@/lib/loadoutPhase1/control/shared";

const MAX_BODY = 4 * 1024 * 1024;

export async function POST(request: Request) {
  // Inference keys never reach the control plane; only the desktop session does.
  if (request.headers.has("authorization") || request.headers.has("x-api-key"))
    return new Response(null, { status: 403 });
  const denied = await requireManagementAuth(request);
  if (denied) return denied;
  const text = await request.text();
  if (text.length > MAX_BODY) return new Response(null, { status: 413 });
  let body: { method?: unknown; params?: unknown };
  try {
    body = JSON.parse(text);
  } catch {
    return Response.json({ error: { code: "invalid_params", message: "请求无效" } }, { status: 400 });
  }
  if (typeof body.method !== "string")
    return Response.json({ error: { code: "invalid_params", message: "请求无效" } }, { status: 400 });
  try {
    return Response.json({ result: (await dispatch(request, body.method, body.params)) ?? null });
  } catch (error) {
    const known = error instanceof RpcError;
    return Response.json({
      error: {
        code: known ? (error as RpcError).code : "internal",
        message: known ? (error as Error).message : sanitizeErrorMessage(error),
      },
    });
  }
}
