// SPDX-License-Identifier: Apache-2.0
// Port of core/oauth.rs: own device login, opt-in, serial background rotation.
import { randomUUID } from "node:crypto";
import { getProviderConnectionById, getProviderConnections, createProviderConnection as nativeCreate,
  updateProviderConnection as nativeUpdate, getProviderNodeById } from "../db/providers";
import { PROFILE, CLIENT_ID, managed, authBase, authPost, tokens } from "./subscriptionProtocol";
import { subscriptionWrite } from "./subscriptionWrite";
const createProviderConnection = (data: any) => subscriptionWrite(() => nativeCreate(data));
const updateProviderConnection = (id: string, data: any) => subscriptionWrite(() => nativeUpdate(id, data));
type Session = { provider: string; alias: string; base: string; expires: number; interval: number;
  device: string; code: string; state: string; failure?: string; connectionId?: string; timer?: ReturnType<typeof setTimeout> };
const global = globalThis as typeof globalThis & { __loadoutSubscription?: {
  sessions: Map<string, Session>; locks: Map<string, Promise<any>>; timer?: ReturnType<typeof setInterval>;
} };
const state = global.__loadoutSubscription ??= { sessions: new Map(), locks: new Map() };
function serial<T>(id: string, work: () => Promise<T>): Promise<T> {
  const task = (state.locks.get(id) ?? Promise.resolve()).catch(() => {}).then(work);
  state.locks.set(id, task);
  task.finally(() => { if (state.locks.get(id) === task) state.locks.delete(id); }).catch(() => {});
  return task;
}
function safe(row: any) {
  return { id: row.id, provider: row.provider, name: row.name, expiresAt: row.expiresAt,
    transportOptIn: row.providerSpecificData.transportOptIn === true,
    loginState: row.providerSpecificData.loginState, active: row.isActive };
}
async function poll(session: Session) {
  if (session.state !== "pending") return;
  if (Date.now() >= session.expires) { session.state = "expired"; return; }
  try {
    const result = await authPost(session.base, "/api/accounts/deviceauth/token", {
      device_auth_id: session.device, user_code: session.code,
    });
    if (session.state !== "pending") return;
    if (result.status === 403 || result.status === 404) {
      session.timer = setTimeout(() => void poll(session), session.interval);
      session.timer.unref();
      return;
    }
    if (!result.ok || typeof result.data.authorization_code !== "string" || typeof result.data.code_verifier !== "string")
      throw Error("AUTH_RESPONSE_INVALID");
    const exchange = await authPost(session.base, "/oauth/token", { grant_type: "authorization_code", client_id: CLIENT_ID,
      code: result.data.authorization_code, code_verifier: result.data.code_verifier,
      redirect_uri: session.base + "/deviceauth/callback" }, true);
    if (!exchange.ok) throw Error("AUTH_EXCHANGE_FAILED");
    if (session.state !== "pending") return;
    if (Date.now() >= session.expires) { session.state = "expired"; return; }
    await serial(session.provider + "/" + session.alias, async () => {
      const rows = await getProviderConnections({ provider: session.provider });
      const existing: any = rows.find((r: any) => r.name === session.alias);
      if (existing && !managed(existing)) throw Error("ACCOUNT_KIND_CONFLICT");
      const node: any = await getProviderNodeById(session.provider);
      const data = { ...tokens(exchange.data), authType: "apikey", maxConcurrent: 2,
        providerSpecificData: { ...(existing?.providerSpecificData ?? {}), credentialProfile: PROFILE,
          baseUrl: node.baseUrl, apiType: "responses", authBase: session.base, clientIdentity: "loadout",
          transportOptIn: existing?.providerSpecificData?.transportOptIn === true, loginState: "ready" },
        isActive: existing?.providerSpecificData?.transportOptIn === true, testStatus: "active" };
      const row = existing ? await updateProviderConnection(existing.id, data) :
        await createProviderConnection({ ...data, provider: session.provider, name: session.alias });
      session.connectionId = row.id;
      session.state = "complete";
    });
  } catch (error: any) { session.state = "failed"; session.failure = /^[A-Z][A-Z_0-9]{0,80}$/.test(error?.message) ? error.message : "LOGIN_FAILED"; }
  finally { if (session.state !== "pending") { session.device = ""; session.code = ""; } }
}
export async function startLogin(input: any) {
  if (typeof input.alias !== "string" || !/^[a-zA-Z0-9_-]{1,64}$/.test(input.alias)) throw Error("ALIAS_INVALID");
  const node: any = await getProviderNodeById(input.provider);
  if (!node || node.type !== "openai-compatible" || node.apiType !== "responses") throw Error("RESPONSES_NODE_REQUIRED");
  const destination = new URL(node.baseUrl);
  if (!(destination.origin === "https://chatgpt.com" && destination.pathname === "/backend-api/codex") &&
      !(destination.protocol === "http:" && destination.hostname === "127.0.0.1")) throw Error("SUBSCRIPTION_ENDPOINT_REJECTED");
  for (const [id, s] of state.sessions) if (s.expires < Date.now()) { clearTimeout(s.timer); state.sessions.delete(id); }
  if (state.sessions.size >= 32 || [...state.sessions.values()].some((s) => s.provider === input.provider && s.alias === input.alias && s.state === "pending"))
    throw Error("LOGIN_ALREADY_PENDING");
  const base = authBase(input.authBase);
  const response = await authPost(base, "/api/accounts/deviceauth/usercode", { client_id: CLIENT_ID });
  if (!response.ok || typeof response.data.device_auth_id !== "string" || typeof response.data.user_code !== "string") throw Error("LOGIN_START_FAILED");
  const interval = Math.max(1, Math.min(60, Number(response.data.interval) || 5)) * 1000;
  const id = randomUUID();
  const session: Session = { provider: input.provider, alias: input.alias, base, interval,
    expires: Date.now() + 900000, device: response.data.device_auth_id, code: response.data.user_code, state: "pending" };
  state.sessions.set(id, session);
  session.timer = setTimeout(() => void poll(session), interval);
  session.timer.unref();
  return { id, verificationUrl: base + "/codex/device", userCode: session.code, expiresAt: session.expires };
}
export function loginStatus(id: string, cancel = false) {
  const session = state.sessions.get(id);
  if (!session) throw Error("LOGIN_NOT_FOUND");
  if (cancel) { clearTimeout(session.timer); session.state = "cancelled"; session.device = ""; session.code = ""; }
  return { state: session.state, connectionId: session.connectionId, failure: session.failure };
}
export async function transport(id: string, enabled: boolean, confirmation: any) {
  return serial(id, async () => {
    const row: any = await getProviderConnectionById(id);
    if (!managed(row)) throw Error("SUBSCRIPTION_NOT_FOUND");
    if (enabled && (confirmation?.risk !== "I understand" || confirmation?.alias !== row.name)) throw Error("TRANSPORT_CONFIRMATION_REQUIRED");
    if (enabled && (row.providerSpecificData.loginState !== "ready" || Date.parse(row.expiresAt) <= Date.now())) throw Error("LOGIN_REQUIRED");
    return safe(await updateProviderConnection(id, { isActive: enabled,
      providerSpecificData: { ...row.providerSpecificData, transportOptIn: enabled } }));
  });
}
export async function refresh(id: string) {
  return serial(id, async () => {
    const row: any = await getProviderConnectionById(id);
    if (!managed(row)) throw Error("SUBSCRIPTION_NOT_FOUND");
    if (row.providerSpecificData.loginState !== "ready") return safe(row);
    if (Date.parse(row.expiresAt) > Date.now() + 86400000) return safe(row);
    const old = row.refreshToken;
    const result = await authPost(row.providerSpecificData.authBase, "/oauth/token", {
      grant_type: "refresh_token", client_id: CLIENT_ID, refresh_token: old, scope: "openid profile email" }, true);
    const current: any = await getProviderConnectionById(id);
    if (!current || current.refreshToken !== old) throw Error("LOGIN_CHANGED");
    if (!result.ok) {
      const type = typeof result.data.error === "string" ? result.data.error : result.data.error?.code;
      if ([400, 401].includes(result.status) && ["invalid_grant", "refresh_token_reused", "refresh_token_expired", "refresh_token_invalidated"].includes(type))
        return safe(await updateProviderConnection(id, { isActive: false,
          providerSpecificData: { ...current.providerSpecificData, loginState: "needs_login" } }));
      throw Error("REFRESH_UNAVAILABLE");
    }
    return safe(await updateProviderConnection(id, tokens(result.data, old)));
  });
}
export async function listSubscriptions() {
  return (await getProviderConnections({})).filter(managed).map(safe);
}
export function startRefreshWorker() {
  if (state.timer) return;
  const tick = async () => { for (const row of await listSubscriptions()) { try { await refresh(row.id); } catch {} } };
  state.timer = setInterval(() => void tick().catch(() => {}), 300000);
  state.timer.unref();
  void tick().catch(() => {});
}
