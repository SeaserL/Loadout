// SPDX-License-Identifier: Apache-2.0
import fs from "node:fs";
import path from "node:path";
import http from "node:http";
import assert from "node:assert/strict";
import { once } from "node:events";
import { DatabaseSync } from "node:sqlite";
import { Instance, sleep, baseline, pkg } from "./harness.mjs";
const root = path.resolve(process.env.FUSION_RELEASE_ROOT);
const dir = path.join(
  root,
  "runs/loadout-fusion-isolated/console-064-" + Date.now(),
);
const result = {
  baseline: baseline(),
  mode: process.argv.includes("--ui-only")
    ? "development-ui-setup"
    : "regression",
  cases: [],
};
let i,
  calls = 0;
const sentinels = [
  "synthetic-console-secret-one",
  "synthetic-console-secret-two",
];
const check = async (name, fn) => {
  await fn();
  result.cases.push({ name, pass: true });
  console.log("PASS " + name);
  fs.writeFileSync(
    path.join(root, "evidence/console.json"),
    JSON.stringify(result, null, 2),
  );
};
const mock = http.createServer(async (req, res) => {
  let raw = "";
  for await (const c of req) raw += c;
  calls++;
  res.setHeader("content-type", "application/json");
  if (req.method !== "POST")
    return res.end(
      JSON.stringify({
        data: [{ id: "fixture-chat" }, { id: "fixture-reasoner" }],
      }),
    );
  const body = JSON.parse(raw);
  res.end(
    JSON.stringify({
      id: "mock-" + calls,
      object: "chat.completion",
      model: body.model,
      choices: [
        {
          index: 0,
          message: { role: "assistant", content: "synthetic reply" },
          finish_reason: "stop",
        },
      ],
      usage: {
        prompt_tokens: 1200 + calls * 100,
        completion_tokens: 400 + calls * 20,
        total_tokens: 1600 + calls * 120,
      },
    }),
  );
});
mock.listen(0, "127.0.0.1");
await once(mock, "listening");
const probes = () => {
  const db = new DatabaseSync(path.join(i.dir, "data/storage.sqlite"), {
    readOnly: true,
  });
  try {
    return db
      .prepare(
        "SELECT count(*) AS n FROM call_logs WHERE model='connection-test'",
      )
      .get().n;
  } finally {
    db.close();
  }
};
try {
  i = await Instance.create(dir, {
    password: "Loadout-console-local-fixture-064!",
    api: crypto.randomUUID(),
    jwt: crypto.randomUUID(),
    control: crypto.randomUUID(),
    bridge: crypto.randomUUID(),
  });
  await i.start();
  await check("same-origin-static-console", async () => {
    const r = await fetch(i.base + "/loadout/index.html");
    assert.equal(r.status, 200);
    const html = await r.text();
    assert(html.includes("Content-Security-Policy"));
    assert(html.includes("<title>Loadout"));
    for (const asset of [
      ...html.matchAll(/(?:src|href)="(\/loadout\/assets\/[^\"]+)"/g),
    ])
      assert.equal((await fetch(i.base + asset[1])).status, 200);
  });
  const a = await i.addProvider(
    "Fixture North",
    `http://127.0.0.1:${mock.address().port}/v1`,
    [
      { alias: "Development", key: sentinels[0] },
      { alias: "Review", key: sentinels[1] },
    ],
  );
  await check("admin-session-and-csrf-remain-required", async () => {
    assert.equal((await fetch(i.base + "/api/providers")).status, 401);
    assert.equal(
      (
        await fetch(i.base + "/api/providers/" + a.connections[0], {
          method: "PUT",
          headers: {
            cookie: i.cookie,
            "content-type": "application/json",
            origin: i.base,
          },
          body: JSON.stringify({ name: "denied" }),
        })
      ).status,
      403,
    );
  });
  if (!process.argv.includes("--ui-only"))
    await check("new-accounts-no-startup-probes", async () => {
      const n = calls,
        p = probes();
      await sleep(35000);
      assert.equal(calls, n);
      assert.equal(probes(), p);
      assert.equal(p, 0);
    });
  if (!process.argv.includes("--ui-only"))
    await check("restart-no-probes-with-default-account-interval", async () => {
      await i.stop();
      await i.start();
      const n = calls;
      await sleep(35000);
      assert.equal(calls, n);
      assert.equal(probes(), 0);
    });
  await check("credential-edit-keeps-scope-and-redaction", async () => {
    const before = await i.admin("/api/providers");
    await i.admin("/api/providers/" + a.connections[0], "PUT", {
      name: "Development",
      apiKey: "synthetic-console-rotated",
    });
    const after = await i.admin("/api/providers");
    assert.deepEqual(
      after.connections.map((a) => [a.id, a.provider, a.isActive]),
      before.connections.map((a) => [a.id, a.provider, a.isActive]),
    );
    assert(!JSON.stringify(after).includes("synthetic-console-rotated"));
  });
  const models = ["fixture-chat", "fixture-reasoner"];
  for (const [index, model] of models.entries())
    await i.admin("/api/combos", "POST", {
      name: "fixture/" + model,
      strategy: "priority",
      models: [
        {
          kind: "model",
          provider: a.provider,
          model,
          connectionId: a.connections[index],
        },
      ],
    });
  const configured = await i.admin("/api/combos");
  const exact = configured.combos.find(
    (c) => c.name === "fixture/fixture-chat",
  );
  await i.admin("/api/model-combo-mappings", "POST", {
    pattern: "^legacy/exact-chat$",
    comboId: exact.id,
    enabled: true,
  });
  for (let index = 0; index < 6; index++) {
    const r = await fetch(i.base + "/v1/chat/completions", {
      method: "POST",
      headers: {
        "content-type": "application/json",
        authorization: "Bearer " + a.key,
      },
      body: JSON.stringify({
        model: "fixture/" + models[index % 2],
        messages: [{ role: "user", content: "synthetic request" }],
      }),
    });
    assert.equal(r.status, 200);
    await r.text();
  }
  await check("native-stats-and-console-contracts", async () => {
    await sleep(4000);
    const u = await i.admin("/api/usage/history");
    assert.equal(u.totalRequests, 6);
    assert.equal(Object.values(u.byAccount).length, 2);
    for (const p of [
      "/api/keys",
      "/api/combos",
      "/api/model-combo-mappings",
      "/api/models",
      "/api/free-models",
      "/api/radar/catalog",
      "/api/radar/status",
      "/api/pricing/sync",
    ]) {
      const v = await i.admin(p);
      result.contracts ??= {};
      result.contracts[p] = {
        keys: Object.keys(v),
        sampleKeys: Object.keys(
          (v.models ??
            v.connections ??
            v.keys ??
            v.combos ??
            v.entries ??
            [])[0] ?? {},
        ),
      };
    }
  });
  result.url = i.base + "/loadout/index.html";
  result.instance = i.dir;
  result.readyForUi = true;
  fs.writeFileSync(
    path.join(root, "evidence/console.json"),
    JSON.stringify(result, null, 2),
  );
  console.log("UI_READY " + result.url);
  if (process.argv.includes("--hold-for-ui"))
    while (!fs.existsSync(path.join(root, "finish-ui"))) await sleep(1000);
  if (!process.argv.includes("--ui-only"))
    await check("readonly-ui-and-idle-did-not-probe", async () =>
      assert.equal(probes(), 0),
    );
} catch (e) {
  result.failure = e.code ?? "CONSOLE_TEST_FAILED";
  result.location = e.stack?.match(/console-test.mjs:\d+/)?.[0];
  console.log(
    JSON.stringify({ failure: result.failure, location: result.location }),
  );
} finally {
  if (i) {
    await i.cleanup();
    result.cleaned = true;
  }
  mock.closeAllConnections();
  await new Promise((r) => mock.close(r));
  fs.writeFileSync(
    path.join(root, "evidence/console.json"),
    JSON.stringify(result, null, 2),
  );
}
process.exitCode = result.failure ? 1 : 0;
