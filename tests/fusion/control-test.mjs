// SPDX-License-Identifier: Apache-2.0
// The Loadout control plane on the packaged backend, against a mock upstream only.
import fs from "node:fs";
import path from "node:path";
import http from "node:http";
import assert from "node:assert/strict";
import { once } from "node:events";
import { createDecipheriv, pbkdf2Sync } from "node:crypto";
import { DatabaseSync } from "node:sqlite";
import { spawnSync } from "node:child_process";
import { Instance, sleep, baseline, pkg } from "./harness.mjs";

const root = path.resolve(process.env.FUSION_RELEASE_ROOT);
const dir = path.join(root, "runs/loadout-fusion-isolated/control-" + Date.now());
const result = { baseline: baseline(), cases: [] };
const KEYS = { a: "synthetic-control-key-a", b: "synthetic-control-key-b", c: "synthetic-control-key-c" };
let failing = new Set();
const served = [];
let upstreamCalls = 0;
const mock = http.createServer(async (req, res) => {
  let raw = "";
  for await (const c of req) raw += c;
  upstreamCalls++;
  res.setHeader("content-type", "application/json");
  const key = String(req.headers.authorization ?? "").replace("Bearer ", "");
  if (req.method === "GET")
    return res.end(JSON.stringify({ data: [{ id: "fixture-chat" }, { id: "fixture-other" }] }));
  const body = JSON.parse(raw);
  if (failing.has(key)) {
    res.statusCode = 429;
    return res.end(JSON.stringify({ error: { message: "rate limited" } }));
  }
  served.push({ key, model: body.model });
  if (body.stream) {
    res.setHeader("content-type", "text/event-stream");
    const frame = (delta, usage) =>
      `data: ${JSON.stringify({ id: "mock-s", object: "chat.completion.chunk", model: body.model, choices: [{ index: 0, delta, finish_reason: usage ? "stop" : null }], ...(usage ? { usage } : {}) })}\n\n`;
    for (let n = 0; n < 12; n++) {
      res.write(frame({ content: "REPLY-TEXT " }));
      await sleep(120);
    }
    res.write(frame({}, { prompt_tokens: 100, completion_tokens: 12, total_tokens: 112 }));
    return res.end("data: [DONE]\n\n");
  }
  res.end(
    JSON.stringify({
      id: "mock-" + served.length,
      object: "chat.completion",
      model: body.model,
      choices: [{ index: 0, message: { role: "assistant", content: "OK" }, finish_reason: "stop" }],
      usage: { prompt_tokens: 100, completion_tokens: 10, total_tokens: 110 },
    }),
  );
});
mock.listen(0, "127.0.0.1");
await once(mock, "listening");
const base = `http://127.0.0.1:${mock.address().port}/v1`;

let i, clientKey;
// The sign-in entry is written to a throwaway key; the real Run key is only ever read, to show it
// did not change.
const AUTOSTART_KEY = "HKCU:\\Software\\Loadout-Test\\control-" + Date.now();
const RUN_KEY = "HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const POWERSHELL = path.join(process.env.SystemRoot ?? "C:\\Windows", "System32", "WindowsPowerShell", "v1.0", "powershell.exe");
function registry(key, action = "read", value = "") {
  if (action !== "read" && !key.startsWith("HKCU:\\Software\\Loadout-Test\\")) throw Error("only the test key is written");
  const script =
    "$ProgressPreference='SilentlyContinue'; [Console]::OutputEncoding=[Text.Encoding]::UTF8; $k=$env:FIXTURE_KEY; " +
    "if ($env:FIXTURE_ACTION -eq 'write') { if (-not (Test-Path -LiteralPath $k)) { $null = New-Item -Path $k -Force }; Set-ItemProperty -LiteralPath $k -Name Loadout -Value $env:FIXTURE_VALUE } " +
    "elseif ($env:FIXTURE_ACTION -eq 'drop') { Remove-Item -LiteralPath $k -Recurse -Force -ErrorAction SilentlyContinue } " +
    "else { $v = (Get-ItemProperty -LiteralPath $k -Name Loadout -ErrorAction SilentlyContinue).Loadout; if ($v) { [Console]::Out.Write($v) } }";
  const done = spawnSync(POWERSHELL, ["-NoProfile", "-NonInteractive", "-Command", script], {
    encoding: "utf8",
    windowsHide: true,
    env: { ...process.env, FIXTURE_KEY: key, FIXTURE_ACTION: action, FIXTURE_VALUE: value },
  });
  if (done.status !== 0) throw Error("registry fixture failed");
  return done.stdout.trim();
}
const check = async (name, fn) => {
  try {
    await fn();
  } catch (error) {
    if (!process.env.CONTROL_KEEP_GOING) throw error;
    console.log(`FAIL ${name}: ${String(error.message).slice(0, 300)}`);
    return;
  }
  result.cases.push({ name, pass: true });
  console.log("PASS " + name);
};
const rpc = async (method, params) => {
  const r = await i.admin("/api/loadout/rpc", "POST", { method, params });
  if (r.error) throw Error(`${method}: ${r.error.message}`);
  return r.result;
};
const rpcError = async (method, params) => (await i.admin("/api/loadout/rpc", "POST", { method, params })).error;
const chat = (model, url = "/v1/chat/completions", key = clientKey) =>
  fetch(i.base + url, {
    method: "POST",
    headers: { authorization: "Bearer " + key, "content-type": "application/json" },
    body: JSON.stringify({ model, messages: [{ role: "user", content: "fixture" }] }),
  });
async function served1(model, url) {
  const n = served.length;
  const r = await chat(model, url);
  assert.equal(r.status, 200, `${model} returned ${r.status}: ${await r.clone().text()}`);
  assert(served.length > n, "upstream not reached");
  return served.at(-1).key;
}

try {
  i = await Instance.create(dir, undefined, { network: "loopback-only", port: 0, autostartKey: AUTOSTART_KEY });
  await i.start();

  await check("rpc-needs-the-session-and-refuses-inference-credentials", async () => {
    const anon = await fetch(i.base + "/api/loadout/rpc", {
      method: "POST",
      headers: { "content-type": "application/json", origin: i.base },
      body: JSON.stringify({ method: "daemon.status" }),
    });
    assert.equal(anon.status, 401);
    const foreign = await fetch(i.base + "/api/loadout/rpc", {
      method: "POST",
      headers: { "content-type": "application/json", origin: "https://example.com", cookie: i.cookie, "x-omniroute-csrf": i.csrf },
      body: JSON.stringify({ method: "daemon.status" }),
    });
    assert.equal(foreign.status, 403);
    assert.equal((await rpcError("no.such.method")).code, "unknown_method");
  });

  await check("sign-in-entry-starts-this-package-and-removes-only-its-own-value", async () => {
    const realBefore = registry(RUN_KEY);
    const expected = `"${path.join(pkg, "Loadout.exe")}" --instance "${dir}" --background`;
    assert.deepEqual(await rpc("system.autostart"), { enabled: false });
    assert.deepEqual(await rpc("system.set_autostart", { enabled: true }), { enabled: true });
    assert.equal(registry(AUTOSTART_KEY), expected);
    assert.deepEqual(await rpc("system.set_autostart", { enabled: false }), { enabled: false });
    assert.equal(registry(AUTOSTART_KEY), "");
    // Another program's value under the same name, in a folder with Chinese in it, is not ours.
    const foreign = "C:\\工具\\其他\\other.exe --background";
    registry(AUTOSTART_KEY, "write", foreign);
    assert.deepEqual(await rpc("system.autostart"), { enabled: false });
    assert.deepEqual(await rpc("system.set_autostart", { enabled: false }), { enabled: false });
    assert.equal(registry(AUTOSTART_KEY), foreign);
    assert.deepEqual(await rpc("system.set_autostart", { enabled: true }), { enabled: true });
    assert.equal(registry(AUTOSTART_KEY), expected);
    assert.equal(registry(RUN_KEY), realBefore, "the real sign-in entry is never touched");
  });

  await check("status-and-provider-presets", async () => {
    const status = await rpc("daemon.status");
    assert.equal(status.gateway.listening, true);
    // Limited to this computer, the updater stays out of it: reading the status starts nothing.
    const update = await rpc("update.status");
    assert.equal(update.phase, "disabled");
    assert.match(update.current_version, /^\d+\.\d+\.\d+$/);
    await assert.rejects(rpc("update.check"), /本机模式/);
    await assert.rejects(rpc("update.open_notes"), /没有可以查看/);
    const kinds = await rpc("provider.kinds");
    assert(kinds.some((k) => k.kind === "sensenova" && k.default_base_url));
    assert(kinds.some((k) => k.kind === "openai-compatible" && k.fixed_slug === null));
    assert(kinds.every((k) => !("usage" in k)));
  });

  await check("add-provider-and-accounts-keeps-keys-out-of-sqlite", async () => {
    await rpc("provider.add", { slug: "north", kind: "openai-compatible", base_url: base });
    for (const [alias, key] of Object.entries(KEYS)) {
      const added = await rpc("account.add", { provider: "north", alias, api_key: key });
      assert.equal(added.sync_error, null);
      assert.equal(added.account.has_key, true);
      assert.equal(added.account.enabled, true);
    }
    const accounts = await rpc("account.list");
    assert.deepEqual(accounts.map((a) => a.alias), ["a", "b", "c"]);
    assert(!JSON.stringify(accounts).includes("synthetic-control-key"));
    const db = new DatabaseSync(path.join(i.dir, "data/storage.sqlite"), { readOnly: true });
    const dump = JSON.stringify(db.prepare("SELECT * FROM provider_connections").all());
    db.close();
    for (const key of Object.values(KEYS)) assert(!dump.includes(key));
  });

  await check("models-are-named-provider-account-model", async () => {
    const models = await rpc("model.list");
    assert(models.some((m) => m.name === "north/a/fixture-chat" && m.enabled));
    assert.equal(models.filter((m) => m.upstream_model === "fixture-chat").length, 3);
    const created = await rpc("client.create", { name: "fixture-app" });
    clientKey = created.key;
    assert(clientKey?.startsWith("sk-"));
    assert.equal(created.client.name, "fixture-app");
  });

  await check("exact-name-reaches-that-account", async () => {
    assert.equal(await served1("north/b/fixture-chat"), KEYS.b);
    assert.equal(await served1("north/c/fixture-chat"), KEYS.c);
  });

  await check("dragged-account-order-is-the-failover-order", async () => {
    await rpc("account.reorder", { provider: "north", aliases: ["c", "a", "b"] });
    assert.deepEqual((await rpc("account.list")).map((a) => a.alias), ["c", "a", "b"]);
    assert.equal(await served1("north/fixture-chat"), KEYS.c);
    failing = new Set([KEYS.c]);
    assert.equal(await served1("north/fixture-other"), KEYS.a);
    failing = new Set();
    await rpc("account.reorder", { provider: "north", aliases: ["a", "b", "c"] });
  });

  await check("model-group-is-a-single-model-name-in-its-order", async () => {
    const pool = await rpc("pool.save", {
      name: "daily",
      ordering: "manual",
      members: ["north/b/fixture-chat", "north/a/fixture-chat"],
    });
    assert.deepEqual(pool.members, ["north/b/fixture-chat", "north/a/fixture-chat"]);
    assert.equal(await served1("loadout/daily"), KEYS.b);
    await rpc("pool.save", {
      name: "daily",
      ordering: "manual",
      members: ["north/a/fixture-chat", "north/b/fixture-chat"],
    });
    assert.equal(await served1("loadout/daily"), KEYS.a);
    const virtuals = await rpc("route.virtuals");
    assert(virtuals.some((v) => v.name === "loadout/daily" && v.kind === "pool"));
  });

  await check("sub-pool-keeps-other-accounts-out-of-every-path", async () => {
    await rpc("subpool.save", { name: "only-c", accounts: ["north/c"], rename_from: null });
    await rpc("subpool.activate", { name: "only-c" });
    assert.equal(await served1("north/fixture-chat"), KEYS.c);
    assert.equal((await chat("north/a/fixture-chat")).status >= 400, true);
    assert.equal((await chat("loadout/daily")).status >= 400, true);
    await rpc("subpool.activate", { name: null });
    assert.equal(await served1("loadout/daily"), KEYS.a);
    assert.equal((await rpc("subpool.list")).active, null);
  });

  await check("unified-api-serves-bare-names-from-its-accounts", async () => {
    const endpoint = await rpc("endpoint.save", {
      name: "north",
      provider: "north",
      enabled: true,
      members: ["b"],
      default_model: null,
      wait_secs: 0,
      rotate: false,
    });
    assert(endpoint.openai_base_url.endsWith("/e/north/v1"));
    assert.equal(await served1("fixture-chat", "/e/north/v1/chat/completions"), KEYS.b);
    const listed = await fetch(i.base + "/e/north/v1/models", { headers: { authorization: "Bearer " + clientKey } });
    assert.equal(listed.status, 200);
    const listedNames = (await listed.json()).data.map((m) => m.id);
    assert.deepEqual(listedNames, ["fixture-chat", "fixture-other"]);
    // The endpoint has one address too: an Anthropic SDK appends /v1/messages to the same .../v1.
    const viaAnthropic = await fetch(i.base + "/e/north/v1/v1/messages", {
      method: "POST",
      headers: { "x-api-key": clientKey, "anthropic-version": "2023-06-01", "content-type": "application/json" },
      body: JSON.stringify({ model: "fixture-chat", max_tokens: 16, messages: [{ role: "user", content: "fixture" }] }),
    });
    assert.equal(viaAnthropic.status, 200);
    assert.equal((await viaAnthropic.json()).type, "message");
    assert.equal(served.at(-1).key, KEYS.b);
    const listedTwice = await fetch(i.base + "/e/north/v1/v1/models", { headers: { "x-api-key": clientKey } });
    assert.deepEqual((await listedTwice.json()).data.map((m) => m.id), listedNames);
    const anon = await fetch(i.base + "/e/north/v1/models");
    assert.equal(anon.status, 401);
    await rpc("endpoint.save", { name: "north", provider: "north", enabled: false, members: null, rotate: false });
    assert.equal((await chat("fixture-chat", "/e/north/v1/chat/completions")).status, 404);
  });

  await check("one-address-serves-openai-and-anthropic-clients", async () => {
    const base = i.base + "/v1";
    const model = "north/a/fixture-chat";
    const openai = await fetch(base + "/chat/completions", {
      method: "POST",
      headers: { authorization: "Bearer " + clientKey, "content-type": "application/json" },
      body: JSON.stringify({ model, messages: [{ role: "user", content: "fixture" }] }),
    });
    assert.equal(openai.status, 200);
    assert(Array.isArray((await openai.json()).choices));
    const anthropic = await fetch(base + "/v1/messages", {
      method: "POST",
      headers: { "x-api-key": clientKey, "anthropic-version": "2023-06-01", "content-type": "application/json" },
      body: JSON.stringify({ model, max_tokens: 16, messages: [{ role: "user", content: "fixture" }] }),
    });
    assert.equal(anthropic.status, 200);
    assert.equal((await anthropic.json()).type, "message");
    assert.equal(served.at(-1).key, KEYS.a);
    // Both see the same names, and neither gets in without a key.
    const once = await fetch(base + "/models", { headers: { authorization: "Bearer " + clientKey } });
    const twice = await fetch(base + "/v1/models", { headers: { "x-api-key": clientKey } });
    assert.deepEqual((await twice.json()).data.map((m) => m.id), (await once.json()).data.map((m) => m.id));
    assert.equal((await fetch(base + "/v1/models")).status, 401);
    assert.equal(
      (await fetch(base + "/v1/messages", { method: "POST", headers: { "content-type": "application/json" }, body: "{}" })).status,
      401,
    );
  });

  await check("client-sees-its-own-names-and-needs-its-key", async () => {
    assert.equal((await fetch(i.base + "/v1/models")).status, 401);
    const r = await fetch(i.base + "/v1/models", { headers: { authorization: "Bearer " + clientKey } });
    assert.equal(r.status, 200);
    const ids = (await r.json()).data.map((m) => m.id);
    assert(ids.includes("loadout/daily"));
    assert(ids.includes("north/a/fixture-chat"));
    assert(!ids.some((id) => id.startsWith("openai-compatible-")));
  });

  await check("switched-off-model-is-refused", async () => {
    await rpc("model.set_enabled", { name: "north/a/fixture-other", enabled: false });
    assert.equal((await chat("north/a/fixture-other")).status, 400);
    assert.equal(await served1("north/fixture-other"), KEYS.b);
    await rpc("model.set_enabled", { name: "north/a/fixture-other", enabled: true });
  });

  await check("client-policy-limits-models-and-accounts", async () => {
    const named = await rpc("client.create", { name: "scoped-app" });
    await rpc("client.set_policy", {
      name: "scoped-app",
      policy: { allowed_models: ["north/b/fixture-chat", "loadout/daily"], allowed_accounts: ["north/b"], requests_per_minute: 50 },
    });
    // A name nothing answers to is refused, and the policy that was there stays.
    for (const wrong of ["north/b/no-such-model", "loadout/no-such-group", "not a model", "north/*"]) {
      const refused = await rpcError("client.set_policy", { name: "scoped-app", policy: { allowed_models: [wrong] } });
      assert.equal(refused?.code, "invalid_params", wrong);
    }
    assert.deepEqual(
      (await rpc("client.list")).find((c) => c.name === "scoped-app").policy.allowed_models.sort(),
      ["loadout/daily", "north/b/fixture-chat"],
    );
    assert.equal((await chat("north/b/fixture-chat", undefined, named.key)).status, 200);
    // A named account is a preference (ADR-003 §8): the key's scope decides who may serve it.
    const n = served.length;
    assert.equal((await chat("north/c/fixture-chat", undefined, named.key)).status, 200);
    assert.equal(served.at(-1).key, KEYS.b);
    assert.equal(served.length, n + 1);
    assert((await chat("north/b/fixture-other", undefined, named.key)).status >= 400);
    const info = (await rpc("client.list")).find((c) => c.name === "scoped-app");
    assert.deepEqual(info.policy.allowed_accounts, ["north/b"]);
    assert.equal(info.policy.requests_per_minute, 50);
    // The per-minute limit is enforced, not only stored.
    await rpc("client.set_policy", { name: "scoped-app", policy: { requests_per_minute: 1 } });
    const burst = [];
    for (let n = 0; n < 3; n++) burst.push((await chat("north/b/fixture-chat", undefined, named.key)).status);
    assert(burst.includes(429), `no 429 in ${burst}`);
    // A migrated key limits through its own rule list: it has to show and stay editable.
    const carried = await rpc("client.create", { name: "carried-rules" });
    const { keys } = await i.admin("/api/keys");
    const row = keys.find((k) => k.name === "carried-rules");
    await i.admin(`/api/keys/${row.id}`, "PATCH", { rateLimits: [{ limit: 8, window: 60 }] });
    const carriedInfo = async () => (await rpc("client.list")).find((c) => c.name === "carried-rules").policy.requests_per_minute;
    assert.equal(await carriedInfo(), 8);
    await rpc("client.set_policy", { name: "carried-rules", policy: { requests_per_minute: 20 } });
    assert.equal(await carriedInfo(), 20);
    assert.equal((await chat("north/b/fixture-chat", undefined, carried.key)).status, 200);
    await rpc("client.revoke", { name: "carried-rules" });
    await rpc("client.revoke", { name: "scoped-app" });
    assert.equal((await chat("north/b/fixture-chat", undefined, named.key)).status, 401);
  });

  await check("flow-and-stats-come-from-real-requests", async () => {
    await sleep(800);
    const { links } = await rpc("stats.flow", { since_ms: Date.now() - 3600000 });
    const mine = links.filter((l) => l.client === "fixture-app");
    assert(mine.some((l) => l.provider === "north" && l.account === "b" && l.requests > 0));
    assert(mine.every((l) => !String(l.provider).startsWith("openai-compatible-")));
    const byModel = await rpc("stats.summary", { since_ms: Date.now() - 3600000, group_by: "model" });
    // A day and an hour are the viewer's own; an upper bound cuts off what came later.
    const hourAgo = Date.now() - 3600000;
    const shifted = new Date(Date.now() + 480 * 60000).toISOString();
    const days = await rpc("stats.summary", { since_ms: hourAgo, group_by: "day", tz_offset_min: 480 });
    assert(days.some((d) => d.key === shifted.slice(0, 10)), JSON.stringify(days.map((d) => d.key)));
    const hours = await rpc("stats.summary", { since_ms: hourAgo, group_by: "hour", tz_offset_min: 480 });
    assert(hours.some((h) => h.key === shifted.slice(11, 13)), JSON.stringify(hours.map((h) => h.key)));
    assert.deepEqual(await rpc("stats.summary", { since_ms: hourAgo, until_ms: hourAgo + 1, group_by: "model" }), []);
    assert(byModel.some((e) => e.key === "fixture-chat" && e.requests > 0 && e.input_tokens > 0));
    const recent = await rpc("requests.recent", { limit: 20 });
    assert(recent.some((r) => r.route?.startsWith("north/")));
    assert(!JSON.stringify(recent).includes("fixture\""));
    const status = await rpc("route.status");
    assert(status.some((s) => s.name === "north/a/fixture-chat" && s.successes > 0));
    const usage = await rpc("usage.list", { refresh: false });
    assert(usage.some((u) => u.alias === "b" && u.spend_today.requests > 0));
  });

  await check("live-view-follows-a-request-and-carries-no-content", async () => {
    await sleep(800);
    const start = await rpc("stats.live", { after: null });
    assert.deepEqual(start.done, []);
    const response = await fetch(i.base + "/v1/chat/completions", {
      method: "POST",
      headers: { authorization: "Bearer " + clientKey, "content-type": "application/json" },
      body: JSON.stringify({ model: "north/a/fixture-chat", stream: true, messages: [{ role: "user", content: "PROMPT-TEXT" }] }),
    });
    assert.equal(response.status, 200);
    const reader = response.body.getReader();
    let live;
    let moving;
    for (let n = 0; n < 40 && !moving; n++) {
      await reader.read();
      live = await rpc("stats.live", { after: start.cursor });
      moving = live.active.find((r) => r.bytes > 0);
    }
    assert(moving, "no request seen moving");
    assert.equal(moving.client, "fixture-app");
    assert.equal(moving.provider, "north");
    assert.equal(moving.account, "a");
    assert.equal(moving.answering, true);
    // The count grows with what reaches the client, while the request is still running.
    await reader.read();
    await sleep(300);
    const later = (await rpc("stats.live", { after: start.cursor })).active.find((r) => r.id === moving.id);
    assert(later?.bytes > moving.bytes, "byte count did not grow");
    const wire = JSON.stringify([live, later]);
    assert(!wire.includes("PROMPT-TEXT") && !wire.includes("REPLY-TEXT"), "content in the live view");
    while (!(await reader.read()).done);
    let done;
    let end;
    for (let n = 0; n < 40; n++) {
      end = await rpc("stats.live", { after: start.cursor });
      done = end.done.find((r) => r.account === "a");
      if (done && end.active.length === 0) break;
      await sleep(100);
    }
    assert.equal(end.active.length, 0);
    assert.equal(done?.ok, true);
    assert.equal(done.client, "fixture-app");
    assert.equal(done.provider, "north");
    // Each logged request is reported once.
    assert.deepEqual((await rpc("stats.live", { after: end.cursor })).done, []);
  });

  await check("monthly-limit-is-the-native-budget-window", async () => {
    const { keys } = await i.admin("/api/keys");
    const id = keys.find((k) => k.name === "fixture-app").id;
    await i.admin("/api/usage/budget", "POST", { apiKeyId: id, dailyLimitUsd: 0, weeklyLimitUsd: 0, monthlyLimitUsd: 25, resetInterval: "monthly" });
    const saved = await i.admin(`/api/usage/budget?apiKeyId=${encodeURIComponent(id)}`);
    assert.equal(saved.monthlyLimitUsd, 25);
    assert.equal(saved.resetInterval, "monthly");
    await i.admin("/api/usage/budget", "POST", { apiKeyId: id, dailyLimitUsd: 0, weeklyLimitUsd: 0, monthlyLimitUsd: 0, resetInterval: "monthly" });
    assert.equal((await chat("north/a/fixture-chat")).status, 200);
  });

  await check("one-click-export-is-the-loadout-keys-envelope", async () => {
    const target = path.join(i.dir, "export.loadout-keys");
    assert.match((await rpcError("keys.export", { passphrase: "fixture-passphrase", confirm: "no", path: target })).message, /EXPORT/);
    assert.match((await rpcError("keys.export", { passphrase: "short", confirm: "EXPORT", path: target })).message, /8/);
    const out = await rpc("keys.export", { passphrase: "fixture-passphrase", confirm: "EXPORT", path: target });
    assert.equal(out.accounts, 3);
    const text = fs.readFileSync(target, "utf8");
    for (const key of Object.values(KEYS)) assert(!text.includes(key));
    // Decrypted here the way crates/core/src/keys_export.rs does it: same fields, same AAD.
    const e = JSON.parse(text);
    assert.equal(e.format, "loadout-keys");
    const key = pbkdf2Sync("fixture-passphrase", Buffer.from(e.salt, "base64"), e.iterations, 32, "sha256");
    const raw = Buffer.from(e.ciphertext, "base64");
    const decipher = createDecipheriv("chacha20-poly1305", key, Buffer.from(e.nonce, "base64"), { authTagLength: 16 });
    const body = raw.subarray(0, raw.length - 16);
    decipher.setAAD(Buffer.from([e.format, e.version, e.kdf, e.iterations, e.salt, e.cipher, e.nonce].join("\n")), { plaintextLength: body.length });
    decipher.setAuthTag(raw.subarray(raw.length - 16));
    const bundle = JSON.parse(Buffer.concat([decipher.update(body), decipher.final()]).toString());
    assert.deepEqual(bundle.providers[0].accounts.map((a) => a.api_key), [KEYS.a, KEYS.b, KEYS.c]);
  });

  await check("import-restores-accounts-and-refuses-a-wrong-passphrase", async () => {
    const content = fs.readFileSync(path.join(i.dir, "export.loadout-keys"), "utf8");
    assert.match((await rpcError("keys.import", { content, passphrase: "wrong-passphrase", overwrite: false })).message, /口令/);
    await rpc("account.remove", { provider: "north", alias: "c" });
    const report = await rpc("keys.import", { content, passphrase: "fixture-passphrase", overwrite: false });
    assert.equal(report.accounts, 1);
    assert.equal(report.skipped.length, 2);
    // The imported account comes with its models: nobody has to find the refresh arrow first.
    assert.deepEqual(report.sync_errors, []);
    assert((await rpc("model.list")).some((m) => m.name === "north/c/fixture-chat"));
    assert.equal(await served1("north/c/fixture-chat"), KEYS.c);
  });

  await check("models-switch-on-and-off-together-and-an-unknown-name-stops-the-batch", async () => {
    const names = ["north/a/fixture-chat", "north/b/fixture-chat"];
    const enabled = async () => {
      const list = await rpc("model.list");
      return names.map((n) => list.find((m) => m.name === n)?.enabled);
    };
    assert.deepEqual(await rpc("model.set_enabled_many", { names, enabled: false }), { updated: 2 });
    assert.deepEqual(await enabled(), [false, false]);
    assert.match((await rpcError("model.set_enabled_many", { names: [...names, "north/a/no-such-model"], enabled: true })).message, /no-such-model/);
    assert.deepEqual(await enabled(), [false, false], "a refused batch changes nothing");
    assert((await rpcError("model.set_enabled_many", { names: [], enabled: true })).message);
    assert.deepEqual(await rpc("model.set_enabled_many", { names, enabled: true }), { updated: 2 });
    assert.deepEqual(await enabled(), [true, true]);
  });

  await check("settings-apply-and-idle-gateway-stays-quiet", async () => {
    await rpc("settings.set", { key: "latency.fast_ms", value: "1500" });
    assert.equal((await rpc("settings.list")).find((s) => s.key === "latency.fast_ms").value, "1500");
    assert((await rpcError("settings.set", { key: "unknown.key", value: "1" })).message);
    const before = upstreamCalls;
    await sleep(35000);
    assert.equal(upstreamCalls, before);
  });

  await check("restart-from-the-ui-brings-the-backend-back", async () => {
    // A year of history must survive: the upstream defaults delete a request after 7 or 30 days.
    const running = await rpc("daemon.status");
    assert.equal(running.log_retention_days, 400);
    assert.match(running.version, /^\d+\.\d+\.\d+ · OmniRoute 3\.8\.52$/);
    const insertOld = (model, daysAgo) => {
      const writer = new DatabaseSync(path.join(i.dir, "data/storage.sqlite"));
      writer
        .prepare("INSERT INTO call_logs (id, timestamp, method, path, status, model, provider, tokens_in, tokens_out) VALUES (?, ?, 'POST', '/v1/chat/completions', 200, ?, 'north', 10, 5)")
        .run(model, new Date(Date.now() - daysAgo * 86400000).toISOString(), model);
      writer.close();
    };
    insertOld("fixture-old", 100);
    const before = (await i.status()).backendPid;
    await rpc("daemon.restart");
    let state;
    for (let n = 0; n < 120; n++) {
      await sleep(500);
      state = await new Instance(dir).status().catch(() => null);
      if (state?.status === "ready" && state.backendPid !== before) break;
    }
    assert.equal(state?.status, "ready");
    assert.notEqual(state.backendPid, before);
    i = new Instance(dir);
    await i.attach();
    assert.equal((await rpc("daemon.status")).gateway.listening, true);
    // The scheduled cleanup starts 30 s after the backend does. Removing what is past the 400 days
    // shows the pass ran, and it must leave the rest alone.
    insertOld("fixture-expired", 500);
    await sleep(33000);
    const between = (from, to) =>
      rpc("stats.summary", { since_ms: Date.now() - from * 86400000, until_ms: Date.now() - to * 86400000, group_by: "model" });
    assert((await between(120, 90)).some((e) => e.key === "fixture-old"), "a 100-day-old request was deleted");
    assert(!(await between(520, 480)).some((e) => e.key === "fixture-expired"), "the scheduled cleanup did not run");
  });

  await check("stop-request-from-the-ui-stops-the-backend", async () => {
    await rpc("daemon.stop");
    for (let n = 0; n < 40 && (await i.status()).status !== "stopped"; n++) await sleep(250);
    assert.equal((await i.status()).status, "stopped");
  });
} catch (error) {
  result.failure = { name: error.name, message: String(error.message).slice(0, 500) };
  console.log("FAIL " + result.failure.message);
  process.exitCode = 1;
} finally {
  mock.close();
  if (i) await i.cleanup().catch(() => undefined);
  try {
    registry(AUTOSTART_KEY, "drop");
  } catch {}
  result.cleaned = true;
  fs.writeFileSync(path.join(root, "evidence/control.json"), JSON.stringify(result, null, 2));
  console.log(JSON.stringify({ passed: result.cases.length, failed: !!result.failure }));
  process.exit(process.exitCode ?? 0);
}
