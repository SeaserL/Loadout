// SPDX-License-Identifier: Apache-2.0
// Software updates (ADR-020). The work happens in tools/updater.mjs, a short-lived process of its
// own: this file only starts it and reads what it left behind.
import fs from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";
import { instanceDir, invalid, kvGet, kvSet } from "./shared";
import { instanceConfig } from "./system";

const CHECK_EVERY_MS = 6 * 3600 * 1000;
const INSTALL_GIVE_UP_MS = 10 * 60 * 1000;
const RESULT_SHOWN_MS = 7 * 24 * 3600 * 1000;
const BUSY = new Set(["checking", "downloading", "extracting", "verifying", "installing"]);
const NOTES = "https://github.com/SeaserL/Loadout/releases/tag/v";

const packageRoot = () => process.cwd();
const updateDir = () => path.join(path.dirname(packageRoot()), "loadout-update");
const read = (name: string): any => {
  try {
    return JSON.parse(fs.readFileSync(path.join(updateDir(), name), "utf8").replace(/^\uFEFF/, ""));
  } catch {
    return null;
  }
};
const alive = (pid: unknown) => {
  try {
    process.kill(Number(pid), 0);
    return true;
  } catch {
    return false;
  }
};
const feed = () => instanceConfig().config?.updateFeed as string | undefined;
const allowed = () => instanceConfig().config?.network === "direct" || !!feed();
const autoCheck = () => kvGet<{ auto_check?: boolean }>("updates", {}).auto_check !== false;

function launch(action: "check" | "download" | "install") {
  const home = process.env.LOADOUT_USER_HOME ?? "";
  const local = process.env.LOADOUT_USER_LOCALAPPDATA ?? "";
  // What a program needs and the owner's real folders. Not the backend's secrets, and not its
  // egress guard: this process keeps its own short list of hosts.
  const env: Record<string, string> = {
    USERPROFILE: home,
    LOCALAPPDATA: local,
    APPDATA: path.join(home, "AppData", "Roaming"),
    TEMP: path.join(local, "Temp"),
    TMP: path.join(local, "Temp"),
    LOADOUT_USER_HOME: home,
    LOADOUT_USER_LOCALAPPDATA: local,
  };
  for (const name of ["SystemRoot", "WINDIR", "COMSPEC", "PATH", "PATHEXT", "PROCESSOR_ARCHITECTURE"]) {
    const value = process.env[name];
    if (value) env[name] = value;
  }
  const args = [path.join(packageRoot(), "tools", "updater.mjs"), action, packageRoot(), instanceDir()];
  const url = feed();
  if (url) args.push(url);
  spawn(path.join(packageRoot(), "node.exe"), args, {
    cwd: packageRoot(),
    env,
    detached: true,
    stdio: "ignore",
    windowsHide: true,
  }).unref();
}

function current() {
  const saved = read("status.json") ?? {};
  let phase: string = saved.phase ?? "idle";
  let error: string | null = saved.error ?? null;
  const stale = phase === "installing" ? Date.now() - (saved.updated_at_ms ?? 0) > INSTALL_GIVE_UP_MS : BUSY.has(phase) && !alive(saved.pid);
  if (stale) {
    phase = "error";
    error = "上次的更新操作被中断了，请重试";
  }
  return { saved, phase, error };
}

/** Reading the status is what keeps it fresh: no timer, so a closed window means no traffic. */
export function updateStatus() {
  const version = process.env.LOADOUT_VERSION ?? "";
  const { saved, phase, error } = current();
  const on = allowed();
  if (on && autoCheck() && !BUSY.has(phase) && Date.now() - (saved.checked_at_ms ?? 0) > CHECK_EVERY_MS) launch("check");
  const result = read("result.json");
  const recent = result && Date.now() - (result.at_ms ?? 0) < RESULT_SHOWN_MS;
  const relevant = recent && (result.status === "installed" ? result.to === version : result.from === version);
  return {
    current_version: version,
    latest_version: saved.latest_version ?? null,
    phase: on ? phase : "disabled",
    progress: saved.progress ?? null,
    size: saved.size ?? null,
    release_url: saved.release_url ?? null,
    checked_at_ms: saved.checked_at_ms ?? null,
    error,
    last_result: relevant ? { status: result.status, from: result.from, to: result.to, at_ms: result.at_ms } : null,
    auto_check: autoCheck(),
  };
}
export function updateStart(action: "check" | "download" | "install") {
  if (!allowed()) throw invalid("本机模式下不能更新");
  const { phase } = current();
  if (BUSY.has(phase)) return updateStatus();
  if (action === "install" && phase !== "ready") throw invalid("没有准备好的更新");
  launch(action);
  return { ...updateStatus(), phase: action === "check" ? "checking" : action === "download" ? "downloading" : "installing" };
}
/** The page of the version on offer. It takes no address: this is not a way to open anything else. */
export function updateOpenNotes() {
  const version = String(read("status.json")?.latest_version ?? "");
  if (!allowed() || !/^\d+\.\d+\.\d+$/.test(version)) throw invalid("没有可以查看的更新说明");
  spawn("explorer.exe", [NOTES + version], { detached: true, stdio: "ignore", windowsHide: true }).unref();
  return { opened: true };
}
export function updateAutoCheck(enabled: boolean) {
  kvSet("updates", { auto_check: !!enabled });
  return updateStatus();
}
