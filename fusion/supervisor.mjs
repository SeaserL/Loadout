// SPDX-License-Identifier: Apache-2.0
import fs from "node:fs";
import path from "node:path";
import http from "node:http";
import { once } from "node:events";
import { spawn } from "node:child_process";
import { randomUUID, timingSafeEqual, createHash } from "node:crypto";
import { pathToFileURL } from "node:url";
import {
  Instance,
  packageRoot,
  toolsRoot,
  cleanEnv,
  freePort,
  takeLock,
  writeJson,
  sleep,
  verifyPackage,
} from "./runtime.mjs";
const instance = new Instance(process.argv[2]);
let lock,
  child,
  server,
  relay,
  closing = false;
async function stop() {
  if (closing) return;
  closing = true;
  if (child && child.exitCode === null) {
    const ended = once(child, "exit");
    child.kill();
    await ended;
  }
  if (relay) await relay.close();
  if (server) {
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
  lock?.release();
  process.exit(0);
}
try {
  lock = takeLock(instance);
  verifyPackage(false);
  const secrets = await instance.secrets(),
    run = randomUUID();
  // cleanEnv points the backend's home at the instance; the control plane still needs the
  // owner's real folders to look for installed apps and to save exports where they can see them.
  const userHome = process.env.LOADOUT_USER_HOME ?? process.env.USERPROFILE ?? "";
  const userLocal = process.env.LOADOUT_USER_LOCALAPPDATA ?? process.env.LOCALAPPDATA ?? "";
  process.env.FUSION_EGRESS_HOSTS = JSON.stringify(
    instance.meta.config.network === "allowlist"
      ? (instance.meta.config.hosts ?? [])
      : [],
  );
  process.env.FUSION_EGRESS_MODE = instance.meta.config.network === "direct" ? "direct" : "isolated";
  process.env.FUSION_EGRESS_LOG = path.join(instance.dir, "egress.log");
  await import("./isolation-guard.mjs");
  if (instance.meta.config.workbuddy) {
    const { desktopReader } = await import("./credential-reader.mjs");
    const { createRelay } = await import("./workbuddy-relay.mjs");
    const { installedVersions } = await import("./workbuddy-connect.mjs");
    const wb = { ...instance.meta.config.workbuddy, ...installedVersions(instance.meta.config.workbuddy.exe) };
    if (!secrets.bridge) throw Error("BRIDGE_KEY_MISSING");
    relay = await createRelay({
      key: secrets.bridge,
      reader: desktopReader(wb),
      installed: wb,
      port: wb.port,
      live: true,
      firstLineCompatibility: true,
    });
  }
  const port = instance.meta.config.port || (await freePort());
  const { productVersion } = JSON.parse(fs.readFileSync(path.join(packageRoot, "fusion-manifest.json"), "utf8"));
  const env = cleanEnv(instance.dir);
  Object.assign(env, {
    NODE_ENV: "production",
    HOSTNAME: "127.0.0.1",
    PORT: String(port),
    DATA_DIR: path.join(instance.dir, "data"),
    OMNIROUTE_MIGRATIONS_DIR: path.join(packageRoot, "migrations"),
    REQUIRE_API_KEY: "true",
    NEXT_TELEMETRY_DISABLED: "1",
    OMNIROUTE_HEADLESS: "1",
    OMNIROUTE_DISABLE_BACKGROUND_SERVICES: "true",
    // The native credential scheduler has a separate import-time opt-out.
    OMNIROUTE_DISABLE_CREDENTIAL_HEALTH_CHECK: "true",
    OMNIROUTE_DISABLE_CONVERSATION_TRACKING: "1",
    RADAR_ENABLED: "true",
    PRICING_SYNC_ENABLED: instance.meta.config.pricingSync === true ? "true" : "false",
    ...(instance.meta.config.radar?.feedUrl ? { RADAR_FEED_URL: instance.meta.config.radar.feedUrl } : {}),
    ...(instance.meta.config.radar?.publicKey ? { RADAR_FEED_PUBKEY: instance.meta.config.radar.publicKey } : {}),
    LOADOUT_TEST_SERVICE: instance.meta.service,
    LOADOUT_FUSION_LAUNCHER: "1",
    LOADOUT_VERSION: productVersion ?? "",
    API_KEY_SECRET: secrets.api,
    JWT_SECRET: secrets.jwt,
    INITIAL_PASSWORD: secrets.password,
    ENABLE_REQUEST_LOGS: "false",
    // Metadata only, and the usage pages reach back a year; the upstream default keeps 7 days.
    CALL_LOG_RETENTION_DAYS: "400",
    CALL_LOGS_TABLE_MAX_ROWS: "1000000",
    FUSION_OWNER_PID: String(process.pid),
    FUSION_EGRESS_HOSTS: process.env.FUSION_EGRESS_HOSTS,
    FUSION_EGRESS_MODE: process.env.FUSION_EGRESS_MODE,
    FUSION_EGRESS_LOG: process.env.FUSION_EGRESS_LOG,
    LOADOUT_USER_HOME: userHome,
    LOADOUT_USER_LOCALAPPDATA: userLocal,
    NODE_OPTIONS:
      "--import=" +
      pathToFileURL(path.join(toolsRoot, "isolation-guard.mjs")).href,
  });
  child = spawn(path.join(packageRoot, "node.exe"), ["server.js"], {
    cwd: packageRoot,
    env,
    stdio: "ignore",
    windowsHide: true,
  });
  child.on("error", () => {
    void stop();
  });
  lock.backend(child.pid);
  let ready = false;
  for (let i = 0; i < 120; i++) {
    if (child.exitCode !== null) throw Error("BACKEND_EXIT");
    try {
      ready = (
        await fetch(`http://127.0.0.1:${port}/healthz`, {
          signal: AbortSignal.timeout(400),
        })
      ).ok;
    } catch {}
    if (ready) break;
    await sleep(400);
  }
  if (!ready) throw Error("BACKEND_TIMEOUT");
  const login = await fetch(`http://127.0.0.1:${port}/api/auth/login`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      origin: `http://127.0.0.1:${port}`,
    },
    body: JSON.stringify({ password: secrets.password }),
    signal: AbortSignal.timeout(10000),
  });
  if (!login.ok || child.exitCode !== null)
    throw Error("BACKEND_IDENTITY_FAILED");
  const expected = createHash("sha256")
    .update("Bearer " + secrets.control)
    .digest();
  const state = {
    status: "ready",
    instance: instance.meta.id,
    run,
    port,
    pid: process.pid,
    backendPid: child.pid,
  };
  server = http.createServer((req, res) => {
    const supplied = createHash("sha256")
      .update(req.headers.authorization ?? "")
      .digest();
    if (req.headers.origin || !timingSafeEqual(expected, supplied)) {
      res.writeHead(403);
      res.end();
      return;
    }
    if (
      !(req.method === "GET" && req.url === "/status") &&
      !(req.method === "POST" && req.url === "/stop")
    ) {
      res.writeHead(404);
      res.end();
      return;
    }
    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify(state));
    if (req.url === "/stop")
      setTimeout(() => {
        void stop();
      }, 100);
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  writeJson(path.join(instance.dir, "running.json"), {
    ...state,
    controlPort: server.address().port,
  });
  instance.meta.config.port = port;
  writeJson(path.join(instance.dir, "instance.json"), instance.meta);
  child.once("exit", () => {
    void stop();
  });
  // The UI asks through a file: the backend must not stop itself in the middle of a response.
  const requestFile = path.join(instance.dir, "control-request.json");
  setInterval(() => {
    try {
      const request = JSON.parse(fs.readFileSync(requestFile, "utf8"));
      fs.unlinkSync(requestFile);
      if (request.op === "stop") void stop();
      if (request.op === "restart") {
        spawn(path.join(packageRoot, "node.exe"), [path.join(toolsRoot, "restart.mjs"), instance.dir, ...(request.connect ? [request.connect] : [])], {
          cwd: packageRoot,
          env: process.env,
          detached: true,
          windowsHide: true,
          stdio: "ignore",
        }).unref();
        void stop();
      }
    } catch {}
  }, 1000).unref();
  process.on("SIGTERM", () => {
    void stop();
  });
  process.on("SIGINT", () => {
    void stop();
  });
} catch (error) {
  // No upstream text, configuration values or credentials in the diagnostic file.
  writeJson(path.join(instance.dir, "last-start.json"), {
    status: "failed",
    category: [
      "INSTANCE_BUSY",
      "PACKAGE_CHANGED",
      "BOOTSTRAP_MISSING",
      "BACKEND_EXIT",
      "BACKEND_TIMEOUT",
    ].includes(error.message)
      ? error.message
      : "START_FAILED",
  });
  await stop();
}
