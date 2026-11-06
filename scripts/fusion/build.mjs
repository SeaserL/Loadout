// SPDX-License-Identifier: Apache-2.0
import fs from "node:fs";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { pathToFileURL } from "node:url";
import { root, source as work, tools, dist } from "./config.mjs";
const version = "52";
const head = spawnSync("git", ["-C", work, "rev-parse", "HEAD"], {encoding:"utf8",windowsHide:true});
if(head.status !== 0 || head.stdout.trim() !== "ad6f33f66bda34eba98645d3bfdabbde294bc843") throw Error("UPSTREAM_PIN_MISMATCH");
const run = path.join(root, "runs", "build-v" + version + "-" + Date.now());
fs.mkdirSync(run, { recursive: true });
const env = {};
for (const k of [
  "SystemRoot",
  "WINDIR",
  "COMSPEC",
  "PATHEXT",
  "PATH",
  "PROCESSOR_ARCHITECTURE",
])
  if (process.env[k]) env[k] = process.env[k];
for (const k of [
  "HOME",
  "USERPROFILE",
  "APPDATA",
  "LOCALAPPDATA",
  "TEMP",
  "TMP",
  "DATA_DIR",
]) {
  env[k] = path.join(run, k);
  fs.mkdirSync(env[k], { recursive: true });
}
const cache = process.env.FUSION_SWC_CACHE;
if (!cache || !/^C:\\loadout-fusion-swc-[a-f0-9-]+$/i.test(cache)) throw Error("SWC_CACHE_SCOPE");
Object.assign(env, {
  NEXT_DIST_DIR: dist,
  NEXT_TELEMETRY_DISABLED: "1",
  OMNIROUTE_BUILD_BACKEND_ONLY: "1",
  OMNIROUTE_USE_TURBOPACK: "0",
  OMNIROUTE_DISABLE_BACKGROUND_SERVICES: "true",
  OMNIROUTE_DISABLE_CONVERSATION_TRACKING: "1",
  OMNIROUTE_BUILD_MEMORY_MB: "8192",
  NODE_PATH: tools,
  SWC_NATIVE_BINDING_CACHE: cache,
  ESBUILD_BINARY_PATH: path.join(tools, "@esbuild/win32-x64/esbuild.exe"),

});
env.NODE_OPTIONS =
  "--import=" +
  new URL("./build-guard.mjs", import.meta.url).href;
env.NEXT_TEST_NATIVE_DIR = path.join(
  version === "52" ? tools : path.join(work, "node_modules"),
  "@next/swc-win32-x64-msvc",
);
const log = fs.openSync(path.join(run, "build.log"), "w");
const child = spawn(
  process.execPath,
  ["scripts/build/build-next-isolated.mjs"],
  { cwd: work, env, stdio: ["ignore", log, log], windowsHide: true },
);
fs.writeFileSync(
  path.join(root, "runs", "build-latest-v" + version + ".json"),
  JSON.stringify({ run, work, pid: child.pid, cache }, null, 2),
);
console.log(JSON.stringify({ run, pid: child.pid }));
let code = await new Promise((resolve) => child.on("exit", resolve));
// The upstream `postbuild` step: bundles the compression, call-log, health and LLMLingua workers
// and co-locates the optional AI runtime. Calling the build script directly skips it, which left
// 1.0 with placeholder workers (ExecPlan 071).
if (code === 0) {
  const post = spawnSync(process.execPath, ["scripts/build/colocate-standalone.mjs"], {
    cwd: work,
    env: { ...env, OMNIROUTE_STANDALONE_DIR: path.join(work, dist, "standalone") },
    stdio: ["ignore", log, log],
    windowsHide: true,
  });
  code = post.status ?? 1;
}
fs.closeSync(log);
fs.writeFileSync(
  path.join(run, "result.json"),
  JSON.stringify(
    { code, version, work, cache, finished: new Date().toISOString() },
    null,
    2,
  ),
);
console.log(JSON.stringify({ code, run }));
process.exitCode = code || 0;
