// SPDX-License-Identifier: Apache-2.0
// A 1.0 install updating itself to 1.1 with its own updater, against a local stand-in for GitHub.
//   node scripts/fusion/update-from-1.0-e2e.mjs <1.0 full.zip> <1.1 full.zip> <1.1 lite.zip>
// 1.0's updater downloads Loadout-<version>-portable-windows-x64.zip (the 1.1 full archive) and
// refuses any file its manifest does not list, so the full archive carries a complete manifest
// (ExecPlan 071). After that, the converted install takes a 1.1 update the 1.1 way: the lite archive
// alone, with the AI pack copied from the install.
// No window is opened. Loadout.exe is single-instance and would take over the owner's window.
import { createHash, randomUUID } from "node:crypto";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { pathToFileURL } from "node:url";

const here = path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1"));

// Run with the package's own Node, in a process of its own, as update-e2e.mjs does.
if (process.argv[2] === "--rpc") {
  const [, , , pkg, dir, op, arg] = process.argv;
  const { Instance } = await import(pathToFileURL(path.join(pkg, "tools/runtime.mjs")).href);
  if (op === "create") {
    const i = await Instance.create(dir, undefined, JSON.parse(arg));
    await i.start();
    console.log(JSON.stringify({ port: (await i.status()).port }));
  } else if (op === "cleanup") {
    await new Instance(dir).cleanup();
    console.log("{}");
  } else {
    const i = new Instance(dir);
    await i.attach();
    const reply = await i.admin("/api/loadout/rpc", "POST", { method: op, params: arg ? JSON.parse(arg) : null });
    console.log(JSON.stringify(reply));
  }
  process.exit(0);
}

const [oldFull, newFull, newLite] = process.argv.slice(2, 5).map((p) => path.resolve(p ?? ""));
if (![oldFull, newFull, newLite].every((p) => fs.existsSync(p)))
  throw Error("usage: update-from-1.0-e2e.mjs <1.0 full.zip> <1.1 full.zip> <1.1 lite.zip>");
const root = path.join(path.parse(newFull).root, "LoadoutUpdateFrom10-" + randomUUID().slice(0, 8));
const pkg = path.join(root, "Loadout");
const instance = path.join(root, "loadout-fusion-isolated", "e2e");
const work = path.join(root, "work");
fs.mkdirSync(work, { recursive: true });
const sha = (data) => createHash("sha256").update(data).digest("hex");
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const step = (text) => console.log(`[${new Date().toISOString().slice(11, 19)}] ${text}`);
const check = (name, ok, detail = "") => {
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${detail ? "  " + detail : ""}`);
  if (!ok) process.exitCode = 1;
};
const python = (code, ...args) => {
  const done = spawnSync("python", ["-c", code, ...args], { encoding: "utf8", windowsHide: true, maxBuffer: 1 << 26 });
  if (done.status !== 0) throw Error("python: " + done.stderr.slice(-400));
  return done.stdout;
};
const zipManifest = (zip) =>
  JSON.parse(python("import sys,zipfile; sys.stdout.write(zipfile.ZipFile(sys.argv[1]).read('fusion-manifest.json').decode('utf-8'))", zip));

/** The lite archive under another version, as a later 1.1 release would ship it. */
function liteVariant(version) {
  const out = path.join(work, `Loadout-${version}-lite-portable-windows-x64.zip`);
  python(
    `
import json, sys, zipfile
src, out, version = sys.argv[1], sys.argv[2], sys.argv[3]
with zipfile.ZipFile(src) as z, zipfile.ZipFile(out, 'w', zipfile.ZIP_DEFLATED, compresslevel=1) as o:
    for info in z.infolist():
        if info.filename == 'fusion-manifest.json':
            manifest = json.loads(z.read(info.filename))
            manifest['productVersion'] = version
            o.writestr(info.filename, json.dumps(manifest))
        else:
            o.writestr(info, z.read(info.filename))
`,
    newLite, out, version,
  );
  return out;
}

/** What the stand-in for GitHub serves: one version, and its assets by name. */
let offer = null;
const pulled = [];
const feed = http.createServer((req, res) => {
  const origin = `http://127.0.0.1:${feed.address().port}`;
  if (req.url === "/releases/latest") {
    res.writeHead(302, { location: `${origin}/releases/tag/v${offer.version}` });
    return res.end();
  }
  const match = /^\/releases\/download\/v([^/]+)\/([^/]+?)(\.sha256)?$/.exec(req.url);
  const asset = match && match[1] === offer.version ? offer.assets[match[2]] : null;
  if (!asset) {
    res.writeHead(404);
    return res.end();
  }
  if (match[3]) return res.end(`${asset.sha}  ${match[2]}\n`);
  if (req.headers.range === "bytes=0-0") {
    res.writeHead(206, { "content-range": `bytes 0-0/${asset.size}`, "content-length": 1 });
    return res.end("x");
  }
  pulled.push(match[2]);
  res.writeHead(200, { "content-length": asset.size });
  fs.createReadStream(asset.file).pipe(res);
});
/** `files` maps each asset name to the file served under it. */
const offerVersion = (version, files) => {
  const assets = {};
  for (const [name, file] of Object.entries(files)) assets[name] = { file, size: fs.statSync(file).size, sha: sha(fs.readFileSync(file)) };
  offer = { version, assets };
  pulled.length = 0;
};
function child(op, arg) {
  const args = [path.join(here, "update-from-1.0-e2e.mjs"), "--rpc", pkg, instance, op, ...(arg ? [JSON.stringify(arg)] : [])];
  const done = spawnSync(path.join(pkg, "node.exe"), args, { encoding: "utf8", windowsHide: true, timeout: 200000 });
  if (done.status !== 0) throw Error(`${op}: ${done.stderr.slice(-300)}`);
  return JSON.parse(done.stdout.trim().split("\n").at(-1));
}
const rpc = (method, params) => {
  const reply = child(method, params);
  if (reply.error) throw Error(`${method}: ${reply.error.message}`);
  return reply.result;
};
async function waitPhase(wanted, ms = 900000) {
  const end = Date.now() + ms;
  let last;
  while (Date.now() < end) {
    last = rpc("update.status");
    if (wanted.includes(last.phase)) return last;
    await sleep(2000);
  }
  throw Error(`phase ${wanted} not reached; last ${JSON.stringify(last)}`);
}
const resultFile = path.join(root, "loadout-update", "result.json");
async function waitResult(previous, ms = 400000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    try {
      const result = JSON.parse(fs.readFileSync(resultFile, "utf8").replace(/^﻿/, ""));
      if (result.at_ms !== previous) return result;
    } catch {}
    await sleep(1000);
  }
  throw Error("no result from the helper");
}
const packageProcesses = () => {
  const done = spawnSync("powershell", ["-NoProfile", "-Command", `Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith('${pkg}\\', [StringComparison]::OrdinalIgnoreCase) } | ForEach-Object { '  ' + $_.ProcessId + ' ' + $_.CommandLine.Replace('${pkg}', '<package>') }`], { encoding: "utf8", windowsHide: true });
  return done.stdout.trimEnd();
};
const hasPack = () => fs.existsSync(path.join(pkg, "ai-pack.json"));

await new Promise((resolve) => feed.listen(0, "127.0.0.1", resolve));
const feedUrl = `http://127.0.0.1:${feed.address().port}/`;
try {
  const start = zipManifest(oldFull).productVersion;
  const target = zipManifest(newFull).productVersion;
  const later = target.replace(/\d+$/, (x) => String(Number(x) + 1));
  step(`unpacking ${start}`);
  python("import sys,zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])", oldFull, pkg);
  check("the old package has no AI pack", !hasPack());

  step("starting it with a client");
  const { port } = child("create", { network: "loopback-only", port: 0, updateFeed: feedUrl });
  rpc("update.set_auto_check", { enabled: false });
  rpc("client.create", { name: "survives-the-update" });
  check("starts on the old version", rpc("daemon.status").version.startsWith(start + " "));

  // GitHub carries all three; 1.0 knows only the full name.
  offerVersion(target, {
    [`Loadout-${target}-portable-windows-x64.zip`]: newFull,
    [`Loadout-${target}-lite-portable-windows-x64.zip`]: newLite,
  });
  step(`checking for ${target}`);
  rpc("update.check");
  const found = await waitPhase(["available", "error"]);
  check("finds the new version", found.phase === "available" && found.latest_version === target, JSON.stringify({ phase: found.phase, error: found.error }));

  step("downloading and verifying with the 1.0 updater");
  rpc("update.download");
  const ready = await waitPhase(["ready", "error"]);
  check("the 1.0 updater accepts the full archive", ready.phase === "ready", JSON.stringify({ phase: ready.phase, error: ready.error }));
  check("only the full archive was downloaded", pulled.length === 1 && pulled[0] === `Loadout-${target}-portable-windows-x64.zip`, pulled.join());

  step("installing");
  const t0 = Date.now();
  rpc("update.install");
  const installed = await waitResult(undefined);
  check("the 1.0 helper reports success", installed.status === "installed", installed.reason ?? "");
  const moved = rpc("daemon.status");
  check("runs the new version from the same folder", moved.version.startsWith(target + " "), `${moved.version}, ${Math.round((Date.now() - t0) / 1000)} s`);
  const ai = rpc("features.list").ai_pack;
  check("it is the full edition", ai.installed === true && hasPack(), JSON.stringify(ai));
  check("the same address", (await fetch(`http://127.0.0.1:${port}/v1/models`)).status === 401, String(port));
  check("the data came along", rpc("client.list").some((c) => c.name === "survives-the-update"));
  check("the 1.1 defaults are in place", rpc("features.list").features.find((f) => f.key === "response_cache")?.enabled === false);
  for (let n = 0; n < 60 && fs.existsSync(`${pkg}.previous-${start}`); n++) await sleep(1000);
  check("the old folder is cleaned up", !fs.existsSync(`${pkg}.previous-${start}`));

  step(`then ${later}, the 1.1 way`);
  offerVersion(later, { [`Loadout-${later}-lite-portable-windows-x64.zip`]: liteVariant(later) });
  rpc("update.check");
  await waitPhase(["available"]);
  rpc("update.download");
  const readyLater = await waitPhase(["ready", "error"]);
  check("downloads the app alone", readyLater.phase === "ready" && pulled.length === 1, JSON.stringify({ phase: readyLater.phase, error: readyLater.error, pulled }));
  rpc("update.install");
  const laterResult = await waitResult(installed.at_ms);
  check("installs it", laterResult.status === "installed" && rpc("daemon.status").version.startsWith(later + " "), laterResult.reason ?? "");
  check("still the full edition", hasPack() && rpc("features.list").ai_pack.installed === true);
  check("the data is still there", rpc("client.list").some((c) => c.name === "survives-the-update"));
} catch (error) {
  check("completed", false, error.message);
  const log = path.join(root, "loadout-update", "apply-update.log");
  if (fs.existsSync(log)) console.log(fs.readFileSync(log, "utf8"));
} finally {
  try {
    child("cleanup");
  } catch (error) {
    console.log("cleanup:", error.message);
  }
  const left = packageProcesses();
  if (left) console.log("processes left in the package:\n" + left);
  feed.closeAllConnections();
  feed.close();
  spawnSync("powershell", ["-NoProfile", "-Command", `Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith('${root}\\', [StringComparison]::OrdinalIgnoreCase) } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }`], { windowsHide: true });
  await sleep(1500);
  fs.rmSync(root, { recursive: true, force: true, maxRetries: 8, retryDelay: 500 });
  console.log("temporary folder removed:", !fs.existsSync(root));
}
