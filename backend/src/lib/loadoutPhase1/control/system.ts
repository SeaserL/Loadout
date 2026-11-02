// SPDX-License-Identifier: Apache-2.0
// Gateway status, curated settings and what this machine already has installed.
import fs from "node:fs";
import path from "node:path";
import { createHash } from "node:crypto";
import { createServer } from "node:net";
import { spawn, spawnSync } from "node:child_process";
import { getDbInstance } from "../../db/core";
import { resolveCallLogRetentionDays } from "../../usage/callLogRetention";
import { instanceDir, invalid, kvGet, kvSet, snapshot, type Native } from "./shared";
import { subpoolRemember, subpoolState } from "./routing";

function instanceConfig() {
  try {
    return JSON.parse(fs.readFileSync(path.join(instanceDir(), "instance.json"), "utf8"));
  } catch {
    return {};
  }
}
function writeInstanceConfig(patch: Record<string, unknown>) {
  const file = path.join(instanceDir(), "instance.json");
  const meta = JSON.parse(fs.readFileSync(file, "utf8"));
  meta.config = { ...meta.config, ...patch };
  const tmp = `${file}.${process.pid}.tmp`;
  fs.writeFileSync(tmp, JSON.stringify(meta, null, 2));
  fs.renameSync(tmp, file);
}
/** The supervisor picks this up; the backend must not stop itself mid-response. */
function requestControl(op: "stop" | "restart", connect?: string) {
  fs.writeFileSync(path.join(instanceDir(), "control-request.json"), JSON.stringify({ op, connect }));
}

export function daemonStatus() {
  const migrations = getDbInstance()
    .prepare("SELECT COUNT(*) AS n FROM _omniroute_migrations")
    .get() as { n: number };
  const runtime = /^fusion-v([\d.]+)-/.exec(String(instanceConfig().baseline ?? ""))?.[1];
  return {
    version: [process.env.LOADOUT_VERSION, runtime && `OmniRoute ${runtime}`].filter(Boolean).join(" · "),
    pid: process.pid,
    started_at_ms: Date.now() - Math.round(process.uptime() * 1000),
    data_dir: instanceDir(),
    schema_version: migrations.n,
    gateway: { addr: `127.0.0.1:${process.env.PORT}`, listening: true },
    recovered_from: null,
    network: instanceConfig().config?.network ?? "unknown",
    log_retention_days: resolveCallLogRetentionDays(),
  };
}
export function daemonControl(op: "stop" | "restart") {
  requestControl(op);
  return { accepted: true };
}

type Def = {
  key: string;
  name: string;
  default: string;
  description: string;
  restart_required?: boolean;
  read?: () => string | null;
  write?: (value: string, native: Native) => Promise<void> | void;
  check?: (value: string) => void;
};
const bool = (value: string) => {
  if (value !== "true" && value !== "false") throw invalid("只能是 true 或 false");
};
const positive = (value: string) => {
  if (!/^\d+$/.test(value) || Number(value) <= 0) throw invalid("需要正整数");
};
const DEFS: Def[] = [
  {
    key: "gateway.direct_network",
    name: "允许连接上游",
    default: "true",
    description: "允许网关连接上游服务商。关闭后只能访问本机（离线排查用）。空闲时不会主动连接。",
    restart_required: true,
    check: bool,
    read: () => (instanceConfig().config?.network === "direct" ? "true" : "false"),
    write: (value) => writeInstanceConfig({ network: value === "true" ? "direct" : "loopback-only" }),
  },
  {
    key: "latency.ultra_fast_ms",
    name: "“超快”档上限（毫秒）",
    default: "800",
    description: "首字延迟不超过这个毫秒数算“超快”档。",
    check: positive,
  },
  {
    key: "latency.fast_ms",
    name: "“快”档上限（毫秒）",
    default: "2000",
    description: "首字延迟不超过这个毫秒数算“快”档。",
    check: positive,
  },
  {
    key: "catalog.radar_opt_in",
    name: "自动检查免费模型目录",
    default: "false",
    description: "每小时检查一次 OmniRoute 免费模型目录。只下载签名目录，不改账号和路由。",
    check: bool,
    write: async (value, native) => {
      await native("/api/radar/settings", "POST", { optIn: value === "true" });
    },
  },
  {
    key: "pricing.auto_sync",
    name: "自动同步价格表",
    default: "false",
    description: "每 24 小时从 LiteLLM 同步一次价格表。",
    restart_required: true,
    check: bool,
    read: () => (instanceConfig().config?.pricingSync === true ? "true" : "false"),
    write: (value) => writeInstanceConfig({ pricingSync: value === "true" }),
  },
  {
    key: "subpool.remember_active",
    name: "记住当前号池",
    default: "true",
    description: "重启后保留当前选择的号池；关闭后每次启动回到主号池。",
    check: bool,
    read: () => String(subpoolState().remember),
    write: (value) => subpoolRemember(value === "true"),
  },
];
const stored = () => kvGet<Record<string, string>>("settings", {});

function settingOf(def: Def) {
  const own = stored();
  const value = def.read?.() ?? own[def.key] ?? def.default;
  return {
    key: def.key,
    name: def.name,
    value,
    default: def.default,
    is_default: value === def.default,
    description: def.description,
    restart_required: !!def.restart_required,
  };
}
export const settingsList = () => DEFS.map(settingOf);
export async function settingSet(native: Native, key: string, value: string) {
  const def = DEFS.find((d) => d.key === key);
  if (!def) throw invalid(`未知设置 ${key}`);
  def.check?.(value);
  await def.write?.(value, native);
  const own = stored();
  own[key] = value;
  kvSet("settings", own);
  if (key.startsWith("latency.")) kvSet(`setting.${key}`, Number(value));
  return settingOf(def);
}
export async function settingReset(native: Native, key: string) {
  const def = DEFS.find((d) => d.key === key);
  if (!def) throw invalid(`未知设置 ${key}`);
  return settingSet(native, key, def.default);
}

export function backupDatabase(p: any) {
  const dir = path.join(instanceDir(), "backups");
  fs.mkdirSync(dir, { recursive: true });
  const stamp = new Date().toISOString().replace(/[-:]/g, "").slice(0, 15);
  const target = String(p?.path ?? "").trim() || path.join(dir, `loadout-${stamp}.sqlite`);
  if (!path.isAbsolute(target) || fs.existsSync(target)) throw invalid("需要一个不存在的完整路径");
  getDbInstance().prepare("VACUUM INTO ?").run(target);
  return { path: target };
}

const SIGN_IN_HOSTS = new Set(["auth.openai.com", "chatgpt.com", "auth0.openai.com"]);
export function openSignInPage(url: string) {
  const parsed = new URL(url);
  if (parsed.protocol !== "https:" || !SIGN_IN_HOSTS.has(parsed.hostname))
    throw invalid("只能打开官方登录页");
  spawn("explorer.exe", [parsed.href], { detached: true, stdio: "ignore", windowsHide: true }).unref();
  return { opened: true };
}

export async function scanTools() {
  const home = process.env.LOADOUT_USER_HOME ?? "";
  const local = process.env.LOADOUT_USER_LOCALAPPDATA ?? "";
  const providers = await snapshot();
  const has = (slug: string) => providers.some((p) => p.slug === slug);
  const exists = (file: string) => !!file && fs.existsSync(file);
  const codexAuth = path.join(home, ".codex", "auth.json");
  const claudeAuth = path.join(home, ".claude", ".credentials.json");
  const workbuddyAuth = path.join(
    local,
    "CodeBuddyExtension",
    "Data",
    "Public",
    "auth",
    "workbuddy-desktop-ai.info"
  );
  const wbConfig = instanceConfig().config?.workbuddy;
  return [
    {
      kind: "workbuddy",
      display_name: "WorkBuddy",
      installed: exists(workbuddyAuth) || !!wbConfig,
      executable: wbConfig?.exe ?? null,
      logged_in: exists(workbuddyAuth),
      auth: exists(workbuddyAuth) ? "桌面端登录" : null,
      detail: wbConfig ? "连接器已启用" : null,
      expired: false,
      config_path: null,
      imported: has("workbuddy") && !!wbConfig,
      inference: true,
    },
    {
      kind: "codex",
      display_name: "Codex CLI",
      installed: exists(codexAuth),
      executable: null,
      logged_in: exists(codexAuth),
      auth: exists(codexAuth) ? "已登录" : null,
      detail: "只读状态；官方 CLI 的登录不会被导入。需要转发请添加“ChatGPT / Codex 订阅”。",
      expired: false,
      config_path: null,
      imported: false,
      inference: false,
    },
    {
      kind: "claude-code",
      display_name: "Claude Code",
      installed: exists(claudeAuth),
      executable: null,
      logged_in: exists(claudeAuth),
      auth: exists(claudeAuth) ? "已登录" : null,
      detail: "只读状态；订阅凭证不用于转发请求。",
      expired: false,
      config_path: null,
      imported: false,
      inference: false,
    },
  ];
}

const workbuddyAuth = () =>
  path.join(
    process.env.LOADOUT_USER_LOCALAPPDATA ?? "",
    "CodeBuddyExtension",
    "Data",
    "Public",
    "auth",
    "workbuddy-desktop-ai.info"
  );
function workbuddyExe() {
  const common = path.join(process.env.LOADOUT_USER_LOCALAPPDATA ?? "", "Programs", "WorkBuddyAI", "WorkBuddyAI.exe");
  if (fs.existsSync(common)) return common;
  const found = spawnSync(
    "powershell.exe",
    ["-NoProfile", "-Command", "(Get-Process WorkBuddyAI -ErrorAction SilentlyContinue | Select-Object -First 1).Path"],
    { encoding: "utf8", windowsHide: true, timeout: 15000 }
  );
  const exe = found.stdout?.trim();
  if (exe && fs.existsSync(exe)) return exe;
  throw invalid("没有找到 WorkBuddy。先打开 WorkBuddy 桌面端再试。");
}
const freePort = () =>
  new Promise<number>((resolve, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address() as { port: number };
      server.close(() => resolve(port));
    });
  });

/** Binds the connector to the account signed in on this PC; the supervisor then restarts it. */
export async function connectTool(kind: string) {
  if (kind !== "workbuddy") throw invalid("这个工具不能用来转发请求");
  let uid: unknown;
  try {
    uid = JSON.parse(fs.readFileSync(workbuddyAuth(), "utf8")).account?.uid;
  } catch {
    throw invalid("没有读到 WorkBuddy 的登录信息。先在 WorkBuddy 桌面端登录。");
  }
  if (!uid) throw invalid("WorkBuddy 还没有登录。");
  const exe = workbuddyExe();
  const current = instanceConfig().config?.workbuddy;
  writeInstanceConfig({
    workbuddy: {
      authPath: workbuddyAuth(),
      exe,
      accountHash: createHash("sha256").update(`loadout-workbuddy-account\0${uid}`).digest("hex"),
      // Replaced at start by what is actually installed.
      clientVersion: current?.clientVersion ?? "5.9.4",
      cliVersion: current?.cliVersion ?? "2.165.0",
      port: current?.port ?? (await freePort()),
    },
  });
  requestControl("restart", "workbuddy");
  return { restarting: true };
}
