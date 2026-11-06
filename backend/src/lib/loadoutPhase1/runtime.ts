// SPDX-License-Identifier: Apache-2.0
import { enabled, initialize } from "./secretStore";
import { write } from "./vaultLifecycle";
export async function initializeRuntime() {
  if (!enabled()) return;
  await initialize();
  // Reconcile interrupted writes before serving requests, using native persisted refs.
  await write(async () => {});
  const { applyReleaseDefaults } = await import("./releaseDefaults");
  await applyReleaseDefaults();
  const { kvGet, kvSet } = await import("./control/shared");
  const subpools = kvGet<any>("subpools", null);
  if (subpools?.active && subpools.remember === false) kvSet("subpools", { ...subpools, active: null });
  const { startRefreshWorker } = await import("./subscription");
  startRefreshWorker();
  const { initRadarSyncScheduler } = await import("../radar/scheduler");
  initRadarSyncScheduler();
  const { initPricingSync } = await import("../pricingSync");
  await initPricingSync();
}
