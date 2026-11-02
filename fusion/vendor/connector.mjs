// development/upstreams/dsh-workbuddy-connect/src/upstream.ts
import { randomUUID } from "node:crypto";

// development/upstreams/dsh-workbuddy-connect/src/app-version.ts
function validAppVersion(value) {
  return typeof value === "string" && /^\d{1,6}(?:\.\d{1,6}){1,3}$/u.test(value);
}

// development/upstreams/dsh-workbuddy-connect/src/client-identity.ts
function validCliVersion(value) {
  return typeof value === "string" && /^\d{1,6}(?:\.\d{1,6}){1,3}(?:-[0-9A-Za-z.]+)?$/u.test(value);
}
function chatUserAgent(identity, region) {
  if (!validAppVersion(identity.clientVersion)) {
    throw new Error(`invalid client version for chat User-Agent: ${JSON.stringify(identity.clientVersion)}`);
  }
  if (identity.cliVersion !== void 0 && !validCliVersion(identity.cliVersion)) {
    throw new Error(`invalid CLI version for chat User-Agent: ${JSON.stringify(identity.cliVersion)}`);
  }
  const product = region === "global" ? "WorkBuddy AI" : "WorkBuddy";
  const parts = [`WorkBuddy/${identity.clientVersion}`, `${product}/${identity.clientVersion}`];
  if (identity.cliVersion !== void 0) parts.push(`CLI/${identity.cliVersion}`);
  return parts.join(" ");
}

// development/upstreams/dsh-workbuddy-connect/src/upstream.ts
var CN_BILLING_BASE = "https://www.codebuddy.cn";
var GLOBAL_BASE = "https://www.workbuddy.ai";
var CLIENT_UA = "CLI/2.63.2 CodeBuddy/2.63.2";
function regionOf(domain) {
  const lowered = domain.trim().toLowerCase();
  if (lowered === "workbuddy.ai" || lowered.endsWith(".workbuddy.ai")) return "global";
  return "cn";
}
function originReferer(credential) {
  return regionOf(credential.domain) === "global" ? GLOBAL_BASE : CN_BILLING_BASE;
}
function commonHeaders(credential) {
  return {
    "Accept": "application/json, text/plain, */*",
    "X-Requested-With": "XMLHttpRequest",
    "Origin": originReferer(credential),
    "Referer": `${originReferer(credential)}/`,
    "User-Agent": CLIENT_UA
  };
}
function conversationHeaders() {
  return {
    "X-Conversation-ID": randomUUID(),
    "X-Conversation-Request-ID": randomUUID(),
    "X-Conversation-Message-ID": randomUUID()
  };
}
function chatHeaders(credential, userAgent, clientVersion) {
  const headers = {
    ...commonHeaders(credential),
    "User-Agent": userAgent,
    "Content-Type": "application/json",
    // 安全红线：chat 请求绝不携带 refresh token。
    ...credential.uid === "" ? { "X-No-User-Id": "1" } : { "X-User-Id": credential.uid },
    ...credential.enterpriseId === void 0 || credential.enterpriseId === "" ? { "X-No-Enterprise-Id": "1" } : { "X-Enterprise-Id": credential.enterpriseId },
    ...credential.domain === "" ? { "X-No-Department-Info": "1" } : { "X-Domain": credential.domain },
    "X-IDE-Type": "WorkBuddy",
    "X-IDE-Name": "WorkBuddy",
    "X-IDE-Version": clientVersion,
    "X-Product": "SaaS",
    ...conversationHeaders()
  };
  return headers;
}

// development/upstreams/dsh-workbuddy-connect/src/wsl.ts
import { release } from "node:os";
function isWsl() {
  if (process.platform !== "linux") return false;
  if (process.env["WSL_DISTRO_NAME"] !== void 0 || process.env["WSL_INTEROP"] !== void 0) return true;
  return release().toLowerCase().includes("microsoft");
}

// development/upstreams/dsh-workbuddy-connect/src/desktop-credential-protection.ts
import { execFile } from "node:child_process";
import { accessSync, constants, readFileSync, readdirSync, realpathSync, statSync } from "node:fs";
import { createCipheriv, createDecipheriv, createHash, randomBytes } from "node:crypto";
import { basename, dirname, join, win32 } from "node:path";

// development/upstreams/dsh-workbuddy-connect/src/status-paths.ts
var WORKBUDDY_STATUS_PATH = "/plugins/dsh-workbuddy-connect/status";
var WORKBUDDY_PROBE_PATH = "/plugins/dsh-workbuddy-connect/probe";
var WORKBUDDY_AI_STATUS_PATH = "/plugins/dsh-workbuddy-connect/ai/status";
var WORKBUDDY_AI_PROBE_PATH = "/plugins/dsh-workbuddy-connect/ai/probe";

// development/upstreams/dsh-workbuddy-connect/src/variants.ts
var WORKBUDDY_VARIANTS = [
  {
    id: "workbuddy",
    displayName: "WorkBuddy",
    appName: "WorkBuddy",
    region: "cn",
    env: "WORKBUDDY_AUTH_FILE",
    electron: {
      productName: "WorkBuddy",
      envVar: "WORKBUDDY_ELECTRON_BIN",
      macOS: {
        bundleId: "com.tencent.workbuddy.mac",
        defaultPath: "/Applications/WorkBuddy.app/Contents/MacOS/Electron"
      },
      windows: {
        displayNamePattern: /^WorkBuddy(?:\s+\d+(?:\.\d+)+(?:[-+][0-9A-Za-z.-]+)?)?$/u,
        exeBasename: "workbuddy.exe",
        defaultPathSegments: ["Programs", "WorkBuddy", "WorkBuddy.exe"]
      }
    },
    desktopFilename: "workbuddy-desktop.info",
    ownFilename: ".workbuddy-auth.json",
    probeFilename: ".workbuddy-probe.json",
    catalogFilename: ".workbuddy-catalog.json",
    visibilityFilename: ".workbuddy-model-visibility.json",
    statusPath: WORKBUDDY_STATUS_PATH,
    probePath: WORKBUDDY_PROBE_PATH
  },
  {
    id: "workbuddy-ai",
    displayName: "WorkBuddy AI",
    appName: "WorkBuddy AI",
    region: "global",
    env: "WORKBUDDY_AI_AUTH_FILE",
    electron: {
      productName: "WorkBuddy AI",
      envVar: "WORKBUDDY_AI_ELECTRON_BIN",
      macOS: {
        bundleId: "com.workbuddy.workbuddy-ai",
        defaultPath: "/Applications/WorkBuddy AI.app/Contents/MacOS/Electron"
      },
      windows: {
        // Measured in #60: `WorkBuddy AI 5.6.2`. The CN pattern cannot match
        // this value ("AI" is not a version) and this pattern cannot match
        // `WorkBuddy 5.6.2` (missing the literal " AI"), so the two records
        // never feed each other's discovery.
        displayNamePattern: /^WorkBuddy AI(?:\s+\d+(?:\.\d+)+(?:[-+][0-9A-Za-z.-]+)?)?$/u,
        exeBasename: "workbuddyai.exe"
      }
    },
    desktopFilename: "workbuddy-desktop-ai.info",
    ownFilename: ".workbuddy-ai-auth.json",
    probeFilename: ".workbuddy-ai-probe.json",
    catalogFilename: ".workbuddy-ai-catalog.json",
    visibilityFilename: ".workbuddy-ai-model-visibility.json",
    statusPath: WORKBUDDY_AI_STATUS_PATH,
    probePath: WORKBUDDY_AI_PROBE_PATH
  }
];
var CN_VARIANT = WORKBUDDY_VARIANTS[0];
var AI_VARIANT = WORKBUDDY_VARIANTS[1];

// development/upstreams/dsh-workbuddy-connect/src/desktop-credential-protection.ts
function defaultWorkBuddyElectronPath(product, platform = process.platform) {
  if (platform === "darwin") return product.macOS.defaultPath;
  if (platform !== "win32" || product.windows.defaultPathSegments === void 0) return void 0;
  const localAppData = process.env.LOCALAPPDATA?.trim();
  return localAppData === void 0 || localAppData === "" ? void 0 : join(localAppData, ...product.windows.defaultPathSegments);
}
function keyIdsOf(fields) {
  return [...new Set(fields.map((wrapped) => wrapped.envelope.keyId))];
}
function parseWrappedField(field, value) {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return void 0;
  const wrapped = value;
  if (wrapped["$wbEncrypted"] !== 1 || typeof wrapped["envelope"] !== "string") return void 0;
  let inner;
  try {
    inner = JSON.parse(Buffer.from(wrapped["envelope"], "base64").toString("utf8"));
  } catch {
    return void 0;
  }
  if (typeof inner !== "object" || inner === null || Array.isArray(inner)) return void 0;
  const parts = inner;
  const nonce = parseBase64(parts["nonce"], 12);
  const authTag = parseBase64(parts["authTag"], 16);
  const ciphertext = parseBase64(parts["ciphertext"]);
  if (nonce === void 0 || authTag === void 0 || ciphertext === void 0) return void 0;
  if (typeof parts["suite"] !== "number" || !Number.isInteger(parts["suite"])) return void 0;
  if (parts["suite"] !== 1) return void 0;
  if (typeof parts["keyId"] !== "string" || !/^[0-9a-f]{16}$/u.test(parts["keyId"])) return void 0;
  return {
    field,
    envelope: {
      suite: parts["suite"],
      keyId: parts["keyId"],
      nonce,
      authTag,
      ciphertext
    }
  };
}
function parseBase64(value, length) {
  if (typeof value !== "string" || value === "") return void 0;
  let decoded;
  try {
    decoded = Buffer.from(value, "base64");
  } catch {
    return void 0;
  }
  if (decoded.length === 0 || decoded.toString("base64").replace(/=+$/u, "") !== value.replace(/=+$/u, "")) return void 0;
  return length === void 0 || decoded.length === length ? decoded : void 0;
}
var AUTH_FIELDS = ["accessToken", "refreshToken"];
function classifyDesktopAuthDocument(text) {
  if (text.trim() === "") return { format: "absent" };
  let parsed;
  try {
    parsed = JSON.parse(text);
  } catch {
    return { format: "unrecognized" };
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) return { format: "unrecognized" };
  const document = parsed;
  const auth = typeof document["auth"] === "object" && document["auth"] !== null ? document["auth"] : document;
  const fields = [];
  for (const field of AUTH_FIELDS) {
    const value = auth[field];
    if (typeof value === "string") continue;
    const wrapped = parseWrappedField(field, value);
    if (wrapped === void 0 && value !== void 0) return { format: "unrecognized" };
    if (wrapped !== void 0) fields.push(wrapped);
  }
  if (fields.length === 0) return { format: "plaintext" };
  return { format: "encrypted", wrapped: { document, fields } };
}
function buildAuthenticatedContextAad(keyId, suite) {
  const prefix = Buffer.from("WB-AAD\0", "ascii");
  const lengthPrefixed = (value) => {
    const bytes = Buffer.from(value, "utf8");
    const header = Buffer.allocUnsafe(4);
    header.writeUInt32BE(bytes.length);
    return Buffer.concat([header, bytes]);
  };
  const suiteBytes = Buffer.allocUnsafe(4);
  suiteBytes.writeUInt32BE(suite);
  return Buffer.concat([
    prefix,
    Buffer.from([1]),
    lengthPrefixed("WBEV1"),
    lengthPrefixed("sym-v1"),
    suiteBytes,
    lengthPrefixed(keyId),
    Buffer.from([2]),
    Buffer.from([0]),
    Buffer.from([0])
  ]);
}
function openAuthField(key, envelope) {
  try {
    const decipher = createDecipheriv("aes-256-gcm", key, envelope.nonce, { authTagLength: 16 });
    decipher.setAAD(buildAuthenticatedContextAad(envelope.keyId, envelope.suite));
    decipher.setAuthTag(envelope.authTag);
    return Buffer.concat([decipher.update(envelope.ciphertext), decipher.final()]).toString("utf8");
  } catch {
    return void 0;
  }
}
function sealAuthFieldForTest(key, plaintext, suite = 1) {
  const keyId = createHash("sha256").update(key).digest("hex").slice(0, 16);
  const nonce = randomBytes(12);
  const cipher = createCipheriv("aes-256-gcm", key, nonce, { authTagLength: 16 });
  cipher.setAAD(buildAuthenticatedContextAad(keyId, suite));
  const ciphertext = Buffer.concat([cipher.update(Buffer.from(plaintext, "utf8")), cipher.final()]);
  const inner = {
    suite,
    keyId,
    nonce: nonce.toString("base64"),
    authTag: cipher.getAuthTag().toString("base64"),
    ciphertext: ciphertext.toString("base64")
  };
  return { "$wbEncrypted": 1, envelope: Buffer.from(JSON.stringify(inner), "utf8").toString("base64") };
}
function parseAtRestPayload(text) {
  let parsed;
  try {
    parsed = JSON.parse(text);
  } catch {
    return void 0;
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) return void 0;
  const payload = parsed;
  if (payload["version"] !== 1) return void 0;
  const secret = payload["atRestSecretKey"];
  if (typeof secret !== "string" || secret === "") return void 0;
  let decoded;
  try {
    decoded = Buffer.from(secret, "base64");
  } catch {
    return void 0;
  }
  if (decoded.length !== 32) return void 0;
  if (decoded.toString("base64") !== secret) return void 0;
  if (decoded.every((byte) => byte === 0)) return void 0;
  return { atRestSecretKey: secret };
}
function deriveProtectorKey(secret) {
  return createHash("sha256").update(secret, "utf8").digest();
}
var MDFIND_BIN = "/usr/bin/mdfind";
var PLUTIL_BIN = "/usr/bin/plutil";
var WINDOWS_REGISTRY_ROOTS = [
  "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
  "HKLM\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
  "HKLM\\Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall"
];
var WINDOWS_REGISTRY_OUTPUT_MAX_BYTES = 1024 * 1024;
var WINDOWS_ELECTRON_VERSION_PATTERN = /^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/u;
var WINDOWS_REGISTRY_RELATIVE_POWERSHELL = "System32\\WindowsPowerShell\\v1.0\\powershell.exe";
var WINDOWS_POWERSHELL_ROOT_ABSENT = 2;
var WINDOWS_POWERSHELL_ROOT_UNREADABLE = 3;
var WINDOWS_POWERSHELL_QUERY_SCRIPT = [
  "param([string]$Root)",
  "[Console]::OutputEncoding=[Text.Encoding]::UTF8;",
  '$ErrorActionPreference="Stop";',
  '$path="Registry::"+$Root;',
  'if (-not (Test-Path -LiteralPath $path)) { [Console]::Error.Write("root absent"); exit 2 };',
  'try { $k=Get-Item -LiteralPath $path -ErrorAction Stop } catch { [Console]::Error.Write("root unreadable"); exit 3 };',
  "$out=@();",
  "foreach ($n in $k.GetSubKeyNames()) {",
  '  try { $p=$k.OpenSubKey($n); if ($null -eq $p) { throw "null" } } catch { [Console]::Error.Write("subkey unreadable"); exit 3 };',
  '  $d=$p.GetValue("DisplayName");$i=$p.GetValue("DisplayIcon");',
  "  if ($null -ne $d -or $null -ne $i) { $out += [pscustomobject]@{ displayName=[string]$d; displayIcon=[string]$i } }",
  "};",
  "ConvertTo-Json -InputObject @($out) -Compress -Depth 3"
].join(" ");
var WINDOWS_POWERSHELL_QUERY_COMMAND = `& { ${WINDOWS_POWERSHELL_QUERY_SCRIPT} }`;
var WINDOWS_REGISTRY_ROOT_PATTERN = /^HK(?:LM|CU)(?:\\[A-Za-z0-9._-]+)+$/u;
var WORKBUDDY_DISCOVERY_STEP_TIMEOUT_MS = 3e3;
var WORKBUDDY_DISCOVERY_BUDGET_MS = 1e4;
var WINDOWS_REGISTRY_STEP_TIMEOUT_MS = 1e4;
var MDFIND_MAX_OUTPUT_BYTES = 1024 * 1024;
var PLUTIL_MAX_OUTPUT_BYTES = 64 * 1024;
var DiscoveryIncompleteError = class extends Error {
};
function wslenvForwardRunAsNode(value) {
  const merged = [];
  let targetIndex = -1;
  let targetFlags = "/w";
  for (const entry of (value ?? "").split(":")) {
    if (entry === "") continue;
    const slash = entry.indexOf("/");
    const name = slash === -1 ? entry : entry.slice(0, slash);
    if (name !== "ELECTRON_RUN_AS_NODE") {
      if (!merged.includes(entry)) merged.push(entry);
      continue;
    }
    const flags = slash === -1 ? "" : entry.slice(slash + 1);
    if (flags.includes("u")) targetFlags = "/uw";
    if (targetIndex === -1) {
      targetIndex = merged.length;
      merged.push(`ELECTRON_RUN_AS_NODE${targetFlags}`);
    } else {
      merged[targetIndex] = `ELECTRON_RUN_AS_NODE${targetFlags}`;
    }
  }
  if (targetIndex === -1) merged.push(`ELECTRON_RUN_AS_NODE${targetFlags}`);
  return merged.join(":");
}
function helperChildEnv(parent, electronPath, wsl) {
  const childEnv = { ...parent, ELECTRON_RUN_AS_NODE: "1" };
  if (wsl && basename(electronPath).toLowerCase().endsWith(".exe")) {
    childEnv["WSLENV"] = wslenvForwardRunAsNode(childEnv["WSLENV"]);
  }
  return childEnv;
}
function workBuddyDiscoveryTools(bundleId) {
  const runTool = (bin, args, maxBytes, signal) => new Promise((resolve, reject) => {
    if (signal.aborted) {
      reject(new DiscoveryIncompleteError(`${bin} was not started: the discovery budget was already spent`));
      return;
    }
    let settled = false;
    const child = execFile(bin, [...args], { maxBuffer: maxBytes, timeout: WORKBUDDY_DISCOVERY_STEP_TIMEOUT_MS }, (error, stdout) => {
      if (settled) return;
      settled = true;
      if (error !== null && error !== void 0) {
        reject(new DiscoveryIncompleteError(`${bin} could not complete (${error.killed === true ? "timed out" : String(error.code ?? "unavailable")})`));
        return;
      }
      resolve(stdout);
    });
    const abort = () => {
      if (settled) return;
      settled = true;
      child.kill();
      reject(new DiscoveryIncompleteError(`${bin} was abandoned: the discovery budget was spent`));
    };
    signal.addEventListener("abort", abort, { once: true });
    child.on("close", () => {
      signal.removeEventListener("abort", abort);
    });
  });
  return {
    findApps: async (signal) => {
      const out = await runTool(
        MDFIND_BIN,
        [`kMDItemCFBundleIdentifier == '${bundleId}'`],
        MDFIND_MAX_OUTPUT_BYTES,
        signal
      );
      return out.split("\n").map((line) => line.trim()).filter((line) => line.endsWith(".app"));
    },
    bundleIdentifier: async (bundlePath, signal) => {
      try {
        const out = await runTool(
          PLUTIL_BIN,
          ["-extract", "CFBundleIdentifier", "raw", "-o", "-", join(bundlePath, "Contents", "Info.plist")],
          PLUTIL_MAX_OUTPUT_BYTES,
          signal
        );
        return out.trim();
      } catch {
        return void 0;
      }
    },
    bundleVersion: async (bundlePath, signal) => {
      try {
        const out = await runTool(
          PLUTIL_BIN,
          ["-extract", "CFBundleShortVersionString", "raw", "-o", "-", join(bundlePath, "Contents", "Info.plist")],
          PLUTIL_MAX_OUTPUT_BYTES,
          signal
        );
        const version = out.trim();
        return version === "" ? void 0 : version;
      } catch {
        return void 0;
      }
    }
  };
}
function workBuddyWindowsDiscoveryTools() {
  const systemRoot = process.env.SystemRoot?.trim();
  const regPath = systemRoot === void 0 || systemRoot === "" ? void 0 : win32.join(systemRoot, "System32", "reg.exe");
  const powershellPath = systemRoot === void 0 || systemRoot === "" ? void 0 : win32.join(systemRoot, WINDOWS_REGISTRY_RELATIVE_POWERSHELL);
  const runTool = (root, signal) => new Promise((resolve, reject) => {
    if (regPath === void 0) {
      reject(new DiscoveryIncompleteError("SystemRoot is not configured"));
      return;
    }
    if (signal.aborted) {
      reject(new DiscoveryIncompleteError("reg.exe was not started: the discovery budget was already spent"));
      return;
    }
    let settled = false;
    const child = execFile(regPath, ["query", root, "/s"], {
      maxBuffer: WINDOWS_REGISTRY_OUTPUT_MAX_BYTES,
      timeout: WINDOWS_REGISTRY_STEP_TIMEOUT_MS,
      windowsHide: true,
      // Raw bytes, not a decode: whether this output is ambiguous is decided
      // from the bytes themselves (#88), so Node must not pick an encoding.
      encoding: "buffer"
    }, (error, stdout, stderr) => {
      if (settled) return;
      settled = true;
      if (error !== null && error !== void 0) {
        if (error.killed !== true && (error.code === 1 || error.code === "1") && windowsRegistryKeyMissingFromBytes(stderr)) {
          resolve(Buffer.alloc(0));
          return;
        }
        reject(new DiscoveryIncompleteError(`reg.exe could not complete (${subprocessFailureReason(error)})`));
        return;
      }
      resolve(stdout);
    });
    const abort = () => {
      if (settled) return;
      settled = true;
      child.kill();
      reject(new DiscoveryIncompleteError("reg.exe was abandoned: the discovery budget was spent"));
    };
    signal.addEventListener("abort", abort, { once: true });
    child.on("close", () => {
      signal.removeEventListener("abort", abort);
    });
  });
  const runFallback = (root, signal) => new Promise((resolve, reject) => {
    if (!WINDOWS_REGISTRY_ROOT_PATTERN.test(root)) {
      reject(new DiscoveryIncompleteError("the registry root was not a plain HKLM/HKCU path this tool may query"));
      return;
    }
    if (powershellPath === void 0) {
      reject(new DiscoveryIncompleteError("SystemRoot is not configured"));
      return;
    }
    if (signal.aborted) {
      reject(new DiscoveryIncompleteError("powershell.exe was not started: the discovery budget was already spent"));
      return;
    }
    let settled = false;
    const child = execFile(powershellPath, [
      "-NoProfile",
      "-NonInteractive",
      "-Command",
      WINDOWS_POWERSHELL_QUERY_COMMAND,
      "-Root",
      root
    ], {
      maxBuffer: WINDOWS_REGISTRY_OUTPUT_MAX_BYTES,
      timeout: WINDOWS_REGISTRY_STEP_TIMEOUT_MS,
      windowsHide: true,
      encoding: "utf8"
    }, (error, stdout) => {
      if (settled) return;
      settled = true;
      if (error !== null && error !== void 0) {
        const code = error.killed === true ? void 0 : Number(error.code);
        if (code === WINDOWS_POWERSHELL_ROOT_ABSENT) {
          resolve("");
          return;
        }
        if (code === WINDOWS_POWERSHELL_ROOT_UNREADABLE) {
          reject(new DiscoveryIncompleteError(`powershell.exe could not read the registry root ${root}`));
          return;
        }
        reject(new DiscoveryIncompleteError(`powershell.exe could not complete (${subprocessFailureReason(error)})`));
        return;
      }
      const records = parseWindowsUninstallJson(stdout);
      if (records === void 0) {
        reject(new DiscoveryIncompleteError("powershell.exe output was not the expected registry document"));
        return;
      }
      resolve(records);
    });
    const abort = () => {
      if (settled) return;
      settled = true;
      child.kill();
      reject(new DiscoveryIncompleteError("powershell.exe was abandoned: the discovery budget was spent"));
    };
    signal.addEventListener("abort", abort, { once: true });
    child.on("close", () => {
      signal.removeEventListener("abort", abort);
    });
  });
  return {
    queryUninstallRoot: async (root, signal) => {
      const raw = await runTool(root, signal);
      if (isAsciiOnly(raw)) return raw.toString("utf8");
      return await runFallback(root, signal);
    }
  };
}
function isAsciiOnly(bytes) {
  for (const byte of bytes) {
    if (byte >= 128) return false;
  }
  return true;
}
function parseWindowsUninstallJson(output) {
  let document;
  try {
    document = JSON.parse(output);
  } catch {
    return void 0;
  }
  const rows = Array.isArray(document) ? document : [document];
  const lines = [];
  for (const [index, row] of rows.entries()) {
    if (typeof row !== "object" || row === null) continue;
    const record = row;
    const displayName = record["displayName"];
    const displayIcon = record["displayIcon"];
    if (typeof displayName !== "string" && typeof displayIcon !== "string") continue;
    lines.push(`HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{PS-${String(index)}}`);
    if (typeof displayName === "string") lines.push(`    DisplayName    REG_SZ    ${displayName}`);
    if (typeof displayIcon === "string") lines.push(`    DisplayIcon    REG_SZ    ${displayIcon}`);
  }
  return lines.join("\r\n");
}
function expandWindowsEnv(raw, env = process.env) {
  return raw.replace(/%([^%]+)%/gu, (match, name) => {
    const key = Object.keys(env).find((k) => k.toLowerCase() === name.toLowerCase());
    return key !== void 0 && env[key] !== void 0 ? env[key] : match;
  });
}
async function readWindowsRegistryUserEnv(varName, options = {}) {
  if (process.platform !== "win32" && options.exec === void 0) return void 0;
  const systemRoot = options.systemRoot ?? process.env.SystemRoot?.trim();
  const regPath = systemRoot === void 0 || systemRoot === "" ? void 0 : win32.join(systemRoot, "System32", "reg.exe");
  if (regPath === void 0) return void 0;
  const timeoutMs = options.timeoutMs ?? 1500;
  const exec = options.exec ?? ((file, args, opts) => new Promise((resolve, reject) => {
    execFile(file, args, { windowsHide: opts.windowsHide, encoding: "buffer", timeout: opts.timeout }, (error, stdout) => {
      if (error) reject(error);
      else resolve(stdout);
    });
  }));
  const roots = [
    "HKCU\\Environment",
    "HKLM\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment"
  ];
  for (const root of roots) {
    try {
      const raw = await exec(regPath, ["query", root, "/v", varName], {
        windowsHide: true,
        encoding: "buffer",
        timeout: timeoutMs
      });
      let text = raw.toString("utf8");
      if (/[\uFFFD]/u.test(text)) {
        try {
          text = new TextDecoder("gbk").decode(raw);
        } catch {
        }
      }
      const match = new RegExp(`^\\s*${varName}\\s+(REG_[A-Z0-9_]+)\\s+(.*)$`, "imu").exec(text);
      if (match !== null && match[1] !== void 0 && match[2] !== void 0) {
        const type = match[1].toUpperCase();
        let val = match[2].trim();
        if (type === "REG_EXPAND_SZ") {
          val = expandWindowsEnv(val);
        }
        if (val !== "") return val;
      }
    } catch {
    }
  }
  return void 0;
}
var WorkBuddyAtRestKeyProvider = class {
  /**
   * The explicit binary, when one was configured. `undefined` here means "the
   * caller did not name one", which is what lets discovery run — an explicit
   * path that turns out to be unusable is an error, never a reason to look for
   * a different app.
   */
  explicitPath;
  product;
  defaultPath;
  discovery;
  tools;
  windowsTools;
  readWindowsUserEnv;
  platform;
  discoveryBudgetMs;
  timeoutMs;
  wsl;
  source;
  spawnHelper;
  /**
   * The path discovery settled on, cached only on success. A failure leaves
   * this unset so the next attempt tries again — the user may install or move
   * the app without restarting DSH.
   */
  discoveredPath;
  /**
   * The explicit binary this product's variable names in the Windows user
   * environment, cached only when the environment actually named one. A miss
   * stays uncached — see {@link windowsUserEnvPath}.
   */
  userEnvCache;
  cache;
  inflight;
  constructor(options) {
    this.product = options.product;
    this.platform = options.platform ?? process.platform;
    this.discovery = options.discovery ?? "none";
    this.readWindowsUserEnv = options.readWindowsUserEnv ?? (options.windowsTools !== void 0 || this.discovery !== "windows-workbuddy" ? () => void 0 : (varName) => this.platform === "win32" && process.platform === "win32" ? readWindowsRegistryUserEnv(varName) : void 0);
    const fromEnv = process.env[options.product.envVar]?.trim();
    const envPath = fromEnv === void 0 || fromEnv === "" ? void 0 : fromEnv;
    this.explicitPath = options.electronPath ?? envPath;
    this.defaultPath = this.discovery === "none" ? void 0 : options.defaultElectronPath === void 0 ? defaultWorkBuddyElectronPath(options.product, this.platform) : options.defaultElectronPath ?? void 0;
    this.tools = options.tools ?? workBuddyDiscoveryTools(options.product.macOS.bundleId);
    this.windowsTools = options.windowsTools ?? workBuddyWindowsDiscoveryTools();
    this.discoveryBudgetMs = options.discoveryBudgetMs ?? WORKBUDDY_DISCOVERY_BUDGET_MS;
    this.timeoutMs = options.timeoutMs ?? 1e4;
    this.wsl = options.wsl ?? isWsl;
    this.spawnHelper = options.spawnHelper ?? ((path) => this.spawnAt(path));
    this.source = options.source ?? (() => this.spawnPayload());
  }
  /**
   * The binary the default helper would use, for diagnostics.
   *
   * Reports a *discovery result* once one exists, so diagnostics describe what
   * would actually run rather than the default that was bypassed. Discovery
   * itself stays in {@link resolveElectronPath}: this accessor never triggers a
   * search (the constructor must remain I/O-free, and callers may ask before
   * any resolution has happened).
   */
  helperPath() {
    if (this.explicitPath !== void 0) return this.explicitPath;
    if (this.discovery === "none") return void 0;
    return this.discoveredPath ?? this.defaultPath;
  }
  /**
   * A protector key matching one of the requested envelope key ids. The first
   * id the cache answers wins; otherwise one spawn resolves the current key,
   * which must match a request — a mismatch means the envelopes were sealed by
   * a different install than the one this machine now runs, and no key we can
   * reach will open them.
   */
  async protectorKeyFor(requested) {
    if (requested.length === 0) {
      throw new WorkBuddyElectronPathError(
        "encrypted-credential-unreadable",
        "encrypted desktop credential carries no key ids"
      );
    }
    const cached = this.cache;
    if (cached !== void 0 && requested.includes(cached.keyId)) return cached.key;
    this.inflight ??= this.source().then((text) => this.ingest(text)).finally(() => {
      this.inflight = void 0;
    });
    const resolved = await this.inflight;
    if (!requested.includes(resolved.keyId)) {
      throw new WorkBuddyElectronPathError(
        "encrypted-credential-unreadable",
        `WorkBuddy's current at-rest key (id ${resolved.keyId}) does not match the credential's envelope (id ${requested.join(" or ")}); the desktop credential was sealed by a different WorkBuddy installation`
      );
    }
    return resolved.key;
  }
  ingest(text) {
    const payload = parseAtRestPayload(text);
    if (payload === void 0) {
      throw new WorkBuddyElectronPathError(
        "encrypted-credential-unreadable",
        "WorkBuddy key helper returned an unusable at-rest payload (expected {version:1, atRestSecretKey})"
      );
    }
    const key = deriveProtectorKey(payload.atRestSecretKey);
    const resolved = {
      key,
      keyId: createHash("sha256").update(key).digest("hex").slice(0, 16)
    };
    this.cache = resolved;
    return resolved;
  }
  /**
   * The explicit binary named in the Windows *user* environment, when
   * `process.env` does not carry it.
   *
   * A hit is cached: it is a user-level preference that only changes when
   * someone edits it, and caching it keeps a warm path from re-spawning
   * `reg.exe` on every poll. A miss is deliberately **not** cached. The whole
   * point of this branch is the window in which the variable is missing from
   * the running host's snapshot — the user reads the card's diagnostic, adds
   * the variable, and retries without restarting DSH. Remembering "it was not
   * there" would pin the failing answer for the life of the process, which is
   * exactly the case this fallback exists to fix. The read is bounded
   * (1500ms, async) and only runs while the variable is absent, so a retry
   * costs one short subprocess per attempt and stops as soon as it succeeds.
   */
  async windowsUserEnvPath() {
    const cached = this.userEnvCache;
    if (cached !== void 0) {
      if (isExecutable(cached)) return cached;
      this.userEnvCache = void 0;
    }
    let raw;
    try {
      raw = await this.readWindowsUserEnv(this.product.envVar);
    } catch {
      return void 0;
    }
    const trimmed = raw?.trim();
    if (trimmed === void 0 || trimmed === "") return void 0;
    this.userEnvCache = trimmed;
    return trimmed;
  }
  /**
   * The binary to spawn, or a diagnosable error saying why there is none.
   *
   * Order is the contract: an explicit path is used as-is and never falls back;
   * discovery runs only for a provider that was configured for it, and only
   * after explicit process and Windows registry configuration, then the platform default, have been tried.
   */
  async resolveElectronPath() {
    if (this.explicitPath !== void 0) {
      if (!isExecutable(this.explicitPath)) {
        throw new WorkBuddyElectronPathError(
          "electron-path-invalid",
          `the configured ${this.product.productName} Electron binary is not available at ${this.explicitPath}; check ${this.product.envVar} or unset it to let the plugin look for the app itself`
        );
      }
      return this.explicitPath;
    }
    if (this.discovery === "none") {
      throw new WorkBuddyElectronPathError(
        "electron-binary-unavailable",
        `no ${this.product.productName} Electron binary is configured for this platform; set ${this.product.envVar} to the app's Electron binary`
      );
    }
    if (this.platform === "win32" && (process.env[this.product.envVar]?.trim() ?? "") === "") {
      const configured = await this.windowsUserEnvPath();
      if (configured !== void 0) {
        if (!isExecutable(configured)) {
          throw new WorkBuddyElectronPathError(
            "electron-path-invalid",
            `the configured ${this.product.productName} Electron binary is not available at ${configured}; check ${this.product.envVar} or unset it to let the plugin look for the app itself`
          );
        }
        this.discoveredPath = configured;
        return configured;
      }
    }
    if (this.defaultPath !== void 0 && isExecutable(this.defaultPath)) return this.defaultPath;
    if (this.discoveredPath !== void 0) {
      if (isExecutable(this.discoveredPath)) return this.discoveredPath;
      this.discoveredPath = void 0;
    }
    const found = this.discovery === "macos-workbuddy" ? await this.discoverMacosApp() : await this.discoverWindowsApp();
    this.discoveredPath = found;
    return found;
  }
  /**
   * Resolve this product's app through Spotlight, then prove each candidate's
   * identity before it can be executed.
   *
   * The whole flow shares one budget: a hang in one candidate must not extend
   * the wait for the others, and running out of budget is reported as an
   * unfinished check rather than an absent app.
   */
  async discoverMacosApp() {
    if (this.platform !== "darwin") {
      throw new WorkBuddyElectronPathError(
        "electron-binary-unavailable",
        `no ${this.product.productName} Electron binary is configured for this platform; set ${this.product.envVar} to the app's Electron binary`
      );
    }
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.discoveryBudgetMs);
    try {
      let candidates;
      try {
        candidates = await this.tools.findApps(controller.signal);
      } catch {
        throw discoveryIncomplete(this.product.productName, "the app search did not complete");
      }
      const seen = /* @__PURE__ */ new Map();
      let unresolved = false;
      for (const candidate of candidates) {
        try {
          statSync(candidate);
        } catch (error) {
          if (isENOENT(error)) continue;
          unresolved = true;
          continue;
        }
        let bundleIdentifier;
        try {
          bundleIdentifier = await this.tools.bundleIdentifier(candidate, controller.signal);
        } catch {
          unresolved = true;
          continue;
        }
        if (bundleIdentifier === void 0) {
          unresolved = true;
          continue;
        }
        if (bundleIdentifier !== this.product.macOS.bundleId) continue;
        const electronPath = join(candidate, "Contents", "MacOS", "Electron");
        if (!isExecutable(electronPath)) continue;
        let identity;
        try {
          identity = realpathSync(candidate);
        } catch {
          identity = candidate;
        }
        if (seen.has(identity)) continue;
        let version;
        try {
          version = await this.tools.bundleVersion(candidate, controller.signal);
        } catch {
          version = void 0;
        }
        seen.set(identity, { bundlePath: candidate, electronPath, ...version === void 0 ? {} : { version } });
      }
      if (seen.size > 1) {
        const listed = [...seen.values()].map((app) => `  - ${app.bundlePath}${app.version === void 0 ? "" : ` (${app.version})`}`).join("\n");
        throw new WorkBuddyElectronPathError(
          "electron-binary-ambiguous",
          `more than one ${this.product.productName} application was found, so none was chosen:
${listed}
 set ${this.product.envVar} to the one to use`
        );
      }
      if (unresolved) throw discoveryIncomplete(this.product.productName, "some candidates could not be checked");
      if (seen.size === 0) {
        throw new WorkBuddyElectronPathError(
          "electron-binary-not-found",
          `no ${this.product.productName} application was found in the default location or the system index; if it is installed elsewhere, it may not be indexed yet; set ${this.product.envVar} to the app's Electron binary`
        );
      }
      return [...seen.values()][0].electronPath;
    } finally {
      clearTimeout(timer);
      controller.abort();
    }
  }
  /**
   * Resolve this product's app through Windows uninstall records. Registry
   * entries provide hints, not trust: every DisplayIcon candidate must still
   * be the product's Electron binary with the known Electron layout before
   * execution.
   */
  async discoverWindowsApp() {
    if (this.platform !== "win32") {
      throw new WorkBuddyElectronPathError(
        "electron-binary-unavailable",
        `no ${this.product.productName} Electron binary is configured for this platform; set ${this.product.envVar} to the app's Electron binary`
      );
    }
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.discoveryBudgetMs);
    try {
      const candidates = [];
      let unresolved = false;
      for (const root of WINDOWS_REGISTRY_ROOTS) {
        let output;
        try {
          output = await this.windowsTools.queryUninstallRoot(root, controller.signal);
        } catch {
          unresolved = true;
          continue;
        }
        const parsed = parseWindowsRegistryOutput(output, this.product.windows.displayNamePattern);
        candidates.push(...parsed.candidates);
        unresolved ||= parsed.incomplete;
      }
      const seen = /* @__PURE__ */ new Map();
      const rejected = [];
      for (const candidate of new Set(candidates)) {
        const inspection = inspectWindowsElectronCandidate(candidate, this.platform, this.product.windows.exeBasename);
        if (inspection === "unresolved") {
          unresolved = true;
          continue;
        }
        if (inspection === void 0) {
          rejected.push(candidate);
          continue;
        }
        seen.set(inspection.identity, inspection.electronPath);
      }
      if (unresolved) throw discoveryIncomplete(this.product.productName, "some registry entries or candidates could not be checked");
      if (seen.size > 1) {
        const listed = [...seen.values()].map((path) => `  - ${path}`).join("\n");
        throw new WorkBuddyElectronPathError(
          "electron-binary-ambiguous",
          `more than one ${this.product.productName} application was found, so none was chosen:
${listed}
 set ${this.product.envVar} to the one to use`
        );
      }
      if (seen.size === 0) {
        const message = rejected.length === 0 ? `no usable ${this.product.productName} Electron binary was found in the default location or Windows uninstall records; set ${this.product.envVar} to the app's Electron binary` : `Windows uninstall records found ${rejected.length} ${this.product.productName} candidate${rejected.length > 1 ? "s" : ""}, but ${rejected.length > 1 ? "none" : "it"} did not match the expected app layout (the app's exe beside a version file and resources\\app.asar); set ${this.product.envVar} to the installed app's executable to use it`;
        throw new WorkBuddyElectronPathError("electron-binary-not-found", message);
      }
      return [...seen.values()][0];
    } finally {
      clearTimeout(timer);
      controller.abort();
    }
  }
  async spawnPayload() {
    return await this.spawnHelper(await this.resolveElectronPath());
  }
  async spawnAt(electronPath) {
    return await new Promise((resolve, reject) => {
      execFile(electronPath, [HELPER_SCRIPT_ARGUMENT_FLAG, HELPER_SCRIPT], {
        timeout: this.timeoutMs,
        maxBuffer: 1024 * 1024,
        windowsHide: true,
        // #133: under WSL→Windows interop the variable must ALSO be listed in
        // the child's WSLENV with the /w flag, or the Windows exe never sees
        // it and starts in GUI mode. helperChildEnv keeps every other
        // platform's environment byte-for-byte as before.
        env: helperChildEnv(process.env, electronPath, this.wsl())
      }, (error, stdout) => {
        if (error !== null && error !== void 0) {
          const reason = error.killed === true ? `timed out or was killed after ${String(this.timeoutMs)}ms` : error.code !== void 0 ? `exited with code ${String(error.code)}` : "could not be started";
          reject(new WorkBuddyElectronPathError(
            "encrypted-credential-unreadable",
            `the WorkBuddy key helper (${electronPath}) ${reason}`
          ));
          return;
        }
        const output = stdout.trim();
        if (output === "") {
          reject(new WorkBuddyElectronPathError(
            "encrypted-credential-unreadable",
            `the WorkBuddy key helper (${electronPath}) produced no payload`
          ));
          return;
        }
        resolve(output);
      });
    });
  }
};
function subprocessFailureReason(error) {
  if (error.code === "ERR_CHILD_PROCESS_STDIO_MAXBUFFER") return "the output exceeded the size limit";
  return error.killed === true ? "timed out" : String(error.code ?? "unavailable");
}
function windowsRegistryKeyMissing(stderr) {
  const detail = stderr.trim();
  return /^ERROR:\s*The system was unable to find the specified registry key or value\.?$/iu.test(detail) || /^错误[:：]\s*系统找不到指定的注册表项或值[。.]?$/u.test(detail);
}
function windowsRegistryKeyMissingFromBytes(stderr) {
  if (windowsRegistryKeyMissing(stderr.toString("utf8"))) return true;
  try {
    return windowsRegistryKeyMissing(new TextDecoder("gbk").decode(stderr));
  } catch {
    return false;
  }
}
function parseWindowsRegistryOutput(output, displayNamePattern) {
  const entries = /* @__PURE__ */ new Map();
  let currentKey;
  for (const line of output.split(/\r?\n/u)) {
    const keyMatch = /^\s*(HKEY_[^\r\n]+?)\s*$/iu.exec(line);
    if (keyMatch !== null) {
      currentKey = keyMatch[1];
      entries.set(currentKey, {});
      continue;
    }
    if (currentKey === void 0) continue;
    const valueMatch = /^\s+(DisplayName|DisplayIcon)\s+REG_[A-Z0-9_]+\s*(.*?)\s*$/iu.exec(line);
    if (valueMatch === null) continue;
    const entry = entries.get(currentKey);
    if (entry === void 0) continue;
    const value = valueMatch[2] ?? "";
    if (valueMatch[1].toLowerCase() === "displayname") entry.displayName = value;
    else entry.displayIcon = value;
  }
  const candidates = [];
  let incomplete = false;
  for (const entry of entries.values()) {
    if (entry.displayName === void 0) continue;
    if (!displayNamePattern.test(entry.displayName.trim())) continue;
    const displayIcon = entry.displayIcon === void 0 ? void 0 : parseWindowsDisplayIcon(entry.displayIcon);
    if (displayIcon === void 0) incomplete = true;
    else candidates.push(displayIcon);
  }
  return { candidates, incomplete };
}
function parseWindowsDisplayIcon(value) {
  const raw = value.trim();
  let path;
  if (raw.startsWith('"')) {
    const closingQuote = raw.indexOf('"', 1);
    if (closingQuote < 0) return void 0;
    const suffix = raw.slice(closingQuote + 1).trim();
    if (suffix !== "" && !/^,\d+$/u.test(suffix)) return void 0;
    path = raw.slice(1, closingQuote).replace(/,\d+$/u, "");
  } else {
    const match = /^(.+?\.exe)(?:,\d+)?$/iu.exec(raw);
    if (match === null) return void 0;
    path = match[1];
  }
  path = path.trim();
  return /\.exe$/iu.test(path) ? path : void 0;
}
function inspectWindowsElectronCandidate(electronPath, platform, exeBasename) {
  if (platform !== "win32" || basename(electronPath).toLowerCase() !== exeBasename) return void 0;
  let binaryStat;
  try {
    binaryStat = statSync(electronPath);
  } catch (error) {
    return isENOENT(error) ? void 0 : "unresolved";
  }
  if (!binaryStat.isFile()) return void 0;
  try {
    accessSync(electronPath, constants.X_OK);
  } catch (error) {
    return isENOENT(error) ? void 0 : "unresolved";
  }
  const installRoot = dirname(electronPath);
  let version;
  try {
    version = readFileSync(join(installRoot, "version"), "utf8").trim();
  } catch (error) {
    return isENOENT(error) ? void 0 : "unresolved";
  }
  if (!WINDOWS_ELECTRON_VERSION_PATTERN.test(version)) return void 0;
  try {
    if (!readdirSync(join(installRoot, "resources")).includes("app.asar")) return void 0;
  } catch (error) {
    return isENOENT(error) ? void 0 : "unresolved";
  }
  let identity;
  try {
    identity = realpathSync(electronPath);
  } catch (error) {
    return isENOENT(error) ? void 0 : "unresolved";
  }
  return {
    electronPath,
    // Windows paths are case-insensitive even when a registry entry preserved
    // a different casing from the filesystem spelling.
    identity: platform === "win32" ? identity.toLowerCase() : identity
  };
}
function isExecutable(path) {
  try {
    accessSync(path, constants.X_OK);
    return true;
  } catch {
    return false;
  }
}
var WorkBuddyElectronPathError = class extends Error {
  reasonCode;
  constructor(reasonCode, message) {
    super(message);
    this.name = "WorkBuddyElectronPathError";
    this.reasonCode = reasonCode;
  }
};
function isENOENT(error) {
  return error?.code === "ENOENT";
}
function discoveryIncomplete(productName, detail) {
  return new WorkBuddyElectronPathError(
    "electron-discovery-incomplete",
    `the ${productName} application search did not finish (${detail}); this is not proof that the app is missing`
  );
}
var HELPER_SCRIPT = 'process.stdout.write(String(process._linkedBinding("electron_browser_workbuddy_storage").loggerGet()))';
var HELPER_SCRIPT_ARGUMENT_FLAG = "-e";

// development/upstreams/dsh-workbuddy-connect/src/auth.ts
function expiryToMs(value) {
  if (value <= 0) return 0;
  return value > 1e12 ? value : value * 1e3;
}
function optionalString(value) {
  return typeof value === "string" && value !== "" ? value : void 0;
}
function parseWorkBuddyAuth(text) {
  let parsed;
  try {
    parsed = JSON.parse(text);
  } catch {
    return void 0;
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) return void 0;
  const document = parsed;
  let auth;
  let identity;
  if (typeof document["auth"] === "object" && document["auth"] !== null) {
    auth = document["auth"];
    identity = typeof document["account"] === "object" && document["account"] !== null ? document["account"] : {};
  } else {
    auth = document;
    identity = document;
  }
  const accessToken = typeof auth["accessToken"] === "string" ? auth["accessToken"] : "";
  if (accessToken === "") return void 0;
  const expiresAtMs = typeof auth["expiresAt"] === "number" ? expiryToMs(auth["expiresAt"]) : 0;
  const refreshExpiresAtMs = typeof auth["refreshExpiresAt"] === "number" ? expiryToMs(auth["refreshExpiresAt"]) : void 0;
  const enterpriseId = optionalString(identity["enterpriseId"]);
  const nickname = optionalString(identity["nickname"]);
  const credential = {
    accessToken,
    refreshToken: typeof auth["refreshToken"] === "string" ? auth["refreshToken"] : "",
    expiresAtMs,
    ...refreshExpiresAtMs === void 0 ? {} : { refreshExpiresAtMs },
    domain: optionalString(auth["domain"]) ?? "",
    uid: optionalString(identity["uid"]) ?? "",
    ...enterpriseId === void 0 ? {} : { enterpriseId },
    ...nickname === void 0 ? {} : { nickname },
    source: "desktop"
  };
  return credential;
}
export {
  AI_VARIANT,
  WorkBuddyAtRestKeyProvider,
  chatHeaders,
  chatUserAgent,
  classifyDesktopAuthDocument,
  keyIdsOf,
  openAuthField,
  parseWorkBuddyAuth,
  sealAuthFieldForTest
};
