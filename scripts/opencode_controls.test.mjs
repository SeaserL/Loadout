// SPDX-License-Identifier: Apache-2.0
import test from "node:test";
import assert from "node:assert/strict";
import plugin from "./opencode_controls.mjs";

test("official hook preserves native identity/tools and applies requested semantics", async () => {
  process.env.LOADOUT_OPENCODE_CONTROL_URL = "http://127.0.0.1:1";
  process.env.OPENCODE_SERVER_PASSWORD = "synthetic";
  const hooks = {};
  await plugin.setup({ session: { async hook(name, fn) { hooks[name] = fn; } } });
  const retry = { decision: { retry: true, delay: 1 } };
  await hooks.retry(retry);
  assert.deepEqual(retry.decision, { retry: false });
  const title = {};
  await hooks.title(title);
  assert.equal(title.result, "Loadout transient request");
  const oldFetch = globalThis.fetch;
  let acknowledged;
  const controls = { max_tokens: 16, tool_choice: { type: "function", function: { name: "loadout_read" } }, external_tools: ["loadout_read"] };
  globalThis.fetch = async (url, options) => {
    assert.equal(url, "http://127.0.0.1:1/internal/control/ses_test");
    if (options.method === "POST") acknowledged = JSON.parse(options.body);
    return Response.json(options.method === "POST" ? { ok: true } : controls);
  };
  try {
    const original = { model: "deepseek-v4.1-flash-free", messages: [{ role: "system", content: "original" }, { role: "user", content: "task" }], tools: [{ type: "function", function: { name: "native" } }], max_completion_tokens: 4096 };
    const event = { kind: "primary", sessionID: "ses_test", request: new Request("https://example.invalid/chat", { method: "POST", headers: { authorization: "Bearer original", "x-opencode-session-id": "ses_test" }, body: JSON.stringify(original) }) };
    await hooks["http.request"](event);
    const body = await event.request.json();
    assert.equal(body.model, original.model);
    assert.deepEqual(body.tools, original.tools);
    assert.equal(event.request.headers.get("authorization"), "Bearer original");
    assert.equal(event.request.headers.get("x-opencode-session-id"), "ses_test");
    assert.equal(body.max_tokens, 16);
    assert.equal(body.max_completion_tokens, undefined);
    assert.deepEqual(body.tool_choice, controls.tool_choice);
    assert.deepEqual(acknowledged, controls);
    assert.deepEqual(body.messages[0], original.messages[0]);
    assert.deepEqual(body.messages.at(-1), original.messages[1]);
    globalThis.fetch = async () => new Response("", { status: 404 });
    await assert.rejects(hooks["http.request"]({ ...event, request: new Request("https://example.invalid", { method: "POST", body: "{}" }) }));
  } finally { globalThis.fetch = oldFetch; }
});
