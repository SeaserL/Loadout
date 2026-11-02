// SPDX-License-Identifier: Apache-2.0
// Experiment-only adapter. No production installation or legacy import is permitted.
import path from "node:path";
import { managed, authBase } from "./subscriptionProtocol";
import { isSubscriptionWrite } from "./subscriptionWrite";
const nativeRequire = process
  .getBuiltinModule("node:module")
  .createRequire(path.join(process.cwd(), "package.json"));
import { randomUUID } from "node:crypto";
import {
  mapProviderSpecificDataSecrets,
  PROVIDER_SPECIFIC_SECRET_FIELDS,
} from "../db/providers/providerSpecificDataSecrets";
export const enabled = () => true; // Mandatory in this isolated distribution.
const prefix = "loadout-ref:v1:";
type VaultState = {
  values: Map<string, string>;
  refs: Map<string, string>;
  queue: Promise<unknown>;
  initialized: boolean;
  unavailable: boolean;
};
const globals = globalThis as typeof globalThis & { __loadoutVault?: VaultState };
const state = (globals.__loadoutVault ??= {
  values: new Map(),
  refs: new Map(),
  queue: Promise.resolve(),
  initialized: false,
  unavailable: false,
});
const { values, refs } = state;
function store() {
  const service = process.env.LOADOUT_TEST_SERVICE || "";
  if (!/^loadout-test:omniroute-phase3-[a-f0-9-]+$/.test(service))
    throw new Error("TEST_STORE_SCOPE_REQUIRED");
  if (state.unavailable) throw new Error("SECRET_STORE_UNAVAILABLE");
  return { service, api: nativeRequire("keytar") };
}
async function serial<T>(action: () => Promise<T>): Promise<T> {
  const pending = state.queue.then(action);
  state.queue = pending.catch(() => {});
  try {
    return await pending;
  } catch {
    throw new Error("SECRET_STORE_UNAVAILABLE");
  }
}
export async function initialize() {
  if (!enabled()) return;
  await serial(async () => {
    const { service, api } = store();
    for (const item of await api.findCredentials(service)) {
      if (!item.account.startsWith(prefix)) throw new Error("INVALID_SECRET_REF");
      values.set(item.account, item.password);
      refs.set(item.password, item.account);
    }
    state.initialized = true;
  });
}
export function isReference(value: unknown): boolean {
  return typeof value === "string" && value.startsWith(prefix);
}
export function read(value: string | null | undefined) {
  if (!value) return value;
  if (!state.initialized || state.unavailable) throw new Error("SECRET_STORE_UNAVAILABLE");
  if (!isReference(value) || !values.has(value)) throw new Error("SECRET_REFERENCE_MISSING");
  return values.get(value)!;
}
export function reference(value: string | null | undefined) {
  if (!value) return value;
  if (!state.initialized || state.unavailable) throw new Error("SECRET_STORE_UNAVAILABLE");
  if (isReference(value)) {
    if (!values.has(value)) throw new Error("SECRET_REFERENCE_MISSING");
    return value;
  }
  const ref = refs.get(value);
  if (!ref) throw new Error("SECRET_NOT_PREPARED");
  return ref;
}
export async function prepareConnection<T>(connection: T): Promise<T> {
  if (!enabled() || !connection) return connection;
  if (!state.initialized) throw new Error("SECRET_STORE_NOT_INITIALIZED");
  const pending = new Set<string>();
  function collect(value: string) {
    if (value && Buffer.byteLength(value) > 2400) throw new Error("SECRET_TOO_LARGE");
    if (value && !isReference(value) && !refs.has(value)) pending.add(value);
    return value;
  }
  const row = connection as Record<string, any>;
  const { getDbInstance } = await import("../db/core");
  const previous = row.id ? getDbInstance().prepare("SELECT * FROM provider_connections WHERE id=?").get(row.id) : null;
  const oldMeta = previous?.provider_specific_data ? JSON.parse(previous.provider_specific_data) : {};
  if (managed(row) || oldMeta.credentialProfile === "loadout-codex-v1") {
    if (!isSubscriptionWrite()) {
      const fixed = ["credentialProfile", "transportOptIn", "loginState", "authBase", "clientIdentity", "baseUrl", "apiType"];
      if (!previous || fixed.some((k) => oldMeta[k] !== row.providerSpecificData?.[k]) ||
          (row.isActive === true && !previous.is_active) || (row.apiKey !== previous.api_key && row.apiKey !== read(previous.api_key)) ||
          (row.refreshToken !== previous.refresh_token && row.refreshToken !== read(previous.refresh_token)) || row.expiresAt !== previous.expires_at || row.provider !== previous.provider)
        throw Error("USE_SUBSCRIPTION_LIFECYCLE");
    }
    authBase(row.providerSpecificData.authBase);
    if (row.maxConcurrent !== 2 || (row.isActive && (row.providerSpecificData.transportOptIn !== true || row.providerSpecificData.loginState !== "ready")))
      throw Error("SUBSCRIPTION_POLICY_REQUIRED");
  }
  if (row.authType === "apikey" && !row.apiKey) throw new Error("API_KEY_REQUIRED");
  if (row.authType && row.authType !== "apikey") throw new Error("UNSUPPORTED_AUTH_TYPE");
  const known = new Set<string>([
    "credentialProfile", "transportOptIn", "loginState", "authBase", "clientIdentity",
    "baseUrl",
    "prefix",
    "nodeName",
    "apiType",
    "operatorDisabledAt",
    "autoDisabledAt",
    "banDisabledAt",
    "apiKeyHealth",
    "selectedKeyId",
    ...PROVIDER_SPECIFIC_SECRET_FIELDS,
  ]);
  if (Object.keys(row.providerSpecificData || {}).some((k) => !known.has(k)))
    throw new Error("UNSUPPORTED_CREDENTIAL_METADATA");
  const metadata = row.providerSpecificData || {};
  if (metadata.selectedKeyId != null && !/^(primary|extra_\d+)$/.test(metadata.selectedKeyId))
    throw new Error("INVALID_HEALTH_METADATA");
  if (metadata.apiKeyHealth != null) {
    for (const [id, health] of Object.entries(metadata.apiKeyHealth) as [
      string,
      Record<string, unknown>,
    ][]) {
      if (!/^(primary|extra_\d+)$/.test(id) || !health || typeof health !== "object")
        throw new Error("INVALID_HEALTH_METADATA");
      for (const [name, value] of Object.entries(health)) {
        const valid =
          name === "status"
            ? ["active", "warning", "invalid"].includes(String(value))
            : ["failures", "totalRequests", "totalFailures"].includes(name)
              ? typeof value === "number" && Number.isFinite(value) && value >= 0
              : ["lastFailure", "lastSuccess", "cooldownUntil"].includes(name)
                ? value == null ||
                  (typeof value === "string" && /^\d{4}-\d{2}-\d{2}T[0-9:.]+Z$/.test(value))
                : false;
        if (!valid) throw new Error("INVALID_HEALTH_METADATA");
      }
    }
  }
  for (const field of PROVIDER_SPECIFIC_SECRET_FIELDS) {
    const value = row.providerSpecificData?.[field];
    if (
      value != null &&
      typeof value !== "string" &&
      !(Array.isArray(value) && value.every((v) => typeof v === "string"))
    )
      throw new Error("UNSUPPORTED_SECRET_SHAPE");
  }
  for (const field of ["apiKey", "accessToken", "refreshToken", "idToken"])
    if (row[field]) collect(row[field]);
  mapProviderSpecificDataSecrets(row.providerSpecificData, collect);
  await serial(async () => {
    const { service, api } = store();
    for (const value of pending) {
      const ref = prefix + randomUUID();
      await api.setPassword(service, ref, value);
      values.set(ref, value);
      refs.set(value, ref);
    }
  });
  return connection;
}
export function setUnavailableForTest(value: boolean) {
  state.unavailable = value;
}
export function clearCacheForTest() {
  values.clear();
  refs.clear();
  state.initialized = false;
}

export function needsPrune(keep: Set<string>) {
  return [...values.keys()].some((ref) => !keep.has(ref));
}
export async function prune(keep: Set<string>) {
  await serial(async () => {
    const { service, api } = store();
    const actual = await api.findCredentials(service);
    for (const item of actual)
      if (!keep.has(item.account)) await api.deletePassword(service, item.account);
    values.clear();
    refs.clear();
    for (const item of actual)
      if (keep.has(item.account)) {
        values.set(item.account, item.password);
        refs.set(item.password, item.account);
      }
  });
}
export async function entries(keep: Set<string>) {
  return serial(async () => {
    const { service, api } = store();
    const found = (await api.findCredentials(service)).filter((item) => keep.has(item.account));
    if (found.length !== keep.size) throw new Error("BACKUP_SECRET_MISSING");
    return found;
  });
}
export async function restoreEntries(incoming: { account: string; password: string }[]) {
  await serial(async () => {
    const { service, api } = store();
    const actual = new Map(
      (await api.findCredentials(service)).map((item) => [item.account, item.password])
    );
    if (
      incoming.some(
        (item) => actual.has(item.account) && actual.get(item.account) !== item.password
      )
    )
      throw new Error("BACKUP_REF_CONFLICT");
    const created: string[] = [];
    try {
      for (const item of incoming)
        if (!actual.has(item.account)) {
          await api.setPassword(service, item.account, item.password);
          created.push(item.account);
        }
    } catch {
      for (const ref of created) await api.deletePassword(service, ref);
      throw new Error("BACKUP_RESTORE_FAILED");
    }
    for (const item of incoming) {
      values.set(item.account, item.password);
      refs.set(item.password, item.account);
    }
  });
}
