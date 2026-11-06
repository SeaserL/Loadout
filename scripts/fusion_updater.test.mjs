// SPDX-License-Identifier: Apache-2.0
// The 1.0 updater against a local stand-in for GitHub and hostile input (ADR-020). No real network,
// no real package, and the folder-swap helper runs in its no-process mode.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { deflateRawSync } from "node:zlib";
import {
  Updater,
  assetName,
  assetsFor,
  fullAssetName,
  packAssetName,
  describe,
  downloadVerified,
  extractZip,
  feedOrigin,
  isNewer,
  latestVersion,
  parseDigest,
  parseLatestLocation,
  parseProxy,
  parseVersion,
  probeSize,
  verifyPackageTree,
} from "../fusion/updater.mjs";

const HELPER = path.join(path.dirname(fileURLToPath(import.meta.url)), "../fusion/apply-update.ps1");
const sha = (data) => createHash("sha256").update(data).digest("hex");
const temp = (t) => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "loadout-updater-"));
  // Patient: the helper outlives the result it writes, and antivirus holds new files for a moment.
  t.after(() => fs.rmSync(dir, { recursive: true, force: true, maxRetries: 15, retryDelay: 200 }));
  return dir;
};
const REQUIRED = { "node.exe": "n", "Loadout.exe": "l", "tools/desktop.mjs": "d", "server.js": "s" };

/** A package folder that matches its own manifest, the way assemble.mjs writes one. */
function packageFiles(version, { upstream = "3.8.52", extra = {} } = {}) {
  const contents = { ...REQUIRED, ...extra, marker: version };
  const files = Object.entries(contents)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([file, text]) => ({ path: file, sha256: sha(text) }));
  const manifest = { releaseId: "fusion-test", productVersion: version, upstreamVersion: upstream, treeHash: sha(JSON.stringify(files)), files };
  return { ...contents, "fusion-manifest.json": JSON.stringify(manifest) };
}
function writePackage(dir, files) {
  for (const [name, text] of Object.entries(files)) {
    const target = path.join(dir, name);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, text);
  }
}
/** A plain zip writer: enough to hand the extractor both good archives and bad ones. */
function zip(entries, { attributes = 0, method = 8, flags = 0 } = {}) {
  const locals = [];
  const central = [];
  let offset = 0;
  for (const [name, text] of entries) {
    const raw = Buffer.from(text);
    const body = method === 8 ? deflateRawSync(raw) : raw;
    const nameBytes = Buffer.from(name);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(flags, 6);
    local.writeUInt16LE(method, 8);
    local.writeUInt32LE(body.length, 18);
    local.writeUInt32LE(raw.length, 22);
    local.writeUInt16LE(nameBytes.length, 26);
    locals.push(local, nameBytes, body);
    const entry = Buffer.alloc(46);
    entry.writeUInt32LE(0x02014b50, 0);
    entry.writeUInt16LE(flags, 8);
    entry.writeUInt16LE(method, 10);
    entry.writeUInt32LE(body.length, 20);
    entry.writeUInt32LE(raw.length, 24);
    entry.writeUInt16LE(nameBytes.length, 28);
    entry.writeUInt32LE(attributes, 38);
    entry.writeUInt32LE(offset, 42);
    central.push(entry, nameBytes);
    offset += 30 + nameBytes.length + body.length;
  }
  const directory = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(directory.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, directory, end]);
}
/** The stand-in for GitHub's release API and its download addresses. */
async function feedServer(t, handler) {
  const server = http.createServer(handler);
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  // One failed cleanup skips the hooks after it; a listener left open must not hang the whole run.
  server.unref();
  t.after(() => new Promise((resolve) => { server.closeAllConnections(); server.close(resolve); }));
  return `http://127.0.0.1:${server.address().port}`;
}
async function releaseFeed(t, version, body, { checksum } = {}) {
  const name = assetName(version);
  const tag = "v" + version;
  const origin = await feedServer(t, (req, res) => {
    if (req.url === "/releases/latest") {
      res.writeHead(302, { location: `${origin}/releases/tag/${tag}` });
      return res.end();
    }
    if (req.url === `/releases/download/${tag}/${name}.sha256`) return res.end(`${checksum ?? sha(body)}  ${name}\n`);
    if (req.url === `/releases/download/${tag}/${name}`) {
      if (req.headers.range === "bytes=0-0") {
        res.writeHead(206, { "content-range": `bytes 0-0/${body.length}`, "content-length": 1 });
        return res.end(body.subarray(0, 1));
      }
      return res.end(body);
    }
    res.writeHead(404);
    res.end();
  });
  return origin + "/";
}
function installation(t, version = "1.0.0") {
  const root = temp(t);
  const packageRoot = path.join(root, "Loadout");
  const instanceDir = path.join(root, "loadout-fusion-isolated", "main");
  writePackage(packageRoot, packageFiles(version));
  fs.mkdirSync(path.join(instanceDir, "data"), { recursive: true });
  fs.writeFileSync(path.join(instanceDir, "data", "storage.sqlite"), "the owner's data");
  return { root, packageRoot, instanceDir };
}
const packageVersion = (dir) => JSON.parse(fs.readFileSync(path.join(dir, "fusion-manifest.json"), "utf8")).productVersion;
async function until(check, ms = 30000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (check()) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw Error("timed out");
}

test("versions compare as numbers and ambiguous ones are refused", () => {
  assert.deepEqual(parseVersion("v1.2.3"), [1, 2, 3]);
  for (const bad of ["1.2", "1.2.3-beta", "01.2.3", "1..3", "", "v", "1.2.3.4"]) assert.equal(parseVersion(bad), null);
  assert(isNewer("1.0.10", "1.0.9"));
  assert(isNewer("2.0.0", "1.9.9"));
  assert(!isNewer("1.0.0", "1.0.0"), "the same version is not an update");
  assert(!isNewer("0.9.9", "1.0.0"), "never downgrade");
  assert(!isNewer("nonsense", "1.0.0"));
});

test("a checksum file must name the asset and hold one real hash", () => {
  const hash = "a".repeat(64);
  assert.equal(parseDigest(`${hash}  a.zip\n`, "a.zip"), hash);
  assert.throws(() => parseDigest(`${hash}  b.zip`, "a.zip"), /CHECKSUM_INVALID/);
  assert.throws(() => parseDigest("short  a.zip", "a.zip"), /CHECKSUM_INVALID/);
  assert.throws(() => parseDigest(`${hash}  a.zip  extra`, "a.zip"), /CHECKSUM_INVALID/);
});

test("the test feed can only be this machine", () => {
  assert.equal(feedOrigin("http://127.0.0.1:9/latest"), "http://127.0.0.1:9");
  for (const bad of ["https://127.0.0.1:9/x", "http://localhost:9/x", "http://example.com/x", "http://10.0.0.1/x"]) {
    assert.throws(() => feedOrigin(bad), /FEED_NOT_LOCAL/);
  }
});

test("the latest version comes from where github.com redirects, and nowhere else", () => {
  const base = "https://github.com/SeaserL/Loadout";
  assert.equal(parseLatestLocation(`${base}/releases/tag/v1.2.3`), "1.2.3");
  for (const bad of [
    `${base}/releases`,
    `${base}/releases/tag/v1.2`,
    `${base}/releases/tag/v1.2.3-beta`,
    `${base}/releases/tag/nightly`,
    "https://github.com/someone/else/releases/tag/v9.9.9",
    "https://evil.example/SeaserL/Loadout/releases/tag/v9.9.9",
    "",
  ]) assert.throws(() => parseLatestLocation(bad), /VERSION_INVALID/, bad);
});

test("assets have one exact name and address per version", () => {
  // 1.1 (ExecPlan 071): updates download the lite app; the full archive keeps 1.0's name for new
  // downloads and for 1.0's updater; the AI pack has its own version.
  const assets = assetsFor("1.2.3");
  assert.equal(assets.name, "Loadout-1.2.3-lite-portable-windows-x64.zip");
  assert.equal(assets.zipUrl, "https://github.com/SeaserL/Loadout/releases/download/v1.2.3/Loadout-1.2.3-lite-portable-windows-x64.zip");
  assert.equal(assets.checksumUrl, assets.zipUrl + ".sha256");
  assert.equal(fullAssetName("1.2.3"), "Loadout-1.2.3-portable-windows-x64.zip");
  const pack = assetsFor("1.2.3", undefined, packAssetName("4"));
  assert.equal(pack.zipUrl, "https://github.com/SeaserL/Loadout/releases/download/v1.2.3/Loadout-ai-4-windows-x64.zip");
  // The name 0.4's updater looks for would make it download this package and fail to install it.
  for (const name of [assets.name, fullAssetName("1.2.3"), pack.name]) assert.notEqual(name, "Loadout-1.2.3-windows-x64.zip");
});

test("the size comes from the server and has a limit", async (t) => {
  const answers = {
    "/range": (res) => { res.writeHead(206, { "content-range": "bytes 0-0/1234", "content-length": 1 }); res.end("x"); },
    "/plain": (res) => res.end("twelve bytes"),
    "/huge": (res) => { res.writeHead(206, { "content-range": `bytes 0-0/${2 ** 31}`, "content-length": 1 }); res.end("x"); },
    "/empty": (res) => res.end(),
    "/gone": (res) => { res.writeHead(404); res.end(); },
  };
  const feed = await feedServer(t, (req, res) => answers[req.url](res));
  const size = (name) => probeSize(feed + name, { feed: feed + "/" });
  assert.equal(await size("/range"), 1234);
  assert.equal(await size("/plain"), 12);
  await assert.rejects(size("/huge"), /ASSET_SIZE/);
  await assert.rejects(size("/empty"), /ASSET_SIZE/);
  await assert.rejects(size("/gone"), /HTTP_404/);
});

test("a redirect to another host is refused before anything is sent to it", async (t) => {
  let reached = false;
  const elsewhere = await feedServer(t, (_req, res) => { reached = true; res.end("x"); });
  const feed = await feedServer(t, (_req, res) => { res.writeHead(302, { location: elsewhere + "/x" }); res.end(); });
  await assert.rejects(probeSize(feed + "/asset", { feed: feed + "/" }), /UNTRUSTED_HOST/);
  await assert.rejects(latestVersion(feed + "/"), /VERSION_INVALID/, "a latest redirect to another host is not followed at all");
  assert.equal(reached, false);
});

test("a redirect loop ends", async (t) => {
  const feed = await feedServer(t, (req, res) => { res.writeHead(302, { location: req.url + "x" }); res.end(); });
  await assert.rejects(probeSize(feed + "/a", { feed: feed + "/" }), /TOO_MANY_REDIRECTS/);
});

test("a download only gets its name when size and hash match", async (t) => {
  const dir = temp(t);
  const body = Buffer.from("the package");
  const feed = await feedServer(t, (_req, res) => res.end(body));
  const file = path.join(dir, "a.zip");
  const options = { feed: feed + "/", size: body.length, sha256: sha(body) };
  await assert.rejects(downloadVerified(feed + "/a", file, { ...options, sha256: "0".repeat(64) }), /CHECKSUM_MISMATCH/);
  await assert.rejects(downloadVerified(feed + "/a", file, { ...options, size: body.length + 1 }), /DOWNLOAD_SIZE/);
  await assert.rejects(downloadVerified(feed + "/a", file, { ...options, size: body.length - 1 }), /DOWNLOAD_SIZE/);
  assert.deepEqual(fs.readdirSync(dir), [], "nothing is left behind");
  await downloadVerified(feed + "/a", file, options);
  assert.equal(fs.readFileSync(file, "utf8"), "the package");
});

test("a bad archive writes nothing at all", async (t) => {
  const cases = [
    ["../escape", /ZIP_PATH/],
    ["a/../../escape", /ZIP_PATH/],
    ["/absolute", /ZIP_PATH/],
    ["C:/absolute", /ZIP_PATH/],
    ["a\\b", /ZIP_PATH/],
    ["a//b", /ZIP_PATH/],
  ];
  for (const [name, expected] of cases) {
    const dir = temp(t);
    const archive = path.join(dir, "a.zip");
    fs.writeFileSync(archive, zip([["good.txt", "fine"], [name, "bad"]]));
    const out = path.join(dir, "out");
    await assert.rejects(extractZip(archive, out), expected, name);
    assert(!fs.existsSync(out), `${name} wrote something`);
    assert(!fs.existsSync(path.join(dir, "escape")));
  }
  const dir = temp(t);
  const archive = path.join(dir, "a.zip");
  const out = path.join(dir, "out");
  fs.writeFileSync(archive, zip([["A.txt", "1"], ["a.txt", "2"]]));
  await assert.rejects(extractZip(archive, out), /ZIP_DUPLICATE/);
  fs.writeFileSync(archive, zip([["link", "target"]], { attributes: (0o120777 << 16) >>> 0 }));
  await assert.rejects(extractZip(archive, out), /ZIP_LINK/);
  fs.writeFileSync(archive, zip([["a", "b"]], { flags: 1 }));
  await assert.rejects(extractZip(archive, out), /ZIP_ENCRYPTED/);
  fs.writeFileSync(archive, zip([["a", "b"]], { method: 12 }));
  await assert.rejects(extractZip(archive, out), /ZIP_METHOD/);
  fs.writeFileSync(archive, "not a zip file at all, and long enough to look for a directory in it");
  await assert.rejects(extractZip(archive, out), /ZIP_INVALID/);
  assert(!fs.existsSync(out));
});

test("a good archive comes out byte for byte, stored or deflated", async (t) => {
  const dir = temp(t);
  const archive = path.join(dir, "a.zip");
  const text = "x".repeat(70000);
  fs.writeFileSync(archive, zip([["deep/er/file.txt", text], ["empty", ""]]));
  await extractZip(archive, path.join(dir, "deflated"));
  assert.equal(fs.readFileSync(path.join(dir, "deflated/deep/er/file.txt"), "utf8"), text);
  assert.equal(fs.readFileSync(path.join(dir, "deflated/empty"), "utf8"), "");
  fs.writeFileSync(archive, zip([["file.txt", "plain"]], { method: 0 }));
  await extractZip(archive, path.join(dir, "stored"));
  assert.equal(fs.readFileSync(path.join(dir, "stored/file.txt"), "utf8"), "plain");
});

test("a package must be exactly what its manifest says", async (t) => {
  const fresh = () => {
    const dir = path.join(temp(t), "p");
    writePackage(dir, packageFiles("1.0.1"));
    return dir;
  };
  const wanted = { version: "1.0.1", upstreamVersion: "3.8.52" };
  await verifyPackageTree(fresh(), wanted);
  let dir = fresh();
  fs.writeFileSync(path.join(dir, "added.js"), "planted");
  await assert.rejects(verifyPackageTree(dir, wanted), /PACKAGE_FILES/);
  dir = fresh();
  fs.rmSync(path.join(dir, "marker"));
  await assert.rejects(verifyPackageTree(dir, wanted), /PACKAGE_FILES/);
  dir = fresh();
  fs.writeFileSync(path.join(dir, "server.js"), "changed");
  await assert.rejects(verifyPackageTree(dir, wanted), /PACKAGE_CONTENT/);
  await assert.rejects(verifyPackageTree(fresh(), { ...wanted, version: "1.0.2" }), /PACKAGE_VERSION/);
  await assert.rejects(verifyPackageTree(fresh(), { ...wanted, upstreamVersion: "3.9.0" }), /UPSTREAM_CHANGED/);
  dir = fresh();
  const manifest = JSON.parse(fs.readFileSync(path.join(dir, "fusion-manifest.json"), "utf8"));
  manifest.treeHash = "0".repeat(64);
  fs.writeFileSync(path.join(dir, "fusion-manifest.json"), JSON.stringify(manifest));
  await assert.rejects(verifyPackageTree(dir, wanted), /MANIFEST_INVALID/);
  dir = fresh();
  fs.rmSync(path.join(dir, "fusion-manifest.json"));
  await assert.rejects(verifyPackageTree(dir, wanted), /MANIFEST_MISSING/);
});

test("the system proxy is read in both of its forms", () => {
  const registry = (enable, server) => `    ProxyEnable    REG_DWORD    ${enable}\n    ProxyServer    REG_SZ    ${server}\n`;
  assert.equal(parseProxy(registry("0x1", "127.0.0.1:7890")), "http://127.0.0.1:7890");
  assert.equal(parseProxy(registry("0x1", "http=a:1;https=b:2")), "http://b:2");
  assert.equal(parseProxy(registry("0x1", "http=a:1;ftp=c:3")), "http://a:1");
  assert.equal(parseProxy(registry("0x1", "socks5://s:9")), "socks5://s:9");
  assert.equal(parseProxy(registry("0x0", "127.0.0.1:7890")), null, "switched off");
  assert.equal(parseProxy("    ProxyEnable    REG_DWORD    0x1\n"), null, "on, but no server");
});

test("errors reach the owner as plain messages, without upstream text", () => {
  assert.match(describe(Error("UPSTREAM_CHANGED")).message, /手动升级/);
  assert.match(describe(Error("HTTP_503")).message, /GitHub/);
  assert.match(describe(Error("HTTP_404")).message, /安装包/);
  assert.match(describe(Error("fetch failed: secret upstream detail")).message, /GitHub/);
  assert(!describe(Error("fetch failed: secret upstream detail")).message.includes("secret"));
  assert.match(describe(Error("ZIP_PATH")).message, /校验/);
});

test("checking finds a newer release and ignores the same or an older one", async (t) => {
  const { packageRoot, instanceDir } = installation(t);
  const check = async (version) => {
    const feed = await releaseFeed(t, version, Buffer.from("zip"));
    return new Updater({ packageRoot, instanceDir, feed, helper: HELPER }).check();
  };
  const newer = await check("1.0.1");
  assert.equal(newer.phase, "available");
  assert.equal(newer.latest_version, "1.0.1");
  assert(newer.checked_at_ms > 0);
  assert.equal((await check("1.0.0")).phase, "current");
  assert.equal((await check("0.9.0")).phase, "current");
  // Every host it spoke to is on record, like the gateway's.
  assert.match(fs.readFileSync(path.join(instanceDir, "egress.log"), "utf8"), /127\.0\.0\.1/);
});

test("a tampered download never becomes ready", async (t) => {
  const { packageRoot, instanceDir, root } = installation(t);
  const body = zip(Object.entries(packageFiles("1.0.1")));
  const feed = await releaseFeed(t, "1.0.1", body, { checksum: "0".repeat(64) });
  const updater = new Updater({ packageRoot, instanceDir, feed, helper: HELPER });
  await assert.rejects(updater.download(), /CHECKSUM_MISMATCH/);
  assert.notEqual(updater.status().phase, "ready");
  assert(!fs.existsSync(updater.stagedDir("1.0.1")));
  assert.equal(packageVersion(path.join(root, "Loadout")), "1.0.0");
});

test("a package that fails verification is discarded", async (t) => {
  const { packageRoot, instanceDir } = installation(t);
  const files = packageFiles("1.0.1");
  files["server.js"] = "not what the manifest says";
  const feed = await releaseFeed(t, "1.0.1", zip(Object.entries(files)));
  const updater = new Updater({ packageRoot, instanceDir, feed, helper: HELPER });
  await assert.rejects(updater.download(), /PACKAGE_CONTENT/);
  assert(!fs.existsSync(updater.stagedDir("1.0.1")));
  assert(!fs.existsSync(path.join(updater.dir, assetName("1.0.1"))), "the zip is gone too");
});

test("another runtime version is left to the manual upgrade", async (t) => {
  const { packageRoot, instanceDir } = installation(t);
  const feed = await releaseFeed(t, "2.0.0", zip(Object.entries(packageFiles("2.0.0", { upstream: "3.9.0" }))));
  const updater = new Updater({ packageRoot, instanceDir, feed, helper: HELPER });
  await assert.rejects(updater.download(), /UPSTREAM_CHANGED/);
  assert.equal(packageVersion(packageRoot), "1.0.0");
});

test("install needs a verified download", async (t) => {
  const { packageRoot, instanceDir } = installation(t);
  const updater = new Updater({ packageRoot, instanceDir, helper: HELPER });
  await assert.rejects(updater.install(), /NOTHING_READY/);
});

test("download then install swaps the folder and keeps the data", async (t) => {
  const { packageRoot, instanceDir, root } = installation(t);
  const feed = await releaseFeed(t, "1.0.1", zip(Object.entries(packageFiles("1.0.1"))));
  const updater = new Updater({ packageRoot, instanceDir, feed, helper: HELPER });
  const ready = await updater.download();
  assert.equal(ready.phase, "ready");
  assert.equal(packageVersion(packageRoot), "1.0.0", "the running version is untouched until install");
  assert(!fs.existsSync(path.join(updater.dir, assetName("1.0.1"))), "the zip is removed once unpacked");

  await updater.install({ testArgs: ["-TestNoProcess"] });
  const result = path.join(updater.dir, "result.json");
  await until(() => fs.existsSync(result) && !fs.existsSync(updater.statusFile) && !fs.existsSync(packageRoot + ".previous-1.0.0"));
  assert.equal(JSON.parse(fs.readFileSync(result, "utf8").replace(/^\uFEFF/, "")).status, "installed");
  assert.equal(packageVersion(packageRoot), "1.0.1");
  assert.equal(fs.readFileSync(path.join(instanceDir, "data/storage.sqlite"), "utf8"), "the owner's data");
  assert.deepEqual(fs.readdirSync(root).sort(), ["Loadout", "loadout-fusion-isolated", "loadout-update"]);
});

test("a failed check counts as a check, so a dead network is not retried on every status read", async (t) => {
  const { packageRoot, instanceDir } = installation(t);
  const feed = await feedServer(t, (req, res) => { res.writeHead(500); res.end(); });
  const updater = new Updater({ packageRoot, instanceDir, feed, helper: HELPER });
  const before = Date.now();
  const failure = await updater.check().catch((error) => error);
  assert.equal(failure.message, "HTTP_500");
  const status = updater.fail("check", failure);
  assert.equal(status.phase, "error");
  assert.match(status.error, /GitHub/);
  assert(status.checked_at_ms >= before);
});

test("a verified download can still be installed when GitHub cannot be reached", async (t) => {
  const { packageRoot, instanceDir } = installation(t);
  const good = await releaseFeed(t, "1.0.1", zip(Object.entries(packageFiles("1.0.1"))));
  await new Updater({ packageRoot, instanceDir, feed: good, helper: HELPER }).download();
  const dead = await feedServer(t, (req, res) => { res.writeHead(500); res.end(); });
  const updater = new Updater({ packageRoot, instanceDir, feed: dead, helper: HELPER });
  const status = updater.fail("check", await updater.check().catch((error) => error));
  assert.equal(status.phase, "ready");
  assert.equal(status.latest_version, "1.0.1");
  assert.match(status.error, /GitHub/);
});

test("a newer download replaces the one still waiting", async (t) => {
  const { packageRoot, instanceDir } = installation(t);
  const first = await releaseFeed(t, "1.0.1", zip(Object.entries(packageFiles("1.0.1"))));
  const older = new Updater({ packageRoot, instanceDir, feed: first, helper: HELPER });
  await older.download();
  assert(fs.existsSync(older.stagedDir("1.0.1")));
  const second = await releaseFeed(t, "1.0.2", zip(Object.entries(packageFiles("1.0.2"))));
  const newer = new Updater({ packageRoot, instanceDir, feed: second, helper: HELPER });
  assert.equal((await newer.download()).ready_version, "1.0.2");
  assert(!fs.existsSync(newer.stagedDir("1.0.1")), "the older version does not stay on disk");
  assert(fs.existsSync(newer.stagedDir("1.0.2")));
});

test("a helper that cannot start says so instead of leaving the owner waiting", async (t) => {
  const { packageRoot, instanceDir, root } = installation(t);
  const feed = await releaseFeed(t, "1.0.1", zip(Object.entries(packageFiles("1.0.1"))));
  const helper = path.join(root, "broken-helper.ps1");
  fs.writeFileSync(helper, "exit 3\n");
  const updater = new Updater({ packageRoot, instanceDir, feed, helper });
  await updater.download();
  const failure = await updater.install().catch((error) => error);
  assert.equal(failure.message, "HELPER_FAILED");
  const status = updater.fail("install", failure);
  assert.equal(status.phase, "error");
  assert.match(status.error, /旧版本没有变动/);
  assert.equal(packageVersion(packageRoot), "1.0.0");
});

test("a copy an earlier update could not delete does not stay or get in the way", async (t) => {
  const { packageRoot, instanceDir } = installation(t);
  const leftovers = [packageRoot + ".previous-1.0.0", packageRoot + ".previous-0.9.0"];
  for (const dir of leftovers) fs.mkdirSync(dir);
  const feed = await releaseFeed(t, "1.0.1", zip(Object.entries(packageFiles("1.0.1"))));
  const updater = new Updater({ packageRoot, instanceDir, feed, helper: HELPER });
  await updater.download();
  await updater.install({ testArgs: ["-TestNoProcess"] });
  const result = path.join(updater.dir, "result.json");
  await until(() => fs.existsSync(result) && !leftovers.some((dir) => fs.existsSync(dir)));
  assert.equal(JSON.parse(fs.readFileSync(result, "utf8").replace(/^﻿/, "")).status, "installed");
  assert.equal(packageVersion(packageRoot), "1.0.1");
});

test("the swap finishes after the process that started it is gone", async (t) => {
  // Node ends its children when it exits. In the real thing the updater exits right away, and the
  // helper has to stop the backend that started it, so it must not depend on either of them.
  const { packageRoot, instanceDir } = installation(t);
  const feed = await releaseFeed(t, "1.0.1", zip(Object.entries(packageFiles("1.0.1"))));
  const updater = new Updater({ packageRoot, instanceDir, feed, helper: HELPER });
  await updater.download();
  const script = `
    const { Updater } = await import(${JSON.stringify(new URL("../fusion/updater.mjs", import.meta.url).href)});
    const updater = new Updater(${JSON.stringify({ packageRoot, instanceDir, feed, helper: HELPER })});
    await updater.install({ testArgs: ["-TestNoProcess"] });
    process.exit(0);
  `;
  const done = spawnSync(process.execPath, ["--input-type=module", "-e", script], { encoding: "utf8", windowsHide: true, timeout: 60000 });
  assert.equal(done.status, 0, done.stderr);
  const result = path.join(updater.dir, "result.json");
  await until(() => fs.existsSync(result) && !fs.existsSync(packageRoot + ".previous-1.0.0"));
  assert.equal(JSON.parse(fs.readFileSync(result, "utf8").replace(/^\uFEFF/, "")).status, "installed");
  assert.equal(packageVersion(packageRoot), "1.0.1");
  assert.match(fs.readFileSync(path.join(updater.dir, "apply-update.log"), "utf8"), /running/);
});

test("a new version that will not start is rolled back, data included", async (t) => {
  const { packageRoot, instanceDir } = installation(t);
  const feed = await releaseFeed(t, "1.0.1", zip(Object.entries(packageFiles("1.0.1"))));
  const updater = new Updater({ packageRoot, instanceDir, feed, helper: HELPER });
  await updater.download();
  await updater.install({ testArgs: ["-TestNoProcess", "-TestFailStart"] });
  const result = path.join(updater.dir, "result.json");
  await until(() => fs.existsSync(result));
  const outcome = JSON.parse(fs.readFileSync(result, "utf8").replace(/^\uFEFF/, ""));
  assert.equal(outcome.status, "failed");
  assert.match(outcome.reason, /NEW_VERSION_DID_NOT_START/);
  assert.equal(packageVersion(packageRoot), "1.0.0", "the old version is back in place");
  assert.equal(fs.readFileSync(path.join(instanceDir, "data/storage.sqlite"), "utf8"), "the owner's data");
  assert(!fs.existsSync(packageRoot + ".previous-1.0.0"));
  // The verified download is kept, so trying again does not fetch it again.
  await until(() => updater.status().phase === "ready");
  assert(fs.existsSync(path.join(updater.stagedDir("1.0.1"), "fusion-manifest.json")));
});
