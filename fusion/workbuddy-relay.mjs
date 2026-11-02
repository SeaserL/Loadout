// SPDX-License-Identifier: Apache-2.0
// Connector for the signed-in WorkBuddy desktop app. OmniRoute owns protocol conversion and tool
// semantics; this only attaches the account's current token and passes the SSE stream through.
import http from "node:http";
import { once } from "node:events";
import { createHash, timingSafeEqual } from "node:crypto";
import { adaptWorkbuddyFirstLine } from "./workbuddy-first-line.mjs";
import { endpoint, wireHeaders, errorSummary } from "./workbuddy-connect.mjs";
export const MODEL = "deepseek-v4.1-flash";
// The `cli` roster of the international /v3/config document, as recorded by
// dsh-workbuddy-connect (src/catalog.ts). A snapshot, not a promise about the upstream.
export const MODELS = [
  "default-model",
  "fast-model",
  "balanced-model",
  "primary-model",
  "deep-model",
  "hy4-preview-f",
  "hy3",
  MODEL,
  "gpt-6-astra",
  "gpt-5.6-sol",
  "gpt-5.6-terra",
  "gpt-5.6-luna",
  "gpt-5.5",
  "gpt-5.4",
  "gpt-5.3-codex",
  "gemini-3.5-flash",
  "glm-5.3",
  "glm-5.2",
  "kimi-k3",
  "kimi-k2.6",
];
// The upstream refuses a conversation that does not open with a system message. Clients that
// send none get a neutral one; a system message the client wrote is never touched.
const DEFAULT_SYSTEM = "You are a helpful assistant.";
const MAX_BODY = 32 * 1024 * 1024;
const REQUEST_TIMEOUT_MS = 600000;
const MAX_CONCURRENT = 6;
// `*-model` ids are upstream aliases; the reply legitimately names the model behind them.
const aliasModel = (id) => id.endsWith("-model");
export async function createRelay({
  key,
  reader,
  installed,
  fetcher = fetch,
  port = 0,
  cooldownMs = 30000,
  live = false,
  firstLineCompatibility = false,
}) {
  if (fetcher === fetch && !live) throw Error("LIVE_OPT_IN_REQUIRED");
  const state = {
    mode: "runtime",
    blockedUntil: 0,
    calls: [],
    denied: [],
    stopped: false,
    active: 0,
  };
  const expected = createHash("sha256")
    .update("Bearer " + key)
    .digest();
  const server = http.createServer(async (req, res) => {
    const error = (status, category) => {
      state.denied.push(category);
      if (state.denied.length > 100) state.denied.shift();
      if (!res.headersSent) {
        res.writeHead(status, { "content-type": "application/json" });
        res.end(
          JSON.stringify({
            error: { message: category, type: "isolated_connector_error" },
          }),
        );
      }
    };
    if (
      !timingSafeEqual(
        expected,
        createHash("sha256")
          .update(req.headers.authorization ?? "")
          .digest(),
      )
    )
      return error(401, "BRIDGE_KEY_REQUIRED");
    if (req.headers.origin) return error(403, "ORIGIN_DENIED");
    if (req.method === "GET" && req.url === "/v1/models") {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(
        JSON.stringify({
          data: MODELS.map((id) => ({ id, object: "model" })),
        }),
      );
      return;
    }
    if (req.method !== "POST" || req.url !== "/v1/chat/completions")
      return error(404, "PATH_DENIED");
    if (Date.now() < state.blockedUntil) return error(503, "EGRESS_CLOSED");
    if (state.active >= MAX_CONCURRENT) return error(429, "CONCURRENCY_LIMIT");
    state.active++;
    let session;
    const abort = new AbortController();
    const timer = setTimeout(() => abort.abort(), REQUEST_TIMEOUT_MS);
    res.on("close", () => {
      if (!res.writableEnded) abort.abort();
    });
    try {
      const parts = [];
      let size = 0;
      for await (const c of req) {
        size += c.length;
        if (size > MAX_BODY) return error(413, "BODY_LIMIT");
        parts.push(c);
      }
      let body;
      try {
        body = JSON.parse(Buffer.concat(parts));
      } catch {
        return error(400, "INVALID_JSON");
      }
      if (!MODELS.includes(body.model)) return error(400, "MODEL_DENIED");
      // WorkBuddy requires SSE; OmniRoute already consumes SSE for non-stream clients.
      body.stream = true;
      if (body.previous_response_id || body.store === true)
        return error(400, "STATEFUL_DENIED");
      if (!Array.isArray(body.messages) || body.messages.length === 0)
        return error(400, "MESSAGES_REQUIRED");
      if (body.messages[0]?.role !== "system")
        body.messages = [
          { role: "system", content: DEFAULT_SYSTEM },
          ...body.messages,
        ];
      session = await reader.current();
      const compatible =
        firstLineCompatibility === true ? adaptWorkbuddyFirstLine(body) : body;
      const firstLineCompatibilityApplied = compatible !== body;
      body = compatible;
      const toolReplies = body.messages.filter((m) => m.role === "tool");
      const call = {
        mode: state.mode,
        model: body.model,
        bytes: size,
        messages: body.messages.length,
        tools: body.tools?.length ?? 0,
        toolReplies: toolReplies.length,
        started: new Date().toISOString(),
        stream: true,
        firstLineCompatibilityApplied,
      };
      state.calls.push(call);
      if (state.calls.length > 100) state.calls.shift();
      const start = performance.now();
      const reply = await fetcher(endpoint, {
        method: "POST",
        headers: wireHeaders(session.value, installed),
        body: JSON.stringify(body),
        redirect: "error",
        signal: abort.signal,
      });
      call.status = reply.status;
      call.headerMs = performance.now() - start;
      if (reply.status !== 200) {
        const chunks = [];
        for await (const c of reply.body) chunks.push(Buffer.from(c));
        const summary = errorSummary(reply.status, Buffer.concat(chunks).toString());
        call.category = summary.category;
        call.applicationCode = summary.applicationCode;
        // Repeating a request the channel check refused can only hurt the account.
        if (summary.category === "CHANNEL_REJECTED")
          state.blockedUntil = Date.now() + cooldownMs;
        return error(reply.status, "UPSTREAM_" + call.category);
      }
      let done = false,
        finished = false,
        committed = false,
        pending = "",
        held = [];
      const tools = new Set();
      const inspect = (line) => {
        if (!line.startsWith("data:")) return;
        const data = line.slice(5).trim();
        if (data === "[DONE]") {
          done = true;
          return;
        }
        if (!data) return;
        let chunk;
        try {
          chunk = JSON.parse(data);
        } catch {
          return;
        }
        if (chunk.error || (chunk.code && chunk.code !== 0))
          throw Error("STREAM_ERROR");
        if (chunk.model && !committed) {
          if (!aliasModel(body.model) && chunk.model !== body.model)
            throw Error("MODEL_MISMATCH");
          committed = true;
        }
        for (const choice of chunk.choices ?? []) {
          if (choice.finish_reason) finished = true;
          for (const t of choice.delta?.tool_calls ?? [])
            tools.add(t.index ?? t.id);
        }
      };
      // Nothing reaches the client until the first event proves the stream is the right one;
      // after that it flows as it arrives, and a broken stream closes the connection.
      const flush = () => {
        if (!res.headersSent)
          res.writeHead(200, {
            "content-type": "text/event-stream",
            "cache-control": "no-store",
          });
        for (const c of held) res.write(c);
        held = [];
      };
      for await (const c of reply.body) {
        const buffer = Buffer.from(c);
        pending += buffer.toString("utf8");
        const lines = pending.split(/\r?\n/);
        pending = lines.pop() ?? "";
        for (const line of lines) inspect(line);
        held.push(buffer);
        if (committed || done) flush();
      }
      if (pending) inspect(pending);
      call.ms = performance.now() - start;
      call.outputToolCalls = tools.size;
      if (!done || !finished) throw Error("INCOMPLETE_STREAM");
      flush();
      call.complete = true;
      res.end();
    } catch (e) {
      const category = [
        "ACCOUNT_CHANGED",
        "UNUSABLE_DESKTOP_SESSION",
        "AUTH_SOURCE_INVALID",
        "MODEL_MISMATCH",
        "INCOMPLETE_STREAM",
        "STREAM_ERROR",
      ].includes(e.message)
        ? e.message
        : "CONNECTOR_FAILED";
      if (res.headersSent) res.destroy();
      else error(502, category);
    } finally {
      if (session) session.value.accessToken = "";
      clearTimeout(timer);
      state.active--;
    }
  });
  server.listen(port, "127.0.0.1");
  await once(server, "listening");
  return {
    state,
    url: `http://127.0.0.1:${server.address().port}/v1`,
    async close() {
      server.closeAllConnections();
      await new Promise((r) => server.close(r));
    },
  };
}
