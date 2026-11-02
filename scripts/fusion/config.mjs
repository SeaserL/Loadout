// SPDX-License-Identifier: Apache-2.0
import path from "node:path";
import { fileURLToPath } from "node:url";
export const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
if (!process.env.FUSION_RELEASE_ROOT) throw Error("USE_FUSION_RELEASE_ENTRY");
export const root = path.resolve(process.env.FUSION_RELEASE_ROOT);
if (!root.startsWith(path.join(repo, "development/fusion/runs") + path.sep)) throw Error("RUN_SCOPE");
export const source = path.join(repo, "development/upstreams/omniroute/.claude/worktrees/loadout-phase11-v52");
export const tools = path.join(repo, "development/fusion/tools/node_modules");
export const dist = ".build/loadout-fusion";
export const profile = "067.1";
export const upstream = "ad6f33f66bda34eba98645d3bfdabbde294bc843";
