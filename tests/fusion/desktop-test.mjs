// SPDX-License-Identifier: Apache-2.0
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";
import { spawn } from "node:child_process";
import { Instance, baseline, pkg } from "./harness.mjs";
const root = path.resolve(process.env.FUSION_RELEASE_ROOT);
const { prepareDesktop, desktopDirectory, choosePort } = await import(pathToFileURL(path.join(pkg, "tools/desktop.mjs")));
const dir = path.join(root, "runs/loadout-fusion-isolated/desktop-entry-" + Date.now());
const result = { baseline: baseline(), cases: [] };
let i, target, before, windowFirst;
async function check(name, fn) { await fn(); result.cases.push({ name, pass: true }); }
async function cli(command) {
  const child = spawn(path.join(pkg, "node.exe"), [path.join(pkg, "tools/desktop.mjs"), command, dir], { windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
  let out = "", err = "";
  child.stdout.on("data", c => { out += c; }); child.stderr.on("data", c => { err += c; });
  const code = await new Promise((resolve, reject) => { child.on("error", reject); child.on("exit", resolve); });
  assert.equal(code, 0, "Desktop helper failed");
  assert(!out.includes("password") && !err.includes("password"));
  return JSON.parse(out);
}
try {
  await check("desktop-paths-reject-production-and-package-storage", async () => {
    assert.throws(() => desktopDirectory(path.join(root, "unsafe")));
    assert.throws(() => desktopDirectory(path.join(pkg, "loadout-fusion-isolated/data")));
    assert(desktopDirectory().endsWith(path.join("loadout-fusion-isolated", "desktop-current")));
  });
  await check("cold-start-uses-packaged-backend-and-assets", async () => {
    target = (await prepareDesktop(dir)).target; i = new Instance(dir); before = await i.status();
    assert.equal(before.status, "ready");
    assert(target.url.endsWith("/loadout/index.html"));
    assert.equal(i.meta.config.network, "direct");
    const html = await (await fetch(target.url)).text();
    const assets = [...html.matchAll(/(?:src|href)="(\/loadout\/assets\/[^\"]+)"/g)];
    assert(assets.length >= 2); assert(!html.includes("@vite/client"));
    for (const asset of assets) assert.equal((await fetch(new URL(asset[1], target.url))).status, 200);
    assert.equal(fs.readFileSync(path.join(pkg, "Loadout.exe")).subarray(0, 2).toString(), "MZ");
  });
  await check("repeat-prepare-reuses-instance-and-backend", async () => {
    assert.deepEqual(await cli("prepare"), target);
    assert.equal((await i.status()).backendPid, before.backendPid);
  });
  await check("shell-gets-a-session-without-any-password-prompt", async () => {
    const prepared = await cli("prepare-session");
    assert.equal(prepared.url, target.url);
    assert.match(prepared.session, /^auth_token=/);
    const secrets = await i.secrets();
    assert(!JSON.stringify(prepared).includes(secrets.password));
    const signed = await fetch(new URL("/api/providers", target.url), { headers: { cookie: prepared.session } });
    assert.equal(signed.status, 200);
  });
  await check("management-still-refuses-anyone-without-the-session", async () => {
    const r = await fetch(new URL("/api/providers", target.url)); assert.equal(r.status, 401);
    const rpc = await fetch(new URL("/api/loadout/rpc", target.url), { method: "POST", body: "{}" });
    assert.equal(rpc.status, 401);
    assert.equal((await i.admin("/api/providers")).connections.length, 0);
    assert.equal((await i.admin("/api/combos")).combos.length, 0);
  });
  await check("explicit-stop-and-restart-preserve-instance-and-port", async () => {
    const id = i.meta.id;
    assert.deepEqual(await cli("stop"), { status: "stopped" });
    assert.equal((await i.status()).status, "stopped");
    assert.deepEqual(await cli("prepare"), target);
    const restarted = await i.status();
    assert.equal(restarted.port, before.port); assert.notEqual(restarted.backendPid, before.backendPid);
    assert.equal(new Instance(dir).meta.id, id);
    // The address is written down, not picked again at each start.
    assert.equal(new Instance(dir).meta.config.port, before.port);
    const held = http.createServer();
    await new Promise((resolve) => held.listen(0, "127.0.0.1", resolve));
    const taken = held.address().port;
    assert.notEqual(await choosePort(taken), taken);
    await new Promise((resolve) => held.close(resolve));
    assert.equal(await choosePort(taken), taken);
  });
  await check("a-lock-left-by-a-shutdown-does-not-block-the-next-start", async () => {
    // After a restart the numbers in a stale lock can belong to any program; here, a ping.
    assert.deepEqual(await cli("stop"), { status: "stopped" });
    const other = spawn(path.join(process.env.SystemRoot ?? "C:\\Windows", "System32", "PING.EXE"), ["-n", "120", "127.0.0.1"], { windowsHide: true, stdio: "ignore" });
    try {
      fs.writeFileSync(path.join(dir, "operation.lock"), JSON.stringify({ id: "left-by-a-shutdown", pid: other.pid, backendPid: other.pid }));
      assert.equal((await i.status()).status, "stopped");
      assert.deepEqual(await cli("prepare"), target);
      assert.equal((await i.status()).status, "ready");
    } finally {
      other.kill();
    }
  });
  await check("first-start-after-the-window-made-its-profile", async () => {
    // The window builds <instance>\webview before it asks for the backend: on a new install that
    // folder is all there is, and the start must not stop there.
    const fresh = path.join(root, "runs/loadout-fusion-isolated/desktop-window-first-" + Date.now());
    fs.mkdirSync(path.join(fresh, "webview", "EBWebView"), { recursive: true });
    windowFirst = (await prepareDesktop(fresh)).instance;
    assert.equal((await windowFirst.status()).status, "ready");
    assert(fs.existsSync(path.join(fresh, "instance.json")));
    assert(fs.existsSync(path.join(fresh, "webview", "EBWebView")), "the window's profile is kept");
  });
  await check("anything-else-in-a-new-instance-folder-is-still-refused", async () => {
    const makers = { file: (d) => fs.writeFileSync(path.join(d, "notes.txt"), "x"), data: (d) => fs.mkdirSync(path.join(d, "data")) };
    for (const [name, make] of Object.entries(makers)) {
      const taken = path.join(root, `runs/loadout-fusion-isolated/desktop-not-empty-${name}-` + Date.now());
      fs.mkdirSync(path.join(taken, "webview"), { recursive: true });
      make(taken);
      await assert.rejects(prepareDesktop(taken), /TARGET_EXISTS/);
      assert(!fs.existsSync(path.join(taken, "instance.json")));
    }
  });
  await check("launch-receipt-does-not-contain-bootstrap-secrets", async () => {
    const secret = await i.secrets();
    const publicText = JSON.stringify(target) + fs.readFileSync(path.join(dir, "instance.json"), "utf8");
    for (const value of Object.values(secret)) assert(!publicText.includes(value));
  });
} catch (error) { result.failure = { name: error.name, message: error.message }; process.exitCode = 1; }
finally {
  if (i) await i.cleanup();
  if (windowFirst) await windowFirst.cleanup();
  result.cleaned = true;
  fs.writeFileSync(path.join(root, "evidence/desktop.json"), JSON.stringify(result, null, 2));
  console.log(JSON.stringify({ passed: result.cases.length, failed: !!result.failure }));
}
