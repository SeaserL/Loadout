// SPDX-License-Identifier: Apache-2.0
// Port of crates/providers/src/codex_subscription.rs; no discovery of CLI tokens.
export const PROFILE = "loadout-codex-v1";
export const CLIENT_ID = "app_EMoamEEZ73f0CkXaXp7hrann";
export const managed = (credentials: any) => credentials?.providerSpecificData?.credentialProfile === PROFILE;
export function facts(token: string) {
  try {
    const claims = JSON.parse(Buffer.from(token.split(".")[1], "base64url").toString());
    return { account: claims["https://api.openai.com/auth"]?.chatgpt_account_id,
      expires: Number(claims.exp) * 1000 };
  } catch { return { account: undefined, expires: NaN }; }
}
export function normalizeSubscription(body: any) {
  const next = structuredClone(body);
  for (const key of ["previous_response_id", "max_output_tokens", "max_tokens", "temperature",
    "top_p", "user", "truncation", "prompt_cache_retention", "safety_identifier", "stream_options", "service_tier"])
    delete next[key];
  next.stream = true;
  next.store = false;
  if (typeof next.instructions !== "string") next.instructions = "";
  if (typeof next.input === "string") next.input = [{ type: "message", role: "user", content: [{ type: "input_text", text: next.input }] }];
  if (Array.isArray(next.input)) for (const item of next.input)
    if (item?.role === "system") item.role = "developer";
  return next;
}
export function subscriptionHeaders(credentials: any) {
  const data = credentials.providerSpecificData;
  const account = facts(credentials.apiKey).account;
  if (!account || /[\r\n]/.test(account) || data.transportOptIn !== true || data.loginState !== "ready" ||
      !Number.isFinite(Date.parse(credentials.expiresAt)) || Date.parse(credentials.expiresAt) <= Date.now())
    throw Error("SUBSCRIPTION_LOGIN_REQUIRED");
  return { "Content-Type": "application/json", Authorization: `Bearer ${credentials.apiKey}`,
    "Chatgpt-Account-Id": account, "User-Agent": "loadout/0.4.0", Accept: "text/event-stream",
    ...(data.clientIdentity === "codex" ? { Originator: "codex_cli_rs" } : {}) };
}
export function authBase(value: unknown = "https://auth.openai.com") {
  const url = new URL(String(value));
  if (url.username || url.password || url.search || url.hash || url.pathname !== "/" ||
      !(url.origin === "https://auth.openai.com" || (url.protocol === "http:" && url.hostname === "127.0.0.1")))
    throw Error("AUTH_ENDPOINT_REJECTED");
  return url.origin;
}
export async function authPost(base: string, suffix: string, body: any, form = false) {
  const response = await fetch(authBase(base) + suffix, {
    method: "POST", redirect: "error", signal: AbortSignal.timeout(30000),
    headers: { "content-type": form ? "application/x-www-form-urlencoded" : "application/json",
      "user-agent": "loadout/0.4.0", accept: "application/json" },
    body: form ? new URLSearchParams(body) : JSON.stringify(body),
  });
  const text = await response.text();
  if (text.length > 65536) throw Error("AUTH_RESPONSE_INVALID");
  let data: any = {};
  try { data = JSON.parse(text); } catch { if (response.ok) throw Error("AUTH_RESPONSE_INVALID"); }
  return { status: response.status, ok: response.ok, data };
}
export function tokens(data: any, previousRefresh?: string) {
  const token = data.access_token;
  if (typeof token !== "string" || !token || !facts(token).account) throw Error("AUTH_RESPONSE_INVALID");
  const refresh = data.refresh_token || previousRefresh;
  if (typeof refresh !== "string" || !refresh) throw Error("AUTH_RESPONSE_INVALID");
  const expires = Number(data.expires_in) > 0 ? Date.now() + Number(data.expires_in) * 1000 : facts(token).expires;
  if (!Number.isFinite(expires) || expires <= Date.now()) throw Error("AUTH_RESPONSE_INVALID");
  // id_token is intentionally discarded, never persisted or returned.
  return { apiKey: token, refreshToken: refresh, expiresAt: new Date(expires).toISOString() };
}
