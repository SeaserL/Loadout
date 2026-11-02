// SPDX-License-Identifier: Apache-2.0
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import http from "node:http";
import { randomUUID } from "node:crypto";
import { Instance, runs, load } from "./harness.mjs";
const { freePort } = await load("runtime.mjs");
const dir = path.join(runs, "management-" + Date.now());
fs.mkdirSync(dir);
const cases = [];
let instance;
const check = async (name, action) => {
  try {
    await action();
    cases.push({ name, pass: true });
  } catch (e) {
    cases.push({ name, pass: false, error: e.message.slice(0, 100) });
  }
  console.log(JSON.stringify(cases.at(-1)));
};
try {
  const wb = {
    authPath: path.join(dir, "missing-encrypted-session.json"),
    exe: path.join(dir, "unused-official.exe"),
    accountHash: "1".repeat(64),
    clientVersion: "1.0.0",
    cliVersion: "1.0.0",
    port: await freePort(),
  };
  instance = await Instance.create(path.join(dir, "instance"), undefined, {
    network: "loopback-only",
    port: 0,
    workbuddy: wb,
  });
  await check("normal-supervisor-hosts-packaged-workbuddy", async () => {
    await instance.start();
    const s = await instance.secrets();
    const r = await fetch(`http://127.0.0.1:${wb.port}/v1/models`, {
      headers: { authorization: "Bearer " + s.bridge },
    });
    assert.equal(r.status, 200);
    assert((await r.json()).data.some((m) => m.id === "deepseek-v4.1-flash"));
  });
  await check(
    "workbuddy-missing-source-fails-without-launching-client",
    async () => {
      const s = await instance.secrets();
      const r = await fetch(`http://127.0.0.1:${wb.port}/v1/chat/completions`, {
        method: "POST",
        headers: {
          authorization: "Bearer " + s.bridge,
          "content-type": "application/json",
        },
        body: JSON.stringify({
          model: "deepseek-v4.1-flash",
          messages: [{ role: "system", content: "synthetic" }],
        }),
      });
      assert.equal(r.status, 502);
      assert((await r.text()).includes("AUTH_SOURCE_INVALID"));
      assert(!fs.existsSync(wb.authPath));
      assert(!fs.existsSync(wb.exe));
    },
  );
  await check("management-foreign-origin-denied", async () => {
    const before = await instance.admin("/api/keys");
    const r = await fetch(instance.base + "/api/keys", {
      method: "POST",
      headers: {
        cookie: instance.cookie,
        origin: "https://invalid.example",
        "x-omniroute-csrf": instance.csrf,
        "content-type": "application/json",
      },
      body: JSON.stringify({ name: "must-not-create" }),
    });
    assert.equal(r.status, 403);
    assert.deepEqual(await instance.admin("/api/keys"), before);
  });
  await check("management-csrf-missing-denied", async () => {
    const r = await fetch(instance.base + "/api/keys", {
      method: "POST",
      headers: {
        cookie: instance.cookie,
        origin: instance.base,
        "content-type": "application/json",
      },
      body: JSON.stringify({ name: "must-not-create" }),
    });
    assert.equal(r.status, 403);
  });
  await check("management-foreign-host-denied", async () => {
    // Node fetch replaces Host with the URL authority; use raw HTTP to send it.
    const status = await new Promise((resolve, reject) => {
      const request = http.get(
        instance.base + "/api/keys",
        {
          headers: { cookie: instance.cookie, host: "invalid.example" },
        },
        (response) => {
          response.resume();
          response.on("end", () => resolve(response.statusCode));
        },
      );
      request.on("error", reject);
    });
    assert.equal(status, 403);
  });
  await check("control-identity-auth-required", async () => {
    const state = JSON.parse(
      fs.readFileSync(path.join(instance.dir, "running.json")),
    );
    const r = await fetch(`http://127.0.0.1:${state.controlPort}/stop`, {
      method: "POST",
    });
    assert.equal(r.status, 403);
    assert.equal((await instance.status()).status, "ready");
  });
  await check("unknown-auth-type-rejected", async () => {
    await assert.rejects(
      instance.admin("/api/providers", "POST", {
        provider: "openai",
        name: "unsupported",
        authType: "oauth",
        accessToken: "synthetic-" + randomUUID(),
      }),
    );
  });
} finally {
  if (instance)
    await check("cleanup", async () => {
      assert.equal(await instance.cleanup(), 0);
    });
  fs.writeFileSync(
    path.join(dir, "result.json"),
    JSON.stringify({ cases }, null, 2),
  );
  console.log("RESULT " + path.join(dir, "result.json"));
}
process.exitCode = cases.some((c) => !c.pass) ? 1 : 0;
