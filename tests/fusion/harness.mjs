// SPDX-License-Identifier: Apache-2.0
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
export const pkg = path.resolve(
  process.env.FUSION_PACKAGE ?? "MISSING_PACKAGE",
);
const load = (name) =>
  import(pathToFileURL(path.join(pkg, "tools", name)).href);
export const { Instance, keytar, sha, sleep, manifest, verifyPackage } =
  await load("runtime.mjs");
export const { backup, restore } = await load("backup.mjs");
export const { compile, applyReady, nativeWait, fingerprint } =
  await load("mapping.mjs");
export const { snapshot, migrate } = await load("migrate.mjs");
export const runs = path.resolve(
  process.env.FUSION_RELEASE_ROOT, "runs/loadout-fusion-isolated",
);
fs.mkdirSync(runs, { recursive: true });
export const baseline = () => ({
  releaseId: manifest().releaseId,
  treeHash: manifest().treeHash,
});
export const { configure, conservativeTraffic } = await load("policy.mjs");
export { load };
