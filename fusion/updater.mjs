// SPDX-License-Identifier: Apache-2.0
// Finds, downloads and verifies a new release, then hands over to apply-update.ps1 (ADR-020).
// A short-lived process of its own: the backend's egress guard stays as it is, and this file
// depends on Node's built-ins only, so a broken package cannot stop it from replacing itself.
import { createHash } from "node:crypto";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { Readable, Transform } from "node:stream";
import { pipeline } from "node:stream/promises";
import { fileURLToPath } from "node:url";
import { createInflateRaw } from "node:zlib";

const REPOSITORY = "https://github.com/SeaserL/Loadout";
const HOSTS = new Set(["github.com", "release-assets.githubusercontent.com", "objects.githubusercontent.com"]);
const USER_AGENT = "Loadout-Updater";
const MAX_REDIRECTS = 5;
const MAX_PACKAGE = 1024 * 1024 * 1024;
const MAX_EXPANDED = 8 * 1024 * 1024 * 1024;
const REQUIRED = ["node.exe", "Loadout.exe", "tools/desktop.mjs", "server.js"];

export const assetName = (version) => `Loadout-${version}-portable-windows-x64.zip`;

export function parseVersion(text) {
  const parts = String(text).replace(/^v/, "").split(".");
  if (parts.length !== 3 || !parts.every((p) => /^(0|[1-9]\d*)$/.test(p))) return null;
  return parts.map(Number);
}
export function isNewer(candidate, current) {
  const [a, b] = [parseVersion(candidate), parseVersion(current)];
  if (!a || !b) return false;
  for (let i = 0; i < 3; i++) if (a[i] !== b[i]) return a[i] > b[i];
  return false;
}
export function parseDigest(text, name) {
  const words = String(text).trim().split(/\s+/);
  if (words.length !== 2 || words[1] !== name || !/^[0-9a-fA-F]{64}$/.test(words[0])) throw Error("CHECKSUM_INVALID");
  return words[0].toLowerCase();
}

/** A test feed can only ever be this machine. */
export function feedOrigin(feed) {
  if (!feed) return null;
  const url = new URL(feed);
  if (url.protocol !== "http:" || url.hostname !== "127.0.0.1") throw Error("FEED_NOT_LOCAL");
  return url.origin;
}
function assertAllowed(url, feed) {
  const parsed = new URL(url);
  if (feed && parsed.origin === feedOrigin(feed)) return parsed;
  if (feed) throw Error("UNTRUSTED_HOST");
  if (parsed.protocol !== "https:" || !HOSTS.has(parsed.hostname) || parsed.username || parsed.password) throw Error("UNTRUSTED_HOST");
  return parsed;
}
/** Redirects are followed by hand, so every hop is checked before anything is sent to it. */
async function request(url, { feed, onHost, range } = {}) {
  let current = url;
  for (let hop = 0; hop <= MAX_REDIRECTS; hop++) {
    const parsed = assertAllowed(current, feed);
    onHost?.(parsed.hostname);
    const response = await fetch(current, {
      redirect: "manual",
      headers: { "user-agent": USER_AGENT, ...(range ? { range } : {}) },
      signal: AbortSignal.timeout(600000),
    });
    if (response.status >= 300 && response.status < 400) {
      const next = response.headers.get("location");
      await response.body?.cancel();
      if (!next) throw Error("BAD_REDIRECT");
      current = new URL(next, current).href;
      continue;
    }
    if (!response.ok) {
      await response.body?.cancel();
      throw Error("HTTP_" + response.status);
    }
    return response;
  }
  throw Error("TOO_MANY_REDIRECTS");
}
async function readLimited(response, limit) {
  const chunks = [];
  let total = 0;
  for await (const chunk of response.body) {
    total += chunk.length;
    if (total > limit) throw Error("RESPONSE_TOO_LARGE");
    chunks.push(chunk);
  }
  return Buffer.concat(chunks).toString("utf8");
}

/**
 * github.com answers /releases/latest with a redirect to the newest stable release, which is all
 * this needs. Not the API: its anonymous quota is per address, and on a shared network other
 * programs use it up. A draft or prerelease is never "latest".
 */
export function parseLatestLocation(location, base = REPOSITORY) {
  const prefix = base + "/releases/tag/v";
  if (!String(location).startsWith(prefix)) throw Error("VERSION_INVALID");
  const version = location.slice(prefix.length);
  if (!parseVersion(version)) throw Error("VERSION_INVALID");
  return version;
}
const baseOf = (feed) => (feed ? feedOrigin(feed) : REPOSITORY);
export async function latestVersion(feed, onHost) {
  const url = baseOf(feed) + "/releases/latest";
  onHost?.(assertAllowed(url, feed).hostname);
  const response = await fetch(url, { redirect: "manual", headers: { "user-agent": USER_AGENT }, signal: AbortSignal.timeout(60000) });
  await response.body?.cancel();
  if (response.status >= 400) throw Error("HTTP_" + response.status);
  return parseLatestLocation(new URL(response.headers.get("location") ?? "", url).href, baseOf(feed));
}
/** The exact addresses a release's assets must come from. */
export function assetsFor(version, feed) {
  const name = assetName(version);
  const tag = `${baseOf(feed)}/releases/download/v${version}/`;
  return { version, name, zipUrl: tag + name, checksumUrl: tag + name + ".sha256", releaseUrl: `${baseOf(feed)}/releases/tag/v${version}` };
}
/** One byte is enough to learn the size, and that the asset exists at all. */
export async function probeSize(url, { feed, onHost } = {}) {
  const response = await request(url, { feed, onHost, range: "bytes=0-0" });
  await response.body?.cancel();
  const size = Number(/\/(\d+)$/.exec(response.headers.get("content-range") ?? "")?.[1] ?? response.headers.get("content-length"));
  if (!(size > 0) || size > MAX_PACKAGE) throw Error("ASSET_SIZE");
  return size;
}

/** Streams to a temporary file; it only gets its real name once size and hash both match. */
export async function downloadVerified(url, file, { size, sha256, feed, onHost, onProgress }) {
  const partial = file + ".part";
  const response = await request(url, { feed, onHost });
  const hash = createHash("sha256");
  let total = 0;
  try {
    await pipeline(
      Readable.fromWeb(response.body),
      new Transform({
        transform(chunk, _encoding, done) {
          total += chunk.length;
          if (total > size || total > MAX_PACKAGE) return done(Error("DOWNLOAD_SIZE"));
          hash.update(chunk);
          onProgress?.(total);
          done(null, chunk);
        },
      }),
      fs.createWriteStream(partial),
    );
    if (total !== size) throw Error("DOWNLOAD_SIZE");
    if (hash.digest("hex") !== sha256) throw Error("CHECKSUM_MISMATCH");
    fs.rmSync(file, { force: true });
    fs.renameSync(partial, file);
  } catch (error) {
    fs.rmSync(partial, { force: true });
    throw error;
  }
}

const EOCD = 0x06054b50;
const CENTRAL = 0x02014b50;
const LOCAL = 0x04034b50;
const S_IFLNK = 0o120000;
function readEntries(fd, length) {
  const tail = Math.min(length, 65557);
  const end = Buffer.alloc(tail);
  fs.readSync(fd, end, 0, tail, length - tail);
  let at = -1;
  for (let i = tail - 22; i >= 0; i--) if (end.readUInt32LE(i) === EOCD) { at = i; break; }
  if (at < 0) throw Error("ZIP_INVALID");
  const count = end.readUInt16LE(at + 10);
  const size = end.readUInt32LE(at + 12);
  const offset = end.readUInt32LE(at + 16);
  if (count === 0xffff || size === 0xffffffff || offset === 0xffffffff) throw Error("ZIP64_REFUSED");
  if (offset + size > length) throw Error("ZIP_INVALID");
  const directory = Buffer.alloc(size);
  fs.readSync(fd, directory, 0, size, offset);
  const entries = [];
  let p = 0;
  for (let n = 0; n < count; n++) {
    if (p + 46 > size || directory.readUInt32LE(p) !== CENTRAL) throw Error("ZIP_INVALID");
    const flags = directory.readUInt16LE(p + 8);
    const method = directory.readUInt16LE(p + 10);
    const compressed = directory.readUInt32LE(p + 20);
    const expanded = directory.readUInt32LE(p + 24);
    const nameLength = directory.readUInt16LE(p + 28);
    const extraLength = directory.readUInt16LE(p + 30);
    const commentLength = directory.readUInt16LE(p + 32);
    const attributes = directory.readUInt32LE(p + 38);
    const local = directory.readUInt32LE(p + 42);
    const name = directory.toString("utf8", p + 46, p + 46 + nameLength);
    if (compressed === 0xffffffff || expanded === 0xffffffff || local === 0xffffffff) throw Error("ZIP64_REFUSED");
    if (flags & 1) throw Error("ZIP_ENCRYPTED");
    if (method !== 0 && method !== 8) throw Error("ZIP_METHOD");
    if ((attributes >>> 16 & 0o170000) === S_IFLNK) throw Error("ZIP_LINK");
    entries.push({ name, method, compressed, expanded, local });
    p += 46 + nameLength + extraLength + commentLength;
  }
  return entries;
}
function safeTarget(root, name) {
  if (!name || name.includes("\\") || name.includes(":") || name.includes("\0") || name.startsWith("/")) throw Error("ZIP_PATH");
  const parts = name.replace(/\/$/, "").split("/");
  if (parts.some((part) => part === "" || part === "." || part === "..")) throw Error("ZIP_PATH");
  const target = path.resolve(root, ...parts);
  if (!target.startsWith(path.resolve(root) + path.sep)) throw Error("ZIP_PATH");
  return target;
}
/** Every path is judged before anything is written, so a bad archive leaves nothing behind. */
export async function extractZip(zip, destination, onProgress) {
  const fd = fs.openSync(zip, "r");
  try {
    const length = fs.fstatSync(fd).size;
    const entries = readEntries(fd, length);
    const seen = new Set();
    let total = 0;
    const planned = entries.map((entry) => {
      const target = safeTarget(destination, entry.name);
      const key = target.toLowerCase();
      if (seen.has(key)) throw Error("ZIP_DUPLICATE");
      seen.add(key);
      total += entry.expanded;
      if (total > MAX_EXPANDED) throw Error("ZIP_TOO_LARGE");
      return { ...entry, target };
    });
    fs.mkdirSync(destination, { recursive: true });
    let done = 0;
    for (const entry of planned) {
      if (entry.name.endsWith("/")) {
        fs.mkdirSync(entry.target, { recursive: true });
        continue;
      }
      fs.mkdirSync(path.dirname(entry.target), { recursive: true });
      const header = Buffer.alloc(30);
      fs.readSync(fd, header, 0, 30, entry.local);
      if (header.readUInt32LE(0) !== LOCAL) throw Error("ZIP_INVALID");
      const start = entry.local + 30 + header.readUInt16LE(26) + header.readUInt16LE(28);
      if (start + entry.compressed > length) throw Error("ZIP_INVALID");
      let written = 0;
      const stages = [
        entry.compressed
          ? fs.createReadStream(zip, { start, end: start + entry.compressed - 1, autoClose: true })
          : Readable.from([]),
        ...(entry.method === 8 ? [createInflateRaw()] : []),
        new Transform({
          transform(chunk, _encoding, callback) {
            written += chunk.length;
            callback(written > entry.expanded ? Error("ZIP_SIZE") : null, chunk);
          },
        }),
        fs.createWriteStream(entry.target),
      ];
      await pipeline(stages);
      if (written !== entry.expanded) throw Error("ZIP_SIZE");
      onProgress?.(++done, planned.length);
    }
  } finally {
    fs.closeSync(fd);
  }
}
const hashFile = async (file) => {
  const hash = createHash("sha256");
  for await (const chunk of fs.createReadStream(file)) hash.update(chunk);
  return hash.digest("hex");
};
function listFiles(root, base = root, out = []) {
  for (const entry of fs.readdirSync(root, { withFileTypes: true })) {
    const full = path.join(root, entry.name);
    if (entry.isSymbolicLink()) throw Error("PACKAGE_LINK");
    if (entry.isDirectory()) listFiles(full, base, out);
    else out.push(path.relative(base, full).replaceAll("\\", "/"));
  }
  return out;
}
/** The package must be exactly what its own manifest says, and the version we meant to install. */
export async function verifyPackageTree(root, { version, upstreamVersion, onProgress }) {
  const manifestFile = path.join(root, "fusion-manifest.json");
  if (!fs.existsSync(manifestFile)) throw Error("MANIFEST_MISSING");
  const manifest = JSON.parse(fs.readFileSync(manifestFile, "utf8"));
  if (createHash("sha256").update(JSON.stringify(manifest.files)).digest("hex") !== manifest.treeHash) throw Error("MANIFEST_INVALID");
  if (manifest.productVersion !== version) throw Error("PACKAGE_VERSION");
  // An instance only follows a package on the same runtime; another one needs the upgrade command.
  if (upstreamVersion && manifest.upstreamVersion !== upstreamVersion) throw Error("UPSTREAM_CHANGED");
  const expected = new Map(manifest.files.map((f) => [f.path, f.sha256]));
  for (const required of REQUIRED) if (!expected.has(required)) throw Error("PACKAGE_INCOMPLETE");
  const actual = listFiles(root).filter((f) => f !== "fusion-manifest.json");
  if (actual.length !== expected.size || actual.some((f) => !expected.has(f))) throw Error("PACKAGE_FILES");
  let done = 0;
  for (const [file, sha] of expected) {
    if ((await hashFile(path.join(root, file))) !== sha) throw Error("PACKAGE_CONTENT");
    onProgress?.(++done, expected.size);
  }
  return manifest;
}

/** The backend's environment is cleaned, so the system proxy has to be read from where it lives. */
export function systemProxy() {
  const done = spawnSync("reg.exe", ["query", "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings"], { encoding: "utf8", windowsHide: true, timeout: 5000 });
  return done.status === 0 ? parseProxy(done.stdout) : null;
}
export function parseProxy(registry) {
  if (!/ProxyEnable\s+REG_DWORD\s+0x0*1\b/i.test(registry)) return null;
  const server = /ProxyServer\s+REG_SZ\s+(\S+)/i.exec(registry)?.[1];
  if (!server) return null;
  // Either "host:port", or per-scheme entries like "http=host:port;https=host:port".
  const https = /(?:^|;)https=([^;]+)/i.exec(server)?.[1];
  const any = server.includes("=") ? /(?:^|;)http=([^;]+)/i.exec(server)?.[1] : server;
  const chosen = https ?? any;
  if (!chosen) return null;
  return /^[a-z][a-z0-9+.-]*:\/\//i.test(chosen) ? chosen : "http://" + chosen;
}

const here = path.dirname(fileURLToPath(import.meta.url));

export class Updater {
  /** `root` holds the program folder; everything temporary stays beside it, on the same disk. */
  constructor({ packageRoot, instanceDir, feed, helper = path.join(here, "apply-update.ps1") }) {
    this.packageRoot = path.resolve(packageRoot);
    this.instanceDir = instanceDir;
    this.feed = feed;
    this.helper = helper;
    this.dir = path.join(path.dirname(this.packageRoot), "loadout-update");
    this.statusFile = path.join(this.dir, "status.json");
    this.manifest = JSON.parse(fs.readFileSync(path.join(this.packageRoot, "fusion-manifest.json"), "utf8"));
    this.current = this.manifest.productVersion;
    this.seenHosts = new Set();
  }
  status() {
    try {
      return JSON.parse(fs.readFileSync(this.statusFile, "utf8"));
    } catch {
      return {};
    }
  }
  write(patch) {
    fs.mkdirSync(this.dir, { recursive: true });
    const next = { ...this.status(), ...patch, pid: process.pid, updated_at_ms: Date.now() };
    const temporary = this.statusFile + "." + process.pid + ".tmp";
    fs.writeFileSync(temporary, JSON.stringify(next));
    fs.renameSync(temporary, this.statusFile);
    return next;
  }
  /** Host names only, in the same log as the gateway's, so the owner can see every connection. */
  noteHost = (host) => {
    if (this.seenHosts.has(host)) return;
    this.seenHosts.add(host);
    if (!this.instanceDir) return;
    try {
      fs.appendFileSync(path.join(this.instanceDir, "egress.log"), `${new Date().toISOString()} ${host}\n`);
    } catch {}
  };
  async find() {
    // Stamped before asking: a failed attempt counts too, or every status read would try again.
    this.write({ checked_at_ms: Date.now() });
    const version = await latestVersion(this.feed, this.noteHost);
    if (!isNewer(version, this.current)) return { assets: null, version };
    const assets = assetsFor(version, this.feed);
    return { assets: { ...assets, size: await probeSize(assets.zipUrl, { feed: this.feed, onHost: this.noteHost }) }, version };
  }
  async check() {
    this.write({ phase: "checking", error: null });
    const { assets, version } = await this.find();
    if (!assets) return this.write({ phase: "current", latest_version: version, release_url: assetsFor(version, this.feed).releaseUrl });
    // A verified download of this same version is still good; do not ask for it again.
    const ready = this.status().ready_version === version && fs.existsSync(this.stagedDir(version));
    return this.write({ phase: ready ? "ready" : "available", latest_version: version, size: assets.size, release_url: assets.releaseUrl });
  }
  stagedDir(version) {
    return path.join(this.dir, "new-" + version);
  }
  async download() {
    this.write({ phase: "checking", error: null, progress: null });
    const { assets, version } = await this.find();
    if (!assets) return this.write({ phase: "current", latest_version: version });
    const staged = this.stagedDir(version);
    const zip = path.join(this.dir, assets.name);
    this.write({ phase: "downloading", latest_version: version, size: assets.size, release_url: assets.releaseUrl, progress: { done: 0, total: assets.size } });
    const checksum = await readLimited(await request(assets.checksumUrl, { feed: this.feed, onHost: this.noteHost }), 1024);
    const sha256 = parseDigest(checksum, assets.name);
    let last = 0;
    await downloadVerified(assets.zipUrl, zip, {
      size: assets.size,
      sha256,
      feed: this.feed,
      onHost: this.noteHost,
      onProgress: (done) => {
        if (Date.now() - last < 500) return;
        last = Date.now();
        this.write({ progress: { done, total: assets.size } });
      },
    });
    const every = (phase) => (done, total) => {
      if (Date.now() - last < 500) return;
      last = Date.now();
      this.write({ phase, progress: { done, total } });
    };
    // Only one version waits here: an older one nobody installed would otherwise stay on disk forever.
    for (const name of fs.readdirSync(this.dir)) {
      if (name.startsWith("new-")) fs.rmSync(path.join(this.dir, name), { recursive: true, force: true });
    }
    try {
      this.write({ phase: "extracting", progress: null });
      await extractZip(zip, staged, every("extracting"));
      this.write({ phase: "verifying", progress: null });
      await verifyPackageTree(staged, { version, upstreamVersion: this.manifest.upstreamVersion, onProgress: every("verifying") });
    } catch (error) {
      fs.rmSync(staged, { recursive: true, force: true });
      throw error;
    } finally {
      fs.rmSync(zip, { force: true });
    }
    return this.write({ phase: "ready", ready_version: version, progress: null });
  }
  /** Starts the helper from a copy outside the folder it is about to replace, then gets out of the way. */
  async install({ testArgs = [] } = {}) {
    const status = this.status();
    const version = status.ready_version;
    const staged = version && this.stagedDir(version);
    if (status.phase !== "ready" || !staged || !fs.existsSync(staged)) throw Error("NOTHING_READY");
    if (!isNewer(version, this.current)) throw Error("NOT_NEWER");
    const helper = path.join(this.dir, "apply-update.ps1");
    fs.copyFileSync(this.helper, helper);
    this.write({ phase: "installing", error: null, progress: null });
    const system = path.join(process.env.SystemRoot ?? "C:\\Windows", "System32");
    const child = spawn(path.join(system, "WindowsPowerShell", "v1.0", "powershell.exe"), [
      "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", helper,
      "-Package", this.packageRoot,
      "-Staged", staged,
      "-Instance", this.instanceDir,
      "-UpdateDir", this.dir,
      "-From", this.current,
      "-To", version,
      ...testArgs,
    // Not detached: without a console PowerShell exits at once, doing nothing and reporting success.
    // Node ends this child when it exits, so the script relaunches itself; wait until it has.
    // Windows locks a process's working directory, so it is not one of the owner's folders.
    ], { stdio: "ignore", windowsHide: true, cwd: system });
    await new Promise((resolve, reject) => {
      child.once("error", reject);
      child.once("exit", (code) => (code === 0 ? resolve() : reject(Error("HELPER_FAILED"))));
    });
  }
  /** What a failure leaves behind. A verified download stays usable: installing needs no network. */
  fail(action, error) {
    const { code, message } = describe(error);
    const waiting = action === "check" ? this.status().ready_version : null;
    const ready = waiting && fs.existsSync(this.stagedDir(waiting));
    return this.write({ phase: ready ? "ready" : "error", ...(ready ? { latest_version: waiting } : {}), error: message, error_code: code, progress: null });
  }
}

/** What the owner sees. Codes stay in the log; no upstream text reaches the status file. */
const MESSAGES = {
  HTTP_404: "这个版本还没有可用的安装包",
  VERSION_INVALID: "没法确认最新版本，请稍后再试",
  UPSTREAM_CHANGED: "这个版本更换了运行时，不能自动更新，请按 Release 说明手动升级",
  CHECKSUM_MISMATCH: "下载的文件校验失败，请重试",
  DOWNLOAD_SIZE: "下载的文件大小不对，请重试",
  NOTHING_READY: "没有准备好的更新",
  HELPER_FAILED: "没能开始安装，旧版本没有变动，请重试",
};
export function describe(error) {
  const code = /^[A-Z][A-Z0-9_]{2,40}$/.test(error?.message ?? "") ? error.message : null;
  if (code && MESSAGES[code]) return { code, message: MESSAGES[code] };
  if (code?.startsWith("HTTP_") || !code) return { code: code ?? "NETWORK", message: "连不上 GitHub，请检查网络后重试" };
  // Everything else means the download itself is not what it should be.
  return { code, message: "更新包没有通过校验，已放弃" };
}

async function main() {
  const [action, packageRoot, instanceDir, feed] = process.argv.slice(2);
  if (!["check", "download", "install"].includes(action) || !packageRoot || !instanceDir) throw Error("UPDATER_ARGUMENTS");
  const updater = new Updater({ packageRoot, instanceDir, feed: feed || undefined });
  const lock = path.join(updater.dir, "updater.lock");
  fs.mkdirSync(updater.dir, { recursive: true });
  try {
    const held = Number(fs.readFileSync(lock, "utf8"));
    try {
      process.kill(held, 0);
      return;
    } catch {
      fs.rmSync(lock, { force: true });
    }
  } catch {}
  let fd;
  try {
    fd = fs.openSync(lock, "wx");
  } catch {
    return;
  }
  fs.writeSync(fd, String(process.pid));
  fs.closeSync(fd);
  try {
    if (!feed) {
      const proxy = systemProxy();
      if (proxy) {
        process.env.HTTPS_PROXY = proxy;
        process.env.HTTP_PROXY = proxy;
        http.setGlobalProxyFromEnv();
      }
    }
    if (action === "check") await updater.check();
    else if (action === "download") await updater.download();
    else await updater.install();
  } catch (error) {
    updater.fail(action, error);
    process.exitCode = 1;
  } finally {
    fs.rmSync(lock, { force: true });
  }
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main();
