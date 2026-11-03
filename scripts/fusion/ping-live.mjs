// SPDX-License-Identifier: Apache-2.0
// Real requests through http://127.0.0.1 on a running instance, the way a client sends them.
// Usage: node.exe ping-live.mjs <package dir> <instance dir> <model>. Creates a temporary client
// key and revokes it afterwards; prints statuses and token counts, never the key.
import path from "node:path";
import { pathToFileURL } from "node:url";

const [pkg, dir, model = "workbuddy/main/deepseek-v4.1-flash"] = process.argv.slice(2);
const { Instance } = await import(pathToFileURL(path.join(pkg, "tools/runtime.mjs")).href);
const instance = new Instance(dir);
await instance.attach();
const rpc = async (method, params) => {
  const r = await instance.admin("/api/loadout/rpc", "POST", { method, params });
  if (r.error) throw Error(`${method}: ${r.error.message}`);
  return r.result;
};
const results = [];
const note = (name, ok, detail) => {
  results.push({ name, ok, detail });
  console.log(`${ok ? "PASS" : "FAIL"} ${name}  ${detail}`);
};
const base = instance.base;
const clientName = "ping-" + Date.now();
const { key } = await rpc("client.create", { name: clientName });
const headers = { authorization: "Bearer " + key, "content-type": "application/json" };
const ask = "Reply with exactly the word LOADOUT_OK and nothing else.";

async function json(name, url, body, read) {
  const started = Date.now();
  const r = await fetch(base + url, { method: "POST", headers, body: JSON.stringify(body), signal: AbortSignal.timeout(120000) });
  const text = await r.text();
  let data;
  try {
    data = JSON.parse(text);
  } catch {
    data = null;
  }
  const answer = data && r.ok ? read(data) : text.slice(0, 200);
  note(name, r.ok && String(answer).includes("LOADOUT_OK"), `HTTP ${r.status} ${Date.now() - started}ms "${String(answer).slice(0, 60)}"`);
}

try {
  const list = await fetch(base + "/v1/models", { headers });
  const ids = list.ok ? (await list.json()).data.map((m) => m.id) : [];
  note("GET /v1/models", list.ok && ids.includes(model), `HTTP ${list.status} ${ids.length} models`);
  const anon = await fetch(base + "/v1/chat/completions", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ model, messages: [{ role: "user", content: ask }] }),
  });
  note("no key is refused", anon.status === 401, `HTTP ${anon.status}`);

  await json("chat, no system message", "/v1/chat/completions", { model, max_tokens: 200, messages: [{ role: "user", content: ask }] }, (d) => d.choices?.[0]?.message?.content);
  await json(
    "chat, own system message",
    "/v1/chat/completions",
    { model, max_tokens: 200, messages: [{ role: "system", content: "You answer tersely." }, { role: "user", content: ask }] },
    (d) => d.choices?.[0]?.message?.content,
  );

  const started = Date.now();
  const stream = await fetch(base + "/v1/chat/completions", {
    method: "POST",
    headers,
    body: JSON.stringify({ model, stream: true, max_tokens: 200, messages: [{ role: "user", content: "Count from 1 to 40, one number per line. End with LOADOUT_OK." }] }),
    signal: AbortSignal.timeout(120000),
  });
  let text = "";
  let chunks = 0;
  let first = 0;
  if (stream.ok)
    for await (const c of stream.body) {
      if (!first) first = Date.now() - started;
      chunks++;
      text += Buffer.from(c).toString();
    }
  // Events, not network chunks: a short reply that arrives at once can come in two chunks.
  const events = text.split("\n").filter((line) => line.startsWith("data: ")).length;
  note(
    "chat, streaming",
    stream.ok && text.includes("[DONE]") && events > 2,
    `HTTP ${stream.status} first byte ${first}ms, ${events} events in ${chunks} chunks, ${Date.now() - started}ms`,
  );

  await json(
    "anthropic messages",
    "/v1/messages",
    { model, max_tokens: 200, messages: [{ role: "user", content: ask }] },
    (d) => (d.content ?? []).map((p) => p.text ?? "").join(""),
  );
  await json(
    "openai responses",
    "/v1/responses",
    { model, input: ask, max_output_tokens: 200 },
    (d) => d.output_text ?? (d.output ?? []).flatMap((o) => o.content ?? []).map((p) => p.text ?? "").join(""),
  );

  const toolReply = await fetch(base + "/v1/chat/completions", {
    method: "POST",
    headers,
    body: JSON.stringify({
      model,
      max_tokens: 200,
      tool_choice: "auto",
      tools: [{ type: "function", function: { name: "get_time", description: "Current time", parameters: { type: "object", properties: {} } } }],
      messages: [{ role: "user", content: "Call the get_time tool now." }],
    }),
    signal: AbortSignal.timeout(120000),
  });
  const toolData = toolReply.ok ? await toolReply.json() : null;
  const calls = toolData?.choices?.[0]?.message?.tool_calls ?? [];
  note("tool call", toolReply.ok && calls[0]?.function?.name === "get_time", `HTTP ${toolReply.status} ${calls.length} call(s)`);

  await new Promise((r) => setTimeout(r, 1000));
  const recent = await rpc("requests.recent", { limit: 8, client: clientName });
  const billed = recent.filter((r) => (r.output_tokens ?? 0) > 0).length;
  note("requests are in the log", recent.length > 0 && billed > 0, `${recent.length} rows, ${billed} with tokens, route ${recent[0]?.route}`);
  const flow = await rpc("stats.flow", { since_ms: Date.now() - 600000 });
  note("flow shows this client", flow.links.some((l) => l.client === clientName), `${flow.links.length} links`);
} finally {
  await rpc("client.revoke", { name: clientName });
}
const failed = results.filter((r) => !r.ok);
console.log(JSON.stringify({ passed: results.length - failed.length, failed: failed.length }));
process.exitCode = failed.length ? 1 : 0;
