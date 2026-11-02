// SPDX-License-Identifier: Apache-2.0
import fs from "node:fs";
import path from "node:path";
import http from "node:http";
import { once } from "node:events";
import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";
import { randomUUID, createHash, createPublicKey } from "node:crypto";
export const toolsRoot = path.dirname(fileURLToPath(import.meta.url));
export const packageRoot = path.resolve(toolsRoot, "..");
export const keytar = createRequire(path.join(packageRoot, "package.json"))(
  path.join(packageRoot, "node_modules/keytar"),
);
export const sha = (value) => createHash("sha256").update(value).digest("hex");
export const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
export const json = (file) =>
  JSON.parse(fs.readFileSync(file, "utf8").replace(/^\uFEFF/, ""));
export const manifest = () =>
  json(path.join(packageRoot, "fusion-manifest.json"));
export function validateConfig(config) {
  if (
    !config ||
    Object.keys(config).some(
      (k) => !["network", "port", "hosts", "workbuddy", "radar", "pricingSync"].includes(k),
    )
  )
    throw Error("INVALID_CONFIG");
  if (
    !["loopback-only", "allowlist", "direct"].includes(config.network) ||
    !Number.isInteger(config.port) ||
    config.port < 0 ||
    config.port > 65535 ||
    config.port === 8317
  )
    throw Error("INVALID_ISOLATED_CONFIG");
  if (
    config.hosts !== undefined &&
    (!Array.isArray(config.hosts) ||
      config.hosts.some(
        (h) => typeof h !== "string" || !/^[a-z0-9][a-z0-9.-]+$/.test(h),
      ))
  )
    throw Error("INVALID_EGRESS_HOSTS");
  if (config.network === "loopback-only" && config.hosts?.length)
    throw Error("NETWORK_CONFIG_CONFLICT");
  if (config.pricingSync !== undefined && typeof config.pricingSync !== "boolean")
    throw Error("INVALID_PRICING_SYNC_CONFIG");
  const radar = config.radar;
  if (radar !== undefined) {
    try {
      if (!radar || typeof radar !== "object" || Array.isArray(radar) ||
          Object.keys(radar).some(k => !["feedUrl", "publicKey"].includes(k)) ||
          typeof radar.feedUrl !== "string") throw Error();
      const url = new URL(radar.feedUrl);
      if (url.username || url.password || url.search || url.hash ||
          !(url.protocol === "https:" || (url.protocol === "http:" && url.hostname === "127.0.0.1"))) throw Error();
      if (radar.publicKey !== undefined &&
          (typeof radar.publicKey !== "string" || !/^[A-Za-z0-9+/=]{40,256}$/.test(radar.publicKey) ||
           createPublicKey({key:Buffer.from(radar.publicKey,"base64"),format:"der",type:"spki"}).asymmetricKeyType !== "ed25519")) throw Error();
    } catch { throw Error("INVALID_RADAR_CONFIG"); }
  }
  const wb = config.workbuddy;
  if (wb) {
    if (
      Object.keys(wb).some(
        (k) =>
          ![
            "authPath",
            "exe",
            "accountHash",
            "clientVersion",
            "cliVersion",
            "port",
          ].includes(k),
      ) ||
      !/^[a-f0-9]{64}$/.test(wb.accountHash) ||
      !/^\d+(\.\d+){2,3}$/.test(wb.clientVersion) ||
      !/^\d+(\.\d+){2}(-[\w.]+)?$/.test(wb.cliVersion) ||
      !Number.isInteger(wb.port) ||
      wb.port <= 0 ||
      wb.port > 65535 ||
      wb.port === 8317 ||
      wb.port === config.port
    )
      throw Error("INVALID_WORKBUDDY_CONFIG");
    for (const file of [wb.authPath, wb.exe])
      if (typeof file !== "string" || !path.isAbsolute(file))
        throw Error("ABSOLUTE_SOURCE_REQUIRED");
  }
  return config;
}
export function verifyPackage(full = true) {
  const m = manifest();
  if (sha(JSON.stringify(m.files)) !== m.treeHash)
    throw Error("MANIFEST_INVALID");
  for (const item of m.files) {
    if (
      !full &&
      !item.path.startsWith("tools/") &&
      !item.path.startsWith("public/loadout/") &&
      !["node.exe", "server.js", "package.json", "Loadout.exe", "Start-Loadout.cmd", "Stop-Backend.cmd"].includes(item.path)
    )
      continue;
    const file = path.resolve(packageRoot, item.path);
    if (
      !file.startsWith(packageRoot + path.sep) ||
      sha(fs.readFileSync(file)) !== item.sha256
    )
      throw Error("PACKAGE_CHANGED");
  }
  return m;
}
export function noLinks(target) {
  const p = path.resolve(target);
  for (let x = p; x !== path.dirname(x); x = path.dirname(x)) {
    if (fs.existsSync(x) && fs.lstatSync(x).isSymbolicLink())
      throw Error("LINK_TARGET");
  }
  return p;
}
export function safeFresh(target) {
  const p = noLinks(target);
  if (fs.existsSync(p)) throw Error("TARGET_EXISTS");
  return p;
}
export function isolatedPath(target) {
  const p = noLinks(target);
  if (!p.toLowerCase().split(path.sep).includes("loadout-fusion-isolated"))
    throw Error("ISOLATED_DIRECTORY_REQUIRED");
  if (p.toLowerCase().startsWith(packageRoot.toLowerCase() + path.sep))
    throw Error("DATA_OUTSIDE_PACKAGE_REQUIRED");
  return p;
}
export function alive(pid) {
  if (!Number.isSafeInteger(pid) || pid < 1) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}
export function writeJson(file, value) {
  const tmp = file + "." + randomUUID() + ".tmp";
  fs.writeFileSync(tmp, JSON.stringify(value, null, 2), { flag: "wx" });
  fs.renameSync(tmp, file);
}
export function cleanEnv(dir) {
  const env = {};
  for (const name of [
    "SystemRoot",
    "WINDIR",
    "COMSPEC",
    "PATH",
    "PATHEXT",
    "PROCESSOR_ARCHITECTURE",
  ])
    if (process.env[name]) env[name] = process.env[name];
  for (const name of [
    "HOME",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "TEMP",
    "TMP",
  ]) {
    env[name] = path.join(dir, name);
    fs.mkdirSync(env[name], { recursive: true });
  }
  return env;
}
export async function freePort() {
  const server = http.createServer();
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const port = server.address().port;
  await new Promise((resolve) => server.close(resolve));
  return port;
}
function readLock(file) {
  try { return json(file); }
  catch (error) { if (error.code === "ENOENT") return null; throw error; }
}
function removeLock(file) {
  try { fs.unlinkSync(file); }
  catch (error) { if (error.code !== "ENOENT") throw error; }
}
export function takeLock(instance) {
  const file = path.join(instance.dir, "operation.lock");
  const old = readLock(file);
  if (old) {
    if (alive(old.pid) || alive(old.backendPid)) throw Error("INSTANCE_BUSY");
    removeLock(file);
  }
  const fd = fs.openSync(file, "wx"),
    id = randomUUID();
  fs.writeFileSync(fd, JSON.stringify({ id, pid: process.pid }));
  fs.closeSync(fd);
  return {
    id,
    backend(pid) {
      writeJson(file, { id, pid: process.pid, backendPid: pid });
    },
    release() {
      if (readLock(file)?.id === id) removeLock(file);
    },
  };
}
export async function withStopped(instance, action) {
  instance.assertStopped();
  const lock = takeLock(instance);
  try {
    return await action();
  } finally {
    lock.release();
  }
}
// Rebuilding on one OmniRoute version keeps the schema: an instance follows the installed package.
// Moving to another upstream version still goes through the explicit upgrade command.
const upstreamOf = (releaseId) => /^fusion-v[\d.]+-/.exec(String(releaseId))?.[0] ?? null;
const sameBase = (a, b) => a === b || (upstreamOf(a) !== null && upstreamOf(a) === upstreamOf(b));
export class Instance {
  constructor(dir) {
    this.dir = isolatedPath(dir);
    this.meta = json(path.join(this.dir, "instance.json"));
    const id = this.meta.id;
    if (
      this.meta.kind !== "loadout-fusion-isolated-v1" ||
      !/^[a-f0-9-]{36}$/.test(id) ||
      this.meta.service !== "loadout-test:omniroute-phase3-" + id ||
      this.meta.bootstrap !== "loadout-test:fusion-bootstrap-" + id ||
      !sameBase(this.meta.baseline, manifest().releaseId)
    )
      throw Error("INSTANCE_OWNERSHIP_OR_VERSION");
    noLinks(path.join(this.dir, "data/storage.sqlite"));
    validateConfig(this.meta.config);
  }
  static async create(
    dir,
    initial,
    config = { network: "loopback-only", port: 0 },
  ) {
    verifyPackage(false);
    isolatedPath(dir);
    safeFresh(dir);
    validateConfig(config);
    fs.mkdirSync(dir, { recursive: true });
    const id = randomUUID();
    writeJson(path.join(dir, "instance.json"), {
      kind: "loadout-fusion-isolated-v1",
      id,
      service: "loadout-test:omniroute-phase3-" + id,
      bootstrap: "loadout-test:fusion-bootstrap-" + id,
      baseline: manifest().releaseId,
      config,
    });
    fs.mkdirSync(path.join(dir, "data"));
    const instance = new Instance(dir);
    await keytar.setPassword(
      instance.meta.bootstrap,
      "runtime",
      JSON.stringify(
        initial ?? {
          password: randomUUID(),
          control: randomUUID(),
          api: randomUUID(),
          jwt: randomUUID(),
          bridge: randomUUID(),
        },
      ),
    );
    return instance;
  }
  async secrets() {
    const raw = await keytar.getPassword(this.meta.bootstrap, "runtime");
    if (!raw) throw Error("BOOTSTRAP_MISSING");
    return JSON.parse(raw);
  }
  async control(operation) {
    const stateFile = path.join(this.dir, "running.json");
    if (!fs.existsSync(stateFile)) throw Error("NOT_RUNNING");
    const state = json(stateFile),
      secrets = await this.secrets();
    if (state.instance !== this.meta.id || !Number.isInteger(state.controlPort))
      throw Error("INVALID_RUNTIME_STATE");
    const response = await fetch(
      `http://127.0.0.1:${state.controlPort}/${operation}`,
      {
        method: operation === "stop" ? "POST" : "GET",
        headers: { authorization: "Bearer " + secrets.control },
        signal: AbortSignal.timeout(2000),
      },
    );
    if (!response.ok) throw Error("CONTROL_UNAVAILABLE");
    const body = await response.json();
    if (body.instance !== this.meta.id || body.run !== state.run)
      throw Error("CONTROL_IDENTITY_MISMATCH");
    return body;
  }
  async status() {
    try {
      return await this.control("status");
    } catch {
      const file = path.join(this.dir, "operation.lock");
      const lock = readLock(file) ?? {};
      return {
        status:
          alive(lock.pid) || alive(lock.backendPid) ? "unavailable" : "stopped",
        instance: this.meta.id,
      };
    }
  }
  async start() {
    const before = await this.status();
    if (before.status !== "stopped") throw Error("INSTANCE_ALREADY_ACTIVE");
    const child = spawn(
      path.join(packageRoot, "node.exe"),
      [path.join(toolsRoot, "supervisor.mjs"), this.dir],
      {
        cwd: packageRoot,
        env: {
          ...cleanEnv(this.dir),
          LOADOUT_USER_HOME: process.env.LOADOUT_USER_HOME ?? process.env.USERPROFILE ?? "",
          LOADOUT_USER_LOCALAPPDATA: process.env.LOADOUT_USER_LOCALAPPDATA ?? process.env.LOCALAPPDATA ?? "",
        },
        detached: true,
        windowsHide: true,
        stdio: "ignore",
      },
    );
    child.unref();
    let spawnError = false;
    child.on("error", () => {
      spawnError = true;
    });
    for (let i = 0; i < 150; i++) {
      if (spawnError) throw Error("START_FAILED");
      const state = await this.status();
      if (state.status === "ready") {
        await this.attach();
        return state;
      }
      if (child.exitCode !== null || (child.pid && !alive(child.pid)))
        throw Error("START_FAILED_SEE_STATUS");
      await sleep(400);
    }
    throw Error("START_TIMEOUT");
  }
  /** The dashboard session for the desktop shell, obtained with the vault-held machine secret. */
  async desktopSession() {
    if (!this.cookie) await this.attach();
    return this.cookie;
  }
  async attach() {
    const state = await this.control("status");
    if (state.status !== "ready") throw Error("NOT_READY");
    this.base = `http://127.0.0.1:${state.port}`;
    const secrets = await this.secrets();
    const response = await fetch(this.base + "/api/auth/login", {
      method: "POST",
      headers: { "content-type": "application/json", origin: this.base },
      body: JSON.stringify({ password: secrets.password }),
      signal: AbortSignal.timeout(10000),
    });
    if (!response.ok) throw Error("LOGIN_FAILED");
    this.cookie = response.headers.get("set-cookie").split(";")[0];
    this.csrf = (
      await (
        await fetch(this.base + "/api/auth/csrf", {
          headers: { cookie: this.cookie },
        })
      ).json()
    ).token;
  }
  async admin(url, method = "GET", body) {
    if (!this.cookie) await this.attach();
    const readOnlyQuery = method === "GET" &&
      /^\/api\/(?:pricing|models|provider-models|synced-available-models|providers\/[^/?]+\/models|usage\/(?:quota|budget|history|model-latency-stats|call-logs))\/?\?/.test(url);
    if (!url.startsWith("/api/") || (url.includes("?") && !readOnlyQuery) || url.includes("..") || url.includes("#") || url.includes("\\"))
      throw Error("ADMIN_PATH");
    const response = await fetch(this.base + url, {
      method,
      headers: {
        "content-type": "application/json",
        cookie: this.cookie,
        origin: this.base,
        "x-omniroute-csrf": this.csrf,
      },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(30000),
    });
    if (!response.ok) throw Error("ADMIN_" + response.status);
    return response.json();
  }
  async settings() {
    const settings = await this.admin("/api/settings");
    if (
      settings.requireApiKey !== true ||
      settings.requestLoggingEnabled !== false
    )
      throw Error("SAFETY_PROFILE_MISSING");
    return settings;
  }
  async addProvider(slug, baseUrl, accounts, apiType = "chat") {
    if (!["chat", "responses"].includes(apiType))
      throw Error("UPSTREAM_PROTOCOL_UNSUPPORTED");
    const url = new URL(baseUrl);
    const local = url.protocol === "http:" && url.hostname === "127.0.0.1";
    const allowed =
      url.protocol === "https:" &&
      (this.meta.config.network === "direct" ||
        (this.meta.config.network === "allowlist" &&
          this.meta.config.hosts?.includes(url.hostname)));
    if ((!local && !allowed) || url.username || url.password)
      throw Error("UPSTREAM_NOT_ALLOWED");
    const { node } = await this.admin("/api/provider-nodes", "POST", {
      type: "openai-compatible",
      name: slug,
      prefix: "fusion-" + slug,
      apiType,
      baseUrl,
    });
    const connections = [];
    for (const [index, account] of accounts.entries()) {
      const { connection } = await this.admin("/api/providers", "POST", {
        provider: node.id,
        name: account.alias,
        apiKey: account.key,
        priority: index + 1,
        providerSpecificData: { baseUrl, apiType },
      });
      connections.push(connection.id);
      await this.admin("/api/providers/" + connection.id, "PUT", {
        isActive: true,
        testStatus: "active",
      });
    }
    const client = await this.admin("/api/keys", "POST", {
      name: slug,
      noLog: true,
      allowedConnections: connections,
    });
    await keytar.setPassword(this.meta.bootstrap, "client-" + slug, client.key);
    return {
      slug,
      provider: node.id,
      connections,
      keyId: client.id,
      key: client.key,
      model: (alias, model) => node.id + "/" + alias + "/" + model,
    };
  }
  assertStopped(ownOperation = false) {
    const file = path.join(this.dir, "operation.lock");
    const lock = readLock(file);
    if (lock) {
      if (
        alive(lock.backendPid) ||
        (alive(lock.pid) && !(ownOperation && lock.pid === process.pid))
      )
        throw Error("STOP_BEFORE_OPERATION");
    }
  }
  async stop() {
    const status = await this.status();
    if (status.status === "stopped") return;
    await this.control("stop");
    for (let i = 0; i < 100; i++) {
      if ((await this.status()).status === "stopped") {
        this.cookie = null;
        return;
      }
      await sleep(100);
    }
    throw Error("STOP_TIMEOUT");
  }
  async cleanup() {
    await this.stop();
    return withStopped(this, async () => {
      for (const service of [this.meta.service, this.meta.bootstrap])
        for (const entry of await keytar.findCredentials(service))
          await keytar.deletePassword(service, entry.account);
      return (
        (await keytar.findCredentials(this.meta.service)).length +
        (await keytar.findCredentials(this.meta.bootstrap)).length
      );
    });
  }
}
