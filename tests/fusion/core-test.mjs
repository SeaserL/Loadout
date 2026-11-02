// SPDX-License-Identifier: Apache-2.0
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import http from "node:http";
import { once } from "node:events";
import { spawn, spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { DatabaseSync } from "node:sqlite";
import {
  Instance,
  keytar,
  backup,
  restore,
  pkg,
  runs,
  sleep,
  baseline,
  verifyPackage,
  snapshot,
  migrate,
  fingerprint,
} from "./harness.mjs";
const dir = path.join(runs, "core-" + Date.now());
fs.mkdirSync(dir);
const result = { baseline: baseline(), cases: [], benchmarks: {} },
  instances = [],
  secrets = [],
  calls = [];
let instance,
  profile,
  mode = "normal",
  closed = 0;
const sentinel = "BODY_" + randomUUID(),
  tokens = new Map();
const check = async (name, action) => {
  try {
    const detail = await action();
    result.cases.push({ name, pass: true, ...detail });
  } catch (error) {
    result.cases.push({
      name,
      pass: false,
      error: error.message.slice(0, 100),
      ...(error.code === "ENOENT" ? { file: error.path, syscall: error.syscall } : {}),
    });
  }
  fs.writeFileSync(
    path.join(dir, "result.json"),
    JSON.stringify(result, null, 2),
  );
  console.log(JSON.stringify(result.cases.at(-1)));
};
const mock = http.createServer(async (req, res) => {
  if (req.method !== "POST") {
    res.setHeader("content-type", "application/json");
    res.end(JSON.stringify({ data: [{ id: "same-model", object: "model" }] }));
    return;
  }
  const chunks = [];
  for await (const c of req) chunks.push(c);
  const body = JSON.parse(Buffer.concat(chunks));
  const account = tokens.get(
    String(req.headers.authorization).replace("Bearer ", ""),
  );
  calls.push({ account, model: body.model });
  if (!account) {
    res.writeHead(401);
    res.end();
    return;
  }
  if (mode === "fail-b" && account === "b") {
    res.writeHead(429, { "content-type": "application/json" });
    res.end('{"error":{"message":"rate limit"}}');
    return;
  }
  const tool = body.tools?.length;
  if (!body.stream) {
    res.setHeader("content-type", "application/json");
    res.end(
      JSON.stringify({
        id: "chat-mock",
        object: "chat.completion",
        model: body.model,
        choices: [
          {
            index: 0,
            message: tool
              ? {
                  role: "assistant",
                  content: null,
                  tool_calls: [
                    {
                      id: "call_a",
                      type: "function",
                      function: {
                        name: "weather",
                        arguments: '{"city":"Paris"}',
                      },
                    },
                  ],
                }
              : { role: "assistant", content: account },
            finish_reason: tool ? "tool_calls" : "stop",
          },
        ],
        usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 },
      }),
    );
    return;
  }
  res.writeHead(200, { "content-type": "text/event-stream" });
  const send = (delta, finish_reason = null) =>
    res.write(
      "data: " +
        JSON.stringify({
          id: "chat-mock",
          object: "chat.completion.chunk",
          model: body.model,
          choices: [{ index: 0, delta, finish_reason }],
        }) +
        "\n\n",
    );
  send({ role: "assistant", ...(tool ? {} : { content: account }) });
  if (mode === "pending" || mode === "partial") {
    const timer = setTimeout(
      () => res.destroy(),
      mode === "partial" ? 100 : 10000,
    );
    res.on("close", () => {
      closed++;
      clearTimeout(timer);
    });
    return;
  }
  if (tool) {
    send({
      tool_calls: [
        {
          index: 0,
          id: "call_a",
          type: "function",
          function: { name: "weather", arguments: '{"city":' },
        },
      ],
    });
    send({ tool_calls: [{ index: 0, function: { arguments: '"Paris"}' } }] });
  }
  send({}, tool ? "tool_calls" : "stop");
  res.end("data: [DONE]\n\n");
});
mock.listen(0, "127.0.0.1");
await once(mock, "listening");
async function cli(...args) {
  const child = spawn(
    path.join(pkg, "node.exe"),
    [path.join(pkg, "tools/cli.mjs"), ...args],
    { cwd: dir, stdio: ["ignore", "pipe", "pipe"], windowsHide: true },
  );
  let out = "",
    err = "";
  child.stdout.on("data", (c) => (out += c));
  child.stderr.on("data", (c) => (err += c));
  const [code] = await once(child, "exit");
  if (code) throw Error("CLI_FAILED_" + err.slice(0, 60));
  return JSON.parse(out);
}
async function query(
  protocol = "chat/completions",
  extra = {},
  key = profile.key,
) {
  const model = profile.provider + "/same-model";
  const body =
    protocol === "responses"
      ? { model, input: sentinel, store: false, ...extra }
      : {
          model,
          messages: [{ role: "user", content: sentinel }],
          max_tokens: 30,
          ...extra,
        };
  return fetch(instance.base + "/v1/" + protocol, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      authorization: "Bearer " + key,
      "anthropic-version": "2023-06-01",
    },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(15000),
  });
}
async function scope(ids) {
  await instance.admin(
    "/api/keys/" + profile.keyId,
    "PATCH",
    ids.length
      ? { allowedConnections: ids, isActive: true }
      : { isActive: false },
  );
}
try {
  verifyPackage();
  await check("raw-backend-entry-refuses-default-environment", async () => {
    const r = spawnSync(
      path.join(pkg, "node.exe"),
      [path.join(pkg, "server.js")],
      {
        cwd: dir,
        env: { SystemRoot: process.env.SystemRoot },
        windowsHide: true,
        encoding: "utf8",
        timeout: 5000,
      },
    );
    assert.equal(r.status, 64);
    assert(r.stderr.includes("USE_FUSION_LAUNCHER"));
  });
  await check("refuse-nonisolated-target", async () => {
    await assert.rejects(Instance.create(path.join(dir, "../../unsafe")));
  });
  instance = await Instance.create(path.join(dir, "main"));
  instances.push(instance);
  await check("cli-start-survives-launcher-exit", async () => {
    const s = await cli("start", instance.dir);
    assert.equal(s.status, "ready");
    await sleep(400);
    await instance.attach();
    assert.equal((await instance.status()).status, "ready");
  });
  await check("duplicate-start-refused", async () => {
    await assert.rejects(instance.start(), /INSTANCE_ALREADY_ACTIVE/);
  });
  await check("occupied-port-does-not-take-over-other-instance", async () => {
    const other = await Instance.create(
      path.join(dir, "port-conflict"),
      undefined,
      { network: "loopback-only", port: Number(new URL(instance.base).port) },
    );
    instances.push(other);
    await assert.rejects(other.start());
    assert.equal((await instance.status()).status, "ready");
  });
  await check("owned-supervisor-crash-and-explicit-restart", async () => {
    const owned = await instance.control("status");
    assert.equal(owned.instance, instance.meta.id);
    process.kill(owned.pid);
    for (
      let i = 0;
      i < 40 && (await instance.status()).status !== "stopped";
      i++
    )
      await sleep(200);
    assert.equal((await instance.status()).status, "stopped");
    await instance.start();
    assert.equal((await instance.status()).status, "ready");
  });
  await check("safe-defaults-no-experimental-flags", async () => {
    const s = await instance.settings();
    assert.equal(s.requireLogin, true);
    assert.equal(s.requestRetry, 0);
    assert.equal(s.debugMode, false);
  });
  await check("unauthenticated-admin-and-inference-denied", async () => {
    const r = await fetch(instance.base + "/api/providers", {
      redirect: "manual",
    });
    assert.notEqual(r.status, 200);
    const q = await fetch(instance.base + "/v1/chat/completions", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ model: "test", messages: [] }),
    });
    assert.notEqual(q.status, 200);
    assert.equal(calls.length, 0);
  });
  await check("lab-and-unreviewed-control-routes-absent", async () => {
    for (const p of [
      "/api/loadout-lab",
      "/api/settings/export-json",
      "/api/keys/x/reveal",
      "/api/mcp/servers",
    ]) {
      const r = await fetch(instance.base + p, {
        headers: { cookie: instance.cookie },
        redirect: "manual",
      });
      assert.equal(r.status, 404, p);
    }
  });
  await check("safe-settings-cannot-be-disabled", async () => {
    await instance.admin("/api/settings", "PATCH", {
      requestLoggingEnabled: true,
      debugMode: true,
    });
    const s = await instance.settings();
    assert.equal(s.requestLoggingEnabled, false);
    assert.equal(s.debugMode, false);
  });
  await instance.admin("/api/resilience", "PATCH", {
    requestQueue: { minTimeBetweenRequestsMs: 0 },
    waitForCooldown: { enabled: false },
    comboCooldownWait: { enabled: false },
  });
  const accounts = ["a", "b", "c"].map((alias) => {
    const key = "synthetic-" + randomUUID();
    tokens.set(key, alias);
    secrets.push(key);
    return { alias, key };
  });
  await check("normal-management-provider-create", async () => {
    profile = await instance.addProvider(
      "core",
      `http://127.0.0.1:${mock.address().port}/v1`,
      accounts,
    );
    secrets.push(profile.key);
  });
  for (const protocol of ["chat/completions", "responses", "messages"]) {
    await check(protocol + "-ordered", async () => {
      const offset = calls.length;
      const r = await query(protocol);
      assert.equal(r.status, 200);
      const b = await r.json();
      assert(b);
      assert.equal(calls[offset].account, "a");
    });
    await check(protocol + "-account-scope", async () => {
      await scope([profile.connections[2]]);
      const offset = calls.length;
      const r = await query(protocol);
      await r.text();
      assert.equal(r.status, 200);
      assert(
        calls
          .slice(offset)
          .every((c) => c.account === "c" && c.model === "same-model"),
      );
      await scope(profile.connections);
    });
    await check(protocol + "-stream-tools", async () => {
      const tools =
        protocol === "messages"
          ? [
              {
                name: "weather",
                input_schema: {
                  type: "object",
                  properties: { city: { type: "string" } },
                },
              },
            ]
          : protocol === "responses"
            ? [
                {
                  type: "function",
                  name: "weather",
                  parameters: { type: "object" },
                },
              ]
            : [
                {
                  type: "function",
                  function: { name: "weather", parameters: { type: "object" } },
                },
              ];
      const r = await query(protocol, { stream: true, tools });
      const text = await r.text();
      assert.equal(r.status, 200);
      assert(text.includes("weather") && text.includes("Paris"));
    });
  }
  await check("empty-subset-disabled-and-no-dispatch", async () => {
    await scope([]);
    const n = calls.length,
      r = await query();
    await r.text();
    assert.notEqual(r.status, 200);
    assert.equal(calls.length, n);
    await scope(profile.connections);
  });
  await check("stateful-responses-explicitly-denied", async () => {
    const n = calls.length,
      r = await query("responses", { previous_response_id: "resp_old" });
    await r.text();
    assert.equal(r.status, 400);
    assert.equal(calls.length, n);
  });
  await check("named-account-scope-cannot-escape", async () => {
    await scope([profile.connections[2]]);
    const n = calls.length,
      r = await query("chat/completions", {
        model: profile.model("b", "same-model"),
        allow_account_failover: false,
      });
    await r.text();
    assert.notEqual(r.status, 200);
    assert.equal(calls.length, n);
    await scope(profile.connections);
  });
  await check("partial-stream-not-replayed", async () => {
    mode = "partial";
    const n = calls.length,
      r = await query("chat/completions", { stream: true });
    const reader = r.body.getReader();
    assert.equal((await reader.read()).done, false);
    try {
      while (!(await reader.read()).done) {}
    } catch {}
    assert.equal(calls.length, n + 1);
    mode = "normal";
  });
  await check("cancel-closes-upstream", async () => {
    mode = "pending";
    const n = closed,
      r = await query("chat/completions", { stream: true });
    const reader = r.body.getReader();
    await reader.read();
    await reader.cancel();
    await sleep(600);
    assert(closed > n);
    mode = "normal";
  });
  await check("credential-update-prunes-old", async () => {
    const value = "synthetic-new-" + randomUUID();
    secrets.push(value);
    tokens.set(value, "a");
    await instance.admin("/api/providers/" + profile.connections[0], "PUT", {
      apiKey: value,
    });
    const entries = await keytar.findCredentials(instance.meta.service);
    assert(entries.some((e) => e.password === value));
    assert(!entries.some((e) => e.password === accounts[0].key));
  });
  await check("credential-delete-prunes-vault", async () => {
    await instance.admin("/api/providers/" + profile.connections[1], "DELETE");
    assert(
      !(await keytar.findCredentials(instance.meta.service)).some(
        (e) => e.password === accounts[1].key,
      ),
    );
  });
  await check("client-revoke-immediate", async () => {
    const key = await instance.admin("/api/keys", "POST", {
      name: "revoke",
      noLog: true,
      allowedConnections: profile.connections,
    });
    secrets.push(key.key);
    await instance.admin("/api/keys/" + key.id, "DELETE");
    const n = calls.length,
      r = await query("chat/completions", {}, key.key);
    await r.text();
    assert.notEqual(r.status, 200);
    assert.equal(calls.length, n);
  });
  await check("encrypted-vault-export-and-wrong-password", async () => {
    const pass = randomUUID();
    const data = await instance.admin("/api/loadout/vault", "POST", {
      operation: "export",
      password: pass,
    });
    assert(!JSON.stringify(data).includes(secrets[0]));
    await assert.rejects(
      instance.admin("/api/loadout/vault", "POST", {
        operation: "restore",
        password: randomUUID(),
        backup: data,
      }),
    );
  });
  await check("backend-restart-keeps-client-and-scope", async () => {
    await scope([profile.connections[2]]);
    const port = new URL(instance.base).port;
    await instance.stop();
    await instance.start();
    assert.equal(new URL(instance.base).port, port);
    const n = calls.length,
      r = await query();
    await r.text();
    assert.equal(r.status, 200);
    assert(calls.slice(n).every((c) => c.account === "c"));
  });
  await check("backup-running-refused", async () => {
    await assert.rejects(
      backup(instance, path.join(dir, "running.enc"), randomUUID()),
    );
  });
  const archive = path.join(dir, "backup.enc"),
    pass = randomUUID();
  await check("stopped-encrypted-backup", async () => {
    await instance.stop();
    await backup(instance, archive, pass);
  });
  await check("wrong-password-and-tamper-no-target", async () => {
    const destination = path.join(dir, "bad");
    await assert.rejects(restore(archive, destination, randomUUID()));
    assert(!fs.existsSync(destination));
    const data = JSON.parse(fs.readFileSync(archive));
    data.tag = Buffer.alloc(16).toString("base64");
    const tamper = path.join(dir, "tamper.enc");
    fs.writeFileSync(tamper, JSON.stringify(data));
    await assert.rejects(restore(tamper, destination, pass));
    assert(!fs.existsSync(destination));
  });
  await check("fresh-restore-original-client-key", async () => {
    const old = instance;
    instance = await restore(archive, path.join(dir, "restored"), pass);
    instances.push(instance);
    assert.notEqual(instance.meta.service, old.meta.service);
    assert.equal(
      instance.meta.config.port,
      JSON.parse(fs.readFileSync(path.join(old.dir, "instance.json"))).config
        .port,
    );
    await instance.start();
    const n = calls.length,
      r = await query();
    await r.text();
    assert.equal(r.status, 200);
    assert(calls.slice(n).every((c) => c.account === "c"));
  });
  await check("restore-no-overwrite", async () => {
    await assert.rejects(restore(archive, instance.dir, pass));
  });
  await check("missing-vault-fails-closed", async () => {
    await instance.stop();
    for (const e of await keytar.findCredentials(instance.meta.service))
      await keytar.deletePassword(instance.meta.service, e.account);
    await instance.start();
    const n = calls.length,
      r = await query();
    await r.text();
    assert.notEqual(r.status, 200);
    assert.equal(calls.length, n);
    await instance.stop();
  });
  await check("rollback-original-copy-still-works", async () => {
    instance = instances[0];
    await instance.start();
    const r = await query();
    await r.text();
    assert.equal(r.status, 200);
  });
  await check("mock-latency-benchmark", async () => {
    const samples = [];
    for (let i = 0; i < 20; i++) {
      const start = performance.now(),
        r = await query();
      await r.text();
      assert.equal(r.status, 200);
      samples.push(performance.now() - start);
    }
    samples.sort((a, b) => a - b);
    result.benchmarks = {
      iterations: samples.length,
      p50Ms: samples[10],
      p95Ms: samples[18],
      interpretation: "full-local-HTTP-mock-not-upstream-latency",
    };
  });
  await check("plain-instance-files-no-secrets-or-body", async () => {
    await instance.stop();
    let scanned = 0;
    function walk(d) {
      for (const e of fs.readdirSync(d, { withFileTypes: true })) {
        const p = path.join(d, e.name);
        if (e.isDirectory()) walk(p);
        else {
          const bytes = fs.readFileSync(p);
          scanned++;
          for (const secret of [...secrets, sentinel]) {
            assert(!bytes.includes(Buffer.from(secret)));
            assert(!bytes.includes(Buffer.from(secret, "utf16le")));
          }
        }
      }
    }
    for (const i of instances) walk(i.dir);
    return { scanned };
  });
  await check("final-artifact-unchanged", async () => {
    verifyPackage();
  });
} catch (error) {
  result.fatal = error.message;
} finally {
  for (const i of instances)
    await check("cleanup-" + i.meta.id, async () => {
      assert.equal(await i.cleanup(), 0);
      assert.equal((await i.status()).status, "stopped");
    });
  mock.closeAllConnections();
  await new Promise((resolve) => mock.close(resolve));
  fs.writeFileSync(
    path.join(dir, "result.json"),
    JSON.stringify(result, null, 2),
  );
  console.log("RESULT " + path.join(dir, "result.json"));
}
process.exitCode = result.fatal || result.cases.some((c) => !c.pass) ? 1 : 0;
