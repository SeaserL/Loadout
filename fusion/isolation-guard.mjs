// SPDX-License-Identifier: Apache-2.0
// Explicit per-instance egress allowlist; empty by default, loopback-only in tests.
import fs from "node:fs";
import net from "node:net";
import tls from "node:tls";
import Module, { syncBuiltinESMExports } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";
const packageBoundary =
  path
    .resolve(path.dirname(fileURLToPath(import.meta.url)), "..")
    .toLowerCase() + path.sep;
// 1.1 (ExecPlan 071): the upstream entry, server-ws.mjs, stamps every request with its real TCP
// peer so OmniRoute's loopback-only routes can tell a local caller from a remote one. Loadout starts
// server.js directly and keeps the rest of server-ws out (its WebSocket and WebDAV handlers would
// answer before proxy.ts's allowlist), so only the stamp is installed, and only in the backend.
// Of the loopback-only routes, proxy.ts lets through /api/mcp/stream alone.
if (process.env.LOADOUT_FUSION_LAUNCHER === "1") {
  const { default: http } = await import("node:http");
  const { pathToFileURL } = await import("node:url");
  const stamp = await import(
    pathToFileURL(path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "peer-stamp.mjs")).href
  );
  stamp.ensurePeerStampToken();
  const createServer = http.createServer.bind(http);
  http.createServer = (...args) => {
    const at = args.findIndex((a) => typeof a === "function");
    if (at >= 0) args[at] = stamp.wrapRequestListenerWithPeerStamp(args[at]);
    return createServer(...args);
  };
}
const originalLoad = Module._load;
Module._load = function (request, parent, main) {
  const resolved = Module._resolveFilename(request, parent, main);
  if (
    typeof resolved === "string" &&
    path.isAbsolute(resolved) &&
    !path.resolve(resolved).toLowerCase().startsWith(packageBoundary)
  )
    throw Error("PACKAGE_EXTERNAL_DEPENDENCY");
  return originalLoad.apply(this, arguments);
};
const hosts = [
  "127.0.0.1",
  "::1",
  "localhost",
  ...JSON.parse(process.env.FUSION_EGRESS_HOSTS || "[]"),
];
// OmniRoute's own update checks and public-IP lookups; none of them serves a request.
const NEVER = new Set(["registry.npmjs.org", "api.github.com", "api.ipify.org", "api4.ipify.org", "api64.ipify.org"]);
const direct = process.env.FUSION_EGRESS_MODE === "direct";
const logFile = process.env.FUSION_EGRESS_LOG;
const seen = new Set(hosts);
const check = (host) => {
  const name = String(host || "localhost");
  if (direct) {
    if (NEVER.has(name)) throw Error("ISOLATED_EGRESS_DENIED");
    // Host names only: lets the owner confirm that an idle gateway connects to nothing.
    if (logFile && !seen.has(name)) {
      seen.add(name);
      try {
        fs.appendFileSync(logFile, `${new Date().toISOString()} ${name}\n`);
      } catch {}
    }
    return;
  }
  if (!hosts.includes(name)) throw Error("ISOLATED_EGRESS_DENIED");
};
const connect = net.Socket.prototype.connect;
net.Socket.prototype.connect = function (...args) {
  let a = args[0];
  if (Array.isArray(a)) a = a[0];
  if (typeof a === "object") {
    if (a.path) throw Error("SOCKET_PATH_DENIED");
    check(a.host);
  } else check(typeof args[1] === "string" ? args[1] : "localhost");
  return connect.apply(this, args);
};
const secure = tls.connect;
tls.connect = function (...args) {
  const a = args[0];
  check(typeof a === "object" ? a.host : args[1]);
  return secure.apply(this, args);
};
const original = globalThis.fetch;
globalThis.fetch = (input, init) => {
  check(new URL(input instanceof Request ? input.url : String(input)).hostname);
  return original(input, init);
};
syncBuiltinESMExports();
if (process.env.FUSION_OWNER_PID) {
  const owner = Number(process.env.FUSION_OWNER_PID);
  setInterval(() => {
    try {
      process.kill(owner, 0);
    } catch {
      process.exit(70);
    }
  }, 500).unref();
}
