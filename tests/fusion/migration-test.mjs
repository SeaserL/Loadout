// SPDX-License-Identifier: Apache-2.0
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import http from "node:http";
import { once } from "node:events";
import { randomUUID } from "node:crypto";
import { DatabaseSync } from "node:sqlite";
import {
  runs,
  snapshot,
  migrate,
  fingerprint,
  keytar,
  sha,
  load,
} from "./harness.mjs";
const dir = path.join(runs, "migration-" + Date.now());
fs.mkdirSync(dir);
const source = path.join(dir, "synthetic-source.db"),
  db = new DatabaseSync(source);
db.exec(`CREATE TABLE providers(id TEXT,slug TEXT,kind TEXT);
CREATE TABLE accounts(id TEXT,provider_id TEXT,enabled INTEGER,position INTEGER);
CREATE TABLE pools(id TEXT,name TEXT,ordering TEXT);
CREATE TABLE account_models(id TEXT,account_id TEXT,upstream_model_id TEXT,enabled INTEGER);
CREATE TABLE pool_members(pool_id TEXT,account_model_id TEXT,position INTEGER);
CREATE TABLE endpoints(name TEXT,provider_slug TEXT,members_json TEXT,wait_secs INTEGER,rotate INTEGER,default_model TEXT,enabled INTEGER);
CREATE TABLE clients(policy_json TEXT,enabled INTEGER,revoked_at TEXT);
CREATE TABLE account_sets(id TEXT);
INSERT INTO providers VALUES('p','synthetic','openai-compatible');
INSERT INTO accounts VALUES('a','p',1,0),('b','p',1,1);
INSERT INTO pools VALUES('pool','old-pool','manual');
INSERT INTO account_models VALUES('ma','a','same-model',1),('mb','b','same-model',1);
INSERT INTO pool_members VALUES('pool','ma',0),('pool','mb',1);
INSERT INTO endpoints VALUES('old-endpoint','synthetic',NULL,30,0,NULL,1);
INSERT INTO clients VALUES('{}',1,NULL);`);
db.close();
const originalHash = sha(fs.readFileSync(source));
const cases = [],
  instances = [];
const check = async (name, fn) => {
  try {
    await fn();
    cases.push({ name, pass: true });
  } catch (e) {
    cases.push({ name, pass: false, error: e.message.slice(0, 100) });
  }
  console.log(JSON.stringify(cases.at(-1)));
};
const calls = [];
const mock = http.createServer(async (req, res) => {
  res.setHeader("content-type", "application/json");
  if (req.method === "GET") {
    res.end('{"data":[{"id":"same-model"}]}');
    return;
  }
  const chunks = [];
  for await (const chunk of req) chunks.push(chunk);
  const body = JSON.parse(Buffer.concat(chunks));
  calls.push(body.model);
  res.end(
    JSON.stringify({
      id: "mock",
      object: "chat.completion",
      model: body.model,
      choices: [
        {
          index: 0,
          message: { role: "assistant", content: "OK" },
          finish_reason: "stop",
        },
      ],
      usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 },
    }),
  );
});
mock.listen(0, "127.0.0.1");
await once(mock, "listening");
const state = snapshot(source);
const input = {
  sourceFingerprint: fingerprint(state),
  endpointModels: { "old-endpoint": ["same-model"] },
  providers: [
    {
      slug: "synthetic",
      traffic: "native-default",
      baseUrl: `http://127.0.0.1:${mock.address().port}/v1`,
      accounts: state.accounts.map((a, i) => ({
        ref: a.ref,
        alias: "a" + i,
        key: "synthetic-" + randomUUID(),
        models: ["same-model"],
      })),
    },
  ],
};
try {
  await check("traffic-policy-must-be-explicit", async () => {
    const bad = structuredClone(input);
    delete bad.providers[0].traffic;
    const target = path.join(dir, "missing-traffic");
    await assert.rejects(
      migrate(source, target, bad),
      /EXPLICIT_TRAFFIC_POLICY_REQUIRED/,
    );
    assert(!fs.existsSync(target));
  });
  await check("preview-only-does-not-modify-source", async () => {
    assert.equal(state.accounts.length, 2);
    assert.equal(state.pools[0].members.length, 2);
    assert.equal(sha(fs.readFileSync(source)), originalHash);
  });
  await check("missing-member-refuses-before-target", async () => {
    const target = path.join(dir, "missing"),
      bad = structuredClone(input);
    bad.providers[0].accounts.pop();
    await assert.rejects(migrate(source, target, bad), /INCOMPLETE/);
    assert(!fs.existsSync(target));
  });
  await check("stale-source-refuses-before-target", async () => {
    const target = path.join(dir, "stale");
    await assert.rejects(
      migrate(source, target, { ...input, sourceFingerprint: "wrong" }),
      /STALE_SOURCE/,
    );
    assert(!fs.existsSync(target));
  });
  await check("complete-private-input-migration", async () => {
    const migrated = await migrate(source, path.join(dir, "target"), input);
    instances.push(migrated.instance);
    assert.deepEqual(migrated.routes, [
      "pool-old-pool",
      "endpoint-old-endpoint-1",
    ]);
    const instance = migrated.instance;
    await instance.start();
    for (const model of migrated.routes) {
      const key = await keytar.getPassword(
        instance.meta.bootstrap,
        "route-" + model,
      );
      const request = (m) =>
        fetch(instance.base + "/v1/chat/completions", {
          method: "POST",
          headers: {
            authorization: "Bearer " + key,
            "content-type": "application/json",
          },
          body: JSON.stringify({
            model: m,
            messages: [{ role: "user", content: "synthetic" }],
          }),
        });
      const r = await request(model);
      await r.text();
      assert.equal(r.status, 200);
      assert.equal(calls.at(-1), "same-model");
      const n = calls.length,
        bypass = await request("other-model");
      await bypass.text();
      assert.notEqual(bypass.status, 200);
      assert.equal(calls.length, n);
    }
    assert.equal(sha(fs.readFileSync(source)), originalHash);
    const report = fs.readFileSync(
      path.join(instance.dir, "migration.json"),
      "utf8",
    );
    for (const account of input.providers[0].accounts)
      assert(!report.includes(account.key));
  });
  await check("unknown-source-policy-explicit-blocker", async () => {
    const { compile } = await load("mapping.mjs");
    assert.throws(
      () =>
        compile(
          { ...state, clientPolicies: [{ unknown: true }] },
          {},
          {},
          "native-bounded-v1",
        ),
      /UNMAPPED_ACCESS_POLICY/,
    );
    assert.throws(
      () =>
        compile({ ...state, accountSetCount: 1 }, {}, {}, "native-bounded-v1"),
      /UNMAPPED_ACCESS_POLICY/,
    );
  });
} finally {
  for (const instance of instances)
    await check("cleanup", async () => {
      assert.equal(await instance.cleanup(), 0);
    });
  mock.closeAllConnections();
  await new Promise((resolve) => mock.close(resolve));
  fs.writeFileSync(
    path.join(dir, "result.json"),
    JSON.stringify({ cases }, null, 2),
  );
  console.log("RESULT " + path.join(dir, "result.json"));
}
process.exitCode = cases.some((c) => !c.pass) ? 1 : 0;
