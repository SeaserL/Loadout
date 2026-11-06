// SPDX-License-Identifier: Apache-2.0
// The updater end to end with a real package: real backend, real folder swap, real rollback, against
// a local stand-in for GitHub. Run it by hand before a release; it takes a few minutes.
//   node scripts/fusion/update-e2e.mjs <full.zip> <lite.zip> <ai-pack.zip>
// 1.1 (ExecPlan 071): updates fetch the app alone and keep or fetch the AI pack; a lite install can add it.
// No window is opened: the helper only reopens one that was running from this package, and none is.
// Loadout.exe is single-instance, so starting it here would take over the owner's window.
import { createHash, randomUUID } from "node:crypto";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { pathToFileURL } from "node:url";

const here = path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1"));

// Run with the package's own Node, in a process of its own: importing runtime.mjs here would load
// keytar's DLL from the package folder and keep that folder from being renamed.
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

const [source, liteSource, packSource] = process.argv.slice(2, 5).map((p) => path.resolve(p ?? ""));
if (![source, liteSource, packSource].every((p) => fs.existsSync(p))) throw Error("usage: update-e2e.mjs <full.zip> <lite.zip> <ai-pack.zip>");
const root = path.join(path.parse(source).root, "LoadoutUpdateE2E-" + randomUUID().slice(0, 8));
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
/** The app (lite asset) with another version, optionally a server.js that will not start, and the AI pack version it asks for. */
function variant(version, broken, packVersion) {
  const out = path.join(work, `Loadout-${version}-lite-portable-windows-x64.zip`);
  python(
    `
import hashlib, json, sys, zipfile
src, out, version, broken, pack = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4] == '1', sys.argv[5]
with zipfile.ZipFile(src) as z, zipfile.ZipFile(out, 'w', zipfile.ZIP_DEFLATED, compresslevel=1) as o:
    manifest = json.loads(z.read('fusion-manifest.json'))
    manifest['productVersion'] = version
    if pack:
        manifest['aiPack'] = pack
    bad = b'process.exit(9);'
    if broken:
        for f in manifest['files']:
            if f['path'] == 'server.js':
                f['sha256'] = hashlib.sha256(bad).hexdigest()
        manifest['treeHash'] = hashlib.sha256(json.dumps(manifest['files'], separators=(',', ':'), ensure_ascii=False).encode()).hexdigest()
    for info in z.infolist():
        if info.filename == 'fusion-manifest.json':
            o.writestr(info.filename, json.dumps(manifest))
        elif broken and info.filename == 'server.js':
            o.writestr(info.filename, bad)
        else:
            o.writestr(info, z.read(info.filename))
`,
    liteSource, out, version, broken ? "1" : "0", packVersion ?? "",
  );
  return out;
}
/** The AI pack asset under another pack version (same files). */
function packVariant(packVersion) {
  const out = path.join(work, `Loadout-ai-${packVersion}-windows-x64.zip`);
  python(
    `
import json, sys, zipfile
src, out, pv = sys.argv[1], sys.argv[2], sys.argv[3]
with zipfile.ZipFile(src) as z, zipfile.ZipFile(out, 'w', zipfile.ZIP_DEFLATED, compresslevel=1) as o:
    for info in z.infolist():
        if info.filename == 'ai-pack.json':
            pack = json.loads(z.read(info.filename))
            pack['packVersion'] = pv
            o.writestr(info.filename, json.dumps(pack))
        else:
            o.writestr(info, z.read(info.filename))
`,
    packSource, out, packVersion,
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
const offerVersion = async (version, files) => {
  const assets = {};
  for (const file of files) assets[path.basename(file)] = { file, size: fs.statSync(file).size, sha: sha(fs.readFileSync(file)) };
  offer = { version, assets };
  pulled.length = 0;
};
const nodeOf = () => path.join(pkg, "node.exe");
function child(op, arg) {
  const args = [path.join(here, "update-e2e.mjs"), "--rpc", pkg, instance, op, ...(arg ? [JSON.stringify(arg)] : [])];
  const done = spawnSync(nodeOf(), args, { encoding: "utf8", windowsHide: true, timeout: 200000 });
  if (done.status !== 0) throw Error(`${op}: ${done.stderr.slice(-300)}`);
  return JSON.parse(done.stdout.trim().split("\n").at(-1));
}
const rpc = (method, params) => {
  const reply = child(method, params);
  if (reply.error) throw Error(`${method}: ${reply.error.message}`);
  return reply.result;
};
async function waitPhase(wanted, ms = 600000) {
  const end = Date.now() + ms;
  let last;
  while (Date.now() < end) {
    last = rpc("update.status");
    if (wanted.includes(last.phase)) return last;
    await sleep(1500);
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
  const done = spawnSync("powershell", ["-NoProfile", "-Command", `@(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith('${pkg}\\', [StringComparison]::OrdinalIgnoreCase) }).Count`], { encoding: "utf8", windowsHide: true });
  return Number(done.stdout.trim());
};

await new Promise((resolve) => feed.listen(0, "127.0.0.1", resolve));
const feedUrl = `http://127.0.0.1:${feed.address().port}/`;
const hasPack = () => fs.existsSync(path.join(pkg, "ai-pack.json"));
const packVersionOf = () => JSON.parse(fs.readFileSync(path.join(pkg, "ai-pack.json"), "utf8")).packVersion;
try {
  step("unpacking the current full package");
  python("import sys,zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])", source, pkg);
  const start = JSON.parse(fs.readFileSync(path.join(pkg, "fusion-manifest.json"), "utf8")).productVersion;
  const firstPack = packVersionOf();
  const bump = (n) => start.replace(/\d+$/, (x) => String(Number(x) + n));
  const next = bump(1);
  const newPack = bump(2);
  const after = bump(3);
  const otherPack = String(Number(firstPack) + 1);

  step("starting it with a client and an account's worth of data");
  const { port } = child("create", { network: "loopback-only", port: 0, updateFeed: feedUrl });
  rpc("update.set_auto_check", { enabled: false });
  rpc("client.create", { name: "survives-the-update" });
  const shown = rpc("daemon.status");
  check("starts on the old version", shown.version.startsWith(start + " "), shown.version);
  check("starts as the full edition", rpc("features.list").ai_pack.installed === true);

  step(`building ${next} (same AI pack), ${newPack} (AI pack ${otherPack}) and a ${after} that will not start`);
  const good = variant(next, false);
  const packed = variant(newPack, false, otherPack);
  const otherPackZip = packVariant(otherPack);
  // It keeps the pack the install has by then, so only the app is downloaded and rolled back.
  const broken = variant(after, true, otherPack);

  await offerVersion(next, [good]);
  step("checking");
  rpc("update.check");
  const found = await waitPhase(["available", "error"]);
  check("finds the new version", found.phase === "available" && found.latest_version === next, JSON.stringify({ phase: found.phase, error: found.error }));

  step("downloading the app only; the AI pack comes from this folder");
  rpc("update.download");
  const ready = await waitPhase(["ready", "error"]);
  check("downloads and verifies", ready.phase === "ready", JSON.stringify({ phase: ready.phase, error: ready.error }));
  check("only the app was downloaded", pulled.length === 1 && pulled[0] === path.basename(good), pulled.join());
  check("the gateway answered throughout", (await fetch(`http://127.0.0.1:${port}/v1/models`)).status === 401);

  step("installing");
  const t0 = Date.now();
  rpc("update.install");
  const installed = await waitResult(undefined);
  check("the helper reports success", installed.status === "installed", installed.reason ?? "");
  const moved = rpc("daemon.status");
  check("runs the new version from the same folder", moved.version.startsWith(next + " "), `${moved.version}, ${Math.round((Date.now() - t0) / 1000)} s`);
  check("still the full edition", hasPack() && packVersionOf() === firstPack);
  check("the same address", (await fetch(`http://127.0.0.1:${port}/v1/models`)).status === 401, String(port));
  check("the data came along", rpc("client.list").some((c) => c.name === "survives-the-update"));
  check("reports what happened", rpc("update.status").last_result?.status === "installed");
  for (let n = 0; n < 60 && fs.existsSync(`${pkg}.previous-${start}`); n++) await sleep(1000);
  check("the old folder is cleaned up", !fs.existsSync(`${pkg}.previous-${start}`));

  await offerVersion(newPack, [packed, otherPackZip]);
  step(`a version that needs AI pack ${otherPack}`);
  rpc("update.check");
  await waitPhase(["available"]);
  rpc("update.download");
  const readyPack = await waitPhase(["ready", "error"]);
  check("downloads the app and the new pack", readyPack.phase === "ready" && pulled.length === 2, JSON.stringify({ phase: readyPack.phase, error: readyPack.error, pulled }));
  const previous = installed.at_ms;
  rpc("update.install");
  const packedResult = await waitResult(previous);
  check("installs it", packedResult.status === "installed" && rpc("daemon.status").version.startsWith(newPack + " "), packedResult.reason ?? "");
  check("carries the new pack", hasPack() && packVersionOf() === otherPack);
  for (let n = 0; n < 60 && fs.existsSync(`${pkg}.previous-${next}`); n++) await sleep(1000);

  await offerVersion(after, [broken]);
  step("now a new version that will not start");
  rpc("update.check");
  await waitPhase(["available"]);
  rpc("update.download");
  const readyBad = await waitPhase(["ready", "error"]);
  check("the broken package passes its own manifest", readyBad.phase === "ready", JSON.stringify({ phase: readyBad.phase, error: readyBad.error }));
  rpc("update.install");
  const failed = await waitResult(packedResult.at_ms);
  check("the helper reports the failure", failed.status === "failed", failed.reason ?? "");
  const restored = rpc("daemon.status");
  check("the previous version is running again", restored.version.startsWith(newPack + " "), restored.version);
  check("the pack survived the rollback", hasPack() && packVersionOf() === otherPack);
  check("the same address after the rollback", (await fetch(`http://127.0.0.1:${port}/v1/models`)).status === 401);
  check("the data survived the rollback", rpc("client.list").some((c) => c.name === "survives-the-update"));
  check("no leftover folder", !fs.existsSync(`${pkg}.previous-${newPack}`));
  check("tells the owner", rpc("update.status").last_result?.status === "failed");

  step("a lite install adds the AI pack from the release it came from");
  child("cleanup");
  fs.rmSync(pkg, { recursive: true, force: true, maxRetries: 8, retryDelay: 500 });
  fs.rmSync(instance, { recursive: true, force: true, maxRetries: 8, retryDelay: 500 });
  python("import sys,zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])", liteSource, pkg);
  child("create", { network: "loopback-only", port: 0, updateFeed: feedUrl });
  rpc("update.set_auto_check", { enabled: false });
  check("lite starts without the pack", !hasPack() && rpc("features.list").ai_pack.installed === false);
  check("the model compression level is unavailable", rpc("features.list").features.find((f) => f.key === "compression").options[0].choices.find((c) => c.value === "model").available === false);
  // The asset under its release name, as the updater asks for it.
  await offerVersion(start, [packVariant(firstPack)]);
  rpc("update.install_pack");
  let pack;
  for (let n = 0; n < 300; n++) {
    pack = rpc("features.list").ai_pack;
    if (pack.phase === "installed" || pack.phase === "error") break;
    await sleep(1500);
  }
  check("installs the AI pack", pack?.installed === true && hasPack() && packVersionOf() === firstPack, JSON.stringify(pack));
  // The pack reads as installed once its list is in; the updater still clears its staging folder.
  const lock = path.join(root, "loadout-update", "updater.lock");
  for (let n = 0; n < 60 && fs.existsSync(lock); n++) await sleep(1000);
  check("the updater finishes", !fs.existsSync(lock));
  check("the model compression level is available", rpc("features.list").features.find((f) => f.key === "compression").options[0].choices.find((c) => c.value === "model").available === true);
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
  if (left) {
    console.log("processes left in the package:", left);
    const which = spawnSync("powershell", ["-NoProfile", "-Command", `Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith('${pkg}\\', [StringComparison]::OrdinalIgnoreCase) } | ForEach-Object { '  ' + $_.ProcessId + ' ' + $_.CommandLine.Replace('${pkg}', '<package>') }`], { encoding: "utf8", windowsHide: true });
    console.log(which.stdout.trimEnd());
  }
  feed.closeAllConnections();
  feed.close();
  spawnSync("powershell", ["-NoProfile", "-Command", `Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith('${root}\\', [StringComparison]::OrdinalIgnoreCase) } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }`], { windowsHide: true });
  await sleep(1500);
  fs.rmSync(root, { recursive: true, force: true, maxRetries: 8, retryDelay: 500 });
  console.log("temporary folder removed:", !fs.existsSync(root));
}
