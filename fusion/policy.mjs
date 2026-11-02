// SPDX-License-Identifier: Apache-2.0
import { Instance, manifest } from "./runtime.mjs";
import { nativeWait } from "./mapping.mjs";
export const conservativeTraffic = () => ({
  minTime: 20000,
  maxConcurrent: 1,
  rpm: 3,
  maxWaitMs: 30000,
  // v51 rejects this per-account field; the same 90s bound remains in
  // nativeWait().requestQueue and each compiled combo's targetTimeoutMs.
  ...(manifest().upstreamVersion === "3.8.51"
    ? {}
    : { executionMaxWaitMs: 90000 }),
});
export async function configure(instance, connectionIds = []) {
  // Configuration only; native OmniRoute owns all pacing and retry decisions.
  if (!(instance instanceof Instance))
    throw Error("ISOLATED_INSTANCE_REQUIRED");
  const providers = (await instance.admin("/api/providers")).connections;
  if (!Array.isArray(providers)) throw Error("PROVIDER_INVENTORY_SHAPE");
  if (
    new Set(connectionIds).size !== connectionIds.length ||
    connectionIds.some((id) => !providers.some((p) => p.id === id))
  )
    throw Error("UNKNOWN_CONNECTION");
  await instance.admin("/api/resilience", "PATCH", nativeWait());
  for (const id of connectionIds) {
    await instance.admin("/api/providers/" + id, "PUT", {
      rateLimitOverrides: conservativeTraffic(),
    });
    await instance.admin("/api/rate-limits", "POST", {
      connectionId: id,
      enabled: true,
    });
  }
  const actual = await instance.admin("/api/resilience");
  if (
    !Object.entries(nativeWait()).every(([s, v]) =>
      Object.entries(v).every(([k, x]) => actual[s]?.[k] === x),
    )
  )
    throw Error("PROFILE_READBACK_FAILED");
  const after = (await instance.admin("/api/providers")).connections;
  for (const id of connectionIds) {
    const p = after.find((p) => p.id === id);
    if (
      p?.rateLimitProtection !== true ||
      !Object.entries(conservativeTraffic()).every(
        ([k, v]) => p.rateLimitOverrides?.[k] === v,
      )
    )
      throw Error("TRAFFIC_READBACK_FAILED");
  }
  return {
    waitPolicy: "native-bounded-v1",
    pacedConnections: connectionIds.length,
    trafficAcceptance: "CONFIGURED_NOT_A_LIVE_PROVIDER_TEST",
  };
}
