// SPDX-License-Identifier: Apache-2.0
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import http from "node:http";
import { once } from "node:events";
import { randomUUID } from "node:crypto";
import { Instance, runs } from "./harness.mjs";
const dir = path.join(runs, "account-rules-" + Date.now());
fs.mkdirSync(dir);
const cases = [],
  tokens = new Map(),
  calls = [];
let instance,
  failing = null;
const check = async (name, fn) => {
  try {
    await fn();
    cases.push({ name, pass: true });
  } catch (e) {
    cases.push({ name, pass: false, error: e.message.slice(0, 100) });
  }
  console.log(JSON.stringify(cases.at(-1)));
};
const mock = http.createServer(async (req, res) => {
  if (req.method !== "POST") {
    res.setHeader("content-type", "application/json");
    res.end('{"data":[{"id":"same-model"}]}');
    return;
  }
  const chunks = [];
  for await (const c of req) chunks.push(c);
  const body = JSON.parse(Buffer.concat(chunks));
  const account = tokens.get(
    String(req.headers.authorization).replace("Bearer ", ""),
  );
  calls.push({ account, model: body.model });
  if (account === failing) {
    res.writeHead(429, { "content-type": "application/json" });
    res.end('{"error":{"message":"synthetic rate limit"}}');
    return;
  }
  res.setHeader("content-type", "application/json");
  res.end(
    JSON.stringify({
      id: "mock",
      object: "chat.completion",
      model: body.model,
      choices: [
        {
          index: 0,
          message: { role: "assistant", content: account },
          finish_reason: "stop",
        },
      ],
      usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 },
    }),
  );
});
mock.listen(0, "127.0.0.1");
await once(mock, "listening");
const accounts = (suffix) =>
  ["a", "b", "c"].map((alias) => {
    const key = "synthetic-" + randomUUID();
    tokens.set(key, alias + suffix);
    return { alias, key };
  });
const send = (p, protocol, model = p.provider + "/same-model", extra = {}) =>
  fetch(instance.base + "/v1/" + protocol, {
    method: "POST",
    headers: {
      authorization: "Bearer " + p.key,
      "content-type": "application/json",
      "anthropic-version": "2023-06-01",
    },
    body: JSON.stringify(
      protocol === "responses"
        ? { model, input: "test", store: false, ...extra }
        : {
            model,
            messages: [{ role: "user", content: "test" }],
            max_tokens: 30,
            ...extra,
          },
    ),
    signal: AbortSignal.timeout(10000),
  });
try {
  instance = await Instance.create(path.join(dir, "instance"));
  await instance.start();
  await instance.admin("/api/resilience", "PATCH", {
    requestQueue: { minTimeBetweenRequestsMs: 0 },
    waitForCooldown: { enabled: false },
    comboCooldownWait: { enabled: false },
  });
  for (const [index, protocol] of [
    "chat/completions",
    "responses",
    "messages",
  ].entries()) {
    const p = await instance.addProvider(
      "rotation-" + index,
      `http://127.0.0.1:${mock.address().port}/v1`,
      accounts(String(index)),
    );
    await check(protocol + "-native-provider-rotation", async () => {
      await instance.admin("/api/settings", "PATCH", {
        providerStrategies: {
          [p.provider]: {
            fallbackStrategy: "round-robin",
            stickyRoundRobinLimit: 1,
          },
        },
      });
      const n = calls.length;
      for (let i = 0; i < 4; i++) {
        const r = await send(p, protocol);
        await r.text();
        assert.equal(r.status, 200);
      }
      assert.deepEqual(
        calls.slice(n).map((c) => c.account),
        ["a", "b", "c", "a"].map((a) => a + index),
      );
      assert(calls.slice(n).every((c) => c.model === "same-model"));
    });
    await check(protocol + "-named-account-overrides-rotation", async () => {
      const n = calls.length;
      for (let i = 0; i < 3; i++) {
        const r = await send(p, protocol, p.model("b", "same-model"));
        await r.text();
        assert.equal(r.status, 200);
      }
      assert(calls.slice(n).every((c) => c.account === "b" + index));
    });
    await check(protocol + "-rotation-subset-and-disable", async () => {
      await instance.admin("/api/keys/" + p.keyId, "PATCH", {
        allowedConnections: p.connections.slice(1),
      });
      const n = calls.length;
      for (let i = 0; i < 4; i++) {
        const r = await send(p, protocol);
        await r.text();
        assert.equal(r.status, 200);
      }
      assert(
        calls
          .slice(n)
          .every((c) => ["b" + index, "c" + index].includes(c.account)),
      );
      assert.equal(new Set(calls.slice(n).map((c) => c.account)).size, 2);
      await instance.admin("/api/settings", "PATCH", {
        providerStrategies: {
          [p.provider]: { fallbackStrategy: "fill-first" },
        },
      });
      const r = await send(p, protocol);
      await r.text();
      assert.equal(r.status, 200);
      assert.equal(calls.at(-1).account, "b" + index);
    });
  }
  await check("preferred-account-fails-over-within-same-model", async () => {
    const p = await instance.addProvider(
      "preferred",
      `http://127.0.0.1:${mock.address().port}/v1`,
      accounts("preferred"),
    );
    failing = "bpreferred";
    const n = calls.length;
    const r = await send(p, "chat/completions", p.model("b", "same-model"));
    await r.text();
    assert.equal(r.status, 200);
    assert.equal(calls[n].account, "bpreferred");
    assert.notEqual(calls.at(-1).account, "bpreferred");
    assert(calls.slice(n).every((c) => c.model === "same-model"));
    failing = null;
  });
  await check("strict-account-never-fails-over", async () => {
    const p = await instance.addProvider(
      "strict",
      `http://127.0.0.1:${mock.address().port}/v1`,
      accounts("strict"),
    );
    failing = "bstrict";
    const n = calls.length;
    const r = await send(p, "chat/completions", p.model("b", "same-model"), {
      allow_account_failover: false,
    });
    await r.text();
    assert.notEqual(r.status, 200);
    assert(calls.length > n);
    assert(calls.slice(n).every((c) => c.account === "bstrict"));
    failing = null;
  });
} finally {
  if (instance)
    await check("cleanup", async () =>
      assert.equal(await instance.cleanup(), 0),
    );
  mock.closeAllConnections();
  await new Promise((r) => mock.close(r));
  fs.writeFileSync(
    path.join(dir, "result.json"),
    JSON.stringify({ cases }, null, 2),
  );
  console.log("RESULT " + path.join(dir, "result.json"));
}
process.exitCode = cases.some((c) => !c.pass) ? 1 : 0;
