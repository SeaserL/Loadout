// SPDX-License-Identifier: Apache-2.0
import assert from "node:assert/strict";
import fs from "node:fs";
import { createHash } from "node:crypto";
import { load } from "./harness.mjs";
const { createReader } = await load("credential-reader.mjs");
const { createRelay, MODEL } = await load("workbuddy-relay.mjs");
const { accountDigest } = await load("workbuddy-connect.mjs");
const { sealAuthFieldForTest, WorkBuddyAtRestKeyProvider, AI_VARIANT } =
  await load("vendor/connector.mjs");
const result = { cases: [] };
async function check(name, fn) {
  await fn();
  result.cases.push({ name, pass: true });
}
const secret = Buffer.alloc(32, 9).toString("base64"),
  key = createHash("sha256").update(secret).digest();
let helperCalls = 0;
const provider = new WorkBuddyAtRestKeyProvider({
  product: AI_VARIANT.electron,
  source: async () => {
    helperCalls++;
    return JSON.stringify({ version: 1, atRestSecretKey: secret });
  },
});
const make = (token, uid = "synthetic-account", expiry = Date.now() + 60000) =>
  JSON.stringify({
    auth: {
      accessToken: sealAuthFieldForTest(key, token),
      refreshToken: sealAuthFieldForTest(key, "synthetic-refresh"),
      domain: "www.workbuddy.ai",
      expiresAt: expiry,
    },
    account: { uid },
  });
let raw = make("synthetic-a"),
  binding = JSON.stringify({
    account_hash: accountDigest("synthetic-account"),
  });
const reader = createReader({
  readDocument: () => raw,
  readBinding: () => binding,
  keyProvider: provider,
});
await check("read-current-encrypted-access-only", async () => {
  const r = await reader.current();
  assert.equal(r.value.accessToken, "synthetic-a");
  assert.equal(r.value.refreshToken, "");
});
await check("same-account-rotation-and-single-helper", async () => {
  raw = make("synthetic-b");
  assert.equal((await reader.current()).value.accessToken, "synthetic-b");
  await reader.current();
  assert.equal(helperCalls, 1);
  assert.equal(reader.stats.tokenChanges, 1);
});
await check("changed-account-fails-before-decrypt", async () => {
  raw = make("synthetic-c", "another-account");
  await assert.rejects(reader.current(), /ACCOUNT_CHANGED/);
  assert.equal(helperCalls, 1);
});
await check("expired-fails-closed", async () => {
  raw = make("synthetic-expired", "synthetic-account", Date.now() - 2000);
  await assert.rejects(reader.current(), /UNUSABLE_DESKTOP_SESSION/);
});
await check("partial-file-fails-closed", async () => {
  raw = "{";
  await assert.rejects(reader.current(), /AUTH_SOURCE_INVALID/);
});
await check("binding-change-fails-closed", async () => {
  raw = make("synthetic-b");
  binding = JSON.stringify({ account_hash: accountDigest("another-account") });
  await assert.rejects(reader.current(), /ACCOUNT_CHANGED/);
  binding = JSON.stringify({
    account_hash: accountDigest("synthetic-account"),
  });
});
let upstreamCalls = 0,
  mode = "tool",
  aborted = false;
const fetcher = async (url, init) => {
  upstreamCalls++;
  assert.equal(init.redirect, "error");
  assert.equal(init.headers.Authorization, "Bearer synthetic-b");
  assert(!JSON.stringify(init.headers).includes("synthetic-refresh"));
  const b = JSON.parse(init.body);
  assert.equal(b.messages[0].content, "unchanged system");
  if (mode === "pending")
    return new Promise((resolve, reject) =>
      init.signal.addEventListener(
        "abort",
        () => {
          aborted = true;
          reject(Error("aborted"));
        },
        { once: true },
      ),
    );
  if (mode === "reject")
    return new Response(
      JSON.stringify({
        code: 11128,
        msg: "Illegal API invocation from an unapproved channel",
      }),
      { status: 400 },
    );
  const model = mode === "wrong" ? "different-model" : MODEL;
  return new Response(
    "data: " +
      JSON.stringify({
        model,
        choices: [
          {
            index: 0,
            delta: {
              tool_calls: [
                {
                  index: 0,
                  id: "call-1",
                  type: "function",
                  function: { name: "read", arguments: "{}" },
                },
              ],
            },
            finish_reason: "tool_calls",
          },
        ],
      }) +
      "\n\ndata: [DONE]\n\n",
    { status: 200 },
  );
};
const relay = await createRelay({
  key: "synthetic-bridge-key",
  reader,
  installed: { clientVersion: "5.9.4", cliVersion: "2.165.0" },
  fetcher,
  cooldownMs: 50,
});
const body = {
  model: MODEL,
  stream: true,
  messages: [
    { role: "system", content: "unchanged system" },
    { role: "user", content: "fixture" },
  ],
  tools: [
    {
      type: "function",
      function: { name: "read", parameters: { type: "object" } },
    },
  ],
};
const request = (b = body, key = "synthetic-bridge-key", signal) =>
  fetch(relay.url + "/chat/completions", {
    method: "POST",
    headers: {
      authorization: "Bearer " + key,
      "content-type": "application/json",
    },
    body: JSON.stringify(b),
    signal,
  });
try {
  await check("tools-preserved-no-protocol-reimplementation", async () => {
    const r = await request();
    assert.equal(r.status, 200);
    assert((await r.text()).includes("tool_calls"));
    assert.equal(relay.state.calls[0].outputToolCalls, 1);
  });
  await check(
    "nonstream-input-uses-upstream-sse-without-conversion",
    async () => {
      const r = await request({ ...body, stream: false });
      assert.equal(r.status, 200);
      assert.equal(r.headers.get("content-type"), "text/event-stream");
      assert((await r.text()).includes("tool_calls"));
    },
  );
  await check("auth-and-model-denied-before-egress", async () => {
    const n = upstreamCalls;
    assert.equal((await request(body, "bad")).status, 401);
    assert.equal((await request({ ...body, model: "other" })).status, 400);
    assert.equal(upstreamCalls, n);
  });
  await check("more-than-sixteen-calls-without-test-budget", async () => {
    for (let i = 0; i < 20; i++) {
      const r = await request();
      assert.equal(r.status, 200);
      await r.text();
    }
  });
  await check("model-output-mismatch-denied", async () => {
    mode = "wrong";
    relay.state.mode = "wrong";
    assert.equal((await request()).status, 502);
  });
  await check("channel-rejection-stops-same-case", async () => {
    await new Promise((r) => setTimeout(r, 80));
    mode = "reject";
    relay.state.mode = "reject";
    assert.equal((await request()).status, 400);
    const n = upstreamCalls;
    assert.equal((await request()).status, 503);
    assert.equal(upstreamCalls, n);
  });
  await check("cancel-propagates-to-fetch", async () => {
    await new Promise((r) => setTimeout(r, 80));
    mode = "pending";
    relay.state.mode = "cancel";
    const abort = new AbortController();
    const n = upstreamCalls;
    const pending = request(body, "synthetic-bridge-key", abort.signal).catch(
      () => null,
    );
    while (upstreamCalls === n) await new Promise((r) => setTimeout(r, 5));
    abort.abort();
    await pending;
    await new Promise((r) => setTimeout(r, 30));
    assert(aborted);
  });
} finally {
  await relay.close();
}
fs.mkdirSync(process.env.FUSION_RELEASE_ROOT + "/evidence", { recursive: true });
fs.writeFileSync(
  process.env.FUSION_RELEASE_ROOT + "/evidence/offline.json",
  JSON.stringify(result, null, 2),
);
console.log(JSON.stringify(result));
