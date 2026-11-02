// SPDX-License-Identifier: Apache-2.0
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { createHash, randomUUID } from "node:crypto";
import { Instance, runs, load } from "./harness.mjs";
const { createRelay, MODEL } = await load("workbuddy-relay.mjs");
const { createReader } = await load("credential-reader.mjs");
const { accountDigest } = await load("workbuddy-connect.mjs");
const { sealAuthFieldForTest } = await load("vendor/connector.mjs");
const { CODEX_FIRST_LINE, WORKBUDDY_FIRST_LINE } = await load(
  "workbuddy-first-line.mjs",
);
const dir = path.join(runs, "workbuddy-chain-" + Date.now());
fs.mkdirSync(dir);
const cases = [],
  key = createHash("sha256").update("synthetic-protector").digest();
let token = "synthetic-access-1",
  uid = "synthetic-account",
  observed = [],
  instance,
  relay;
const reader = createReader({
  readDocument: () =>
    JSON.stringify({
      account: { uid },
      auth: {
        accessToken: sealAuthFieldForTest(key, token),
        refreshToken: sealAuthFieldForTest(key, "never-read-refresh"),
        domain: "www.workbuddy.ai",
        expiresAt: Date.now() + 600000,
      },
    }),
  readBinding: () =>
    JSON.stringify({ account_hash: accountDigest("synthetic-account") }),
  keyProvider: { protectorKeyFor: async () => key },
});
const check = async (name, fn) => {
  try {
    await fn();
    cases.push({ name, pass: true });
  } catch (e) {
    cases.push({ name, pass: false, error: e.message.slice(0, 100) });
  }
  console.log(JSON.stringify(cases.at(-1)));
};
try {
  relay = await createRelay({
    key: "synthetic-bridge",
    reader,
    installed: { clientVersion: "1.0.0", cliVersion: "1.0.0" },
    firstLineCompatibility: true,
    fetcher: async (_url, init) => {
      const body = JSON.parse(init.body);
      observed.push({ token: init.headers.Authorization, body });
      const chunk = {
        id: "mock",
        object: "chat.completion.chunk",
        model: MODEL,
        choices: [
          {
            index: 0,
            delta: { role: "assistant", content: "CHAIN_OK" },
            finish_reason: null,
          },
        ],
      };
      return new Response(
        "data: " +
          JSON.stringify(chunk) +
          "\n\ndata: " +
          JSON.stringify({
            ...chunk,
            choices: [{ index: 0, delta: {}, finish_reason: "stop" }],
          }) +
          "\n\ndata: [DONE]\n\n",
        { headers: { "content-type": "text/event-stream" } },
      );
    },
  });
  instance = await Instance.create(path.join(dir, "instance"));
  await instance.start();
  const profile = await instance.addProvider("workbuddy-synthetic", relay.url, [
    { alias: "bound", key: "synthetic-bridge" },
  ]);
  const body = {
    model: profile.model("bound", MODEL),
    messages: [
      { role: "system", content: CODEX_FIRST_LINE + "\nKEEP_SECOND_LINE" },
      {
        role: "user",
        content: "keep-" + "x".repeat(150000) + CODEX_FIRST_LINE,
      },
    ],
    tools: [
      {
        type: "function",
        function: {
          name: "read",
          parameters: { type: "object", description: CODEX_FIRST_LINE },
        },
      },
    ],
  };
  const send = () =>
    fetch(instance.base + "/v1/chat/completions", {
      method: "POST",
      headers: {
        authorization: "Bearer " + profile.key,
        "content-type": "application/json",
      },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(15000),
    });
  await check("complete-backend-to-packaged-connector-long-body", async () => {
    const r = await send(),
      answer = await r.text();
    assert.equal(r.status, 200);
    assert(answer.includes("CHAIN_OK"));
    const sent = observed.at(-1);
    assert.equal(sent.body.model, MODEL);
    assert.equal(
      sent.body.messages[0].content,
      WORKBUDDY_FIRST_LINE + "\nKEEP_SECOND_LINE",
    );
    assert.equal(sent.body.messages[1].content, body.messages[1].content);
    fs.writeFileSync(
      path.join(dir, "synthetic-tool-shape.json"),
      JSON.stringify({ input: body.tools, output: sent.body.tools }, null, 2),
    );
    const normalizedTools = structuredClone(body.tools);
    Object.assign(normalizedTools[0].function.parameters, {
      properties: {},
      additionalProperties: true,
    });
    assert.deepEqual(sent.body.tools, normalizedTools);
    assert.equal(sent.token, "Bearer synthetic-access-1");
  });
  await check("rotation-next-request-uses-new-access-only", async () => {
    token = "synthetic-access-2";
    const r = await send();
    await r.text();
    assert.equal(r.status, 200);
    assert.equal(observed.at(-1).token, "Bearer synthetic-access-2");
    assert.equal(reader.stats.tokenChanges, 1);
  });
  await check("changed-binding-denied-without-upstream", async () => {
    uid = "another-account";
    const n = observed.length,
      r = await send();
    await r.text();
    assert.notEqual(r.status, 200);
    assert.equal(observed.length, n);
  });
} finally {
  if (instance)
    await check("cleanup", async () => {
      assert.equal(await instance.cleanup(), 0);
    });
  if (relay) await relay.close();
  fs.writeFileSync(
    path.join(dir, "result.json"),
    JSON.stringify({ cases }, null, 2),
  );
  console.log("RESULT " + path.join(dir, "result.json"));
}
process.exitCode = cases.some((c) => !c.pass) ? 1 : 0;
