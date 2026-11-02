// SPDX-License-Identifier: Apache-2.0
// Port of crates/providers/src/codex.rs: metadata only, never imports CLI tokens.
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
const cache = new Map();
export function windowsFrom(limits, eventTime = Date.now()) {
  return ["primary", "secondary"].flatMap((name) => {
    const w = limits?.[name], used = w?.usedPercent ?? w?.used_percent;
    if (typeof used !== "number" || !Number.isFinite(used)) return [];
    const mins = w.windowDurationMins ?? w.window_minutes ?? w.windowMinutes;
    const seconds = w.resetsAt ?? w.resets_at;
    return [{ scope: mins === 300 ? "5h" : mins === 10080 ? "weekly" : mins == null ? "window" : `${Math.round(mins)}m`,
      used: Math.min(100, Math.max(0, used)), windowSeconds: mins == null ? null : Math.round(mins) * 60,
      resetsAt: seconds != null ? Number(seconds) * 1000 :
        w.resets_in_seconds != null || w.resetsInSeconds != null ? eventTime + Number(w.resets_in_seconds ?? w.resetsInSeconds) * 1000 : null }];
  });
}
export async function appServer(executable, home, args = ["app-server"]) {
  const child = spawn(executable, args, { windowsHide: true, shell: false, stdio: ["pipe", "pipe", "ignore"],
    env: { ...process.env, CODEX_HOME: home } });
  const lines = createInterface({ input: child.stdout });
  const waiters = new Map();
  let failed = false;
  const fail = () => { failed = true; for (const w of waiters.values()) w.reject(Error("SCAN_RPC_FAILED")); waiters.clear(); };
  child.on("error", fail); child.on("exit", fail); child.stdin.on("error", fail);
  lines.on("line", (line) => {
    if (line.length > 1024 * 1024) return fail();
    try { const v = JSON.parse(line), w = waiters.get(v.id); if (!w) return;
      waiters.delete(v.id); v.error ? w.reject(Error("SCAN_RPC_FAILED")) : w.resolve(v.result); } catch {}
  });
  const send = (method, id, params) => {
    if (failed) return Promise.reject(Error("SCAN_RPC_FAILED"));
    const result = new Promise((resolve, reject) => waiters.set(id, { resolve, reject }));
    child.stdin.write(JSON.stringify({ method, id, params }) + "\n"); return result;
  };
  const timer = setTimeout(fail, 15000);
  try {
    await send("initialize", 0, { clientInfo: { name: "loadout", title: "Loadout", version: "1.0.0" } });
    child.stdin.write(JSON.stringify({ method: "initialized", params: {} }) + "\n");
    const account = await send("account/read", 1).catch(() => null);
    for (const id of [2, 3]) {
      const result = await send("account/rateLimits/read", id), limits = result?.rateLimits ?? result;
      const windows = windowsFrom(limits);
      if (windows.length) return { source: "official-cli", confidence: "exact", windows,
        plan: limits?.planType ?? account?.account?.planType ?? null };
      await new Promise((r) => setTimeout(r, 400));
    }
    throw Error("SCAN_WINDOWS_MISSING");
  } finally { clearTimeout(timer); lines.close(); child.stdin.destroy(); child.kill(); }
}
export function sessionUsage(home) {
  const files = [], stack = [path.join(home, "sessions")];
  while (stack.length) {
    const dir = stack.pop();
    if (!fs.existsSync(dir)) continue;
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      if (entry.isSymbolicLink()) continue;
      const p = path.join(dir, entry.name);
      if (entry.isDirectory()) stack.push(p); else if (entry.name.endsWith(".jsonl")) files.push(p);
    }
  }
  for (const file of files.sort().reverse().slice(0, 5)) {
    for (const line of fs.readFileSync(file, "utf8").split("\n").reverse()) {
      if (!line.includes("rate_limits")) continue;
      try {
        const v = JSON.parse(line), p = v.payload ?? v, limits = p.rate_limits ?? p.info?.rate_limits;
        const windows = windowsFrom(limits, Date.parse(v.timestamp) || Date.now());
        if (windows.length) return { source: "local-files", confidence: "estimated", windows };
      } catch {}
    }
  }
  throw Error("SCAN_WINDOWS_MISSING");
}
export async function scanSubscription({ home = process.env.CODEX_HOME || path.join(os.homedir(), ".codex"), executable = "codex.exe" } = {}) {
  if (!path.isAbsolute(home)) throw Error("ABSOLUTE_SOURCE_REQUIRED");
  const id = home + "\0" + executable, previous = cache.get(id);
  if (previous && previous.at + 600000 > Date.now()) return previous.value;
  let loggedIn = false, authMode = null;
  try { const auth = JSON.parse(fs.readFileSync(path.join(home, "auth.json"), "utf8"));
    loggedIn = !!auth; authMode = auth.auth_mode ?? (auth.OPENAI_API_KEY != null ? "api_key" : "chatgpt"); } catch {}
  let usage;
  try { usage = await appServer(executable, home); } catch {
    try { usage = sessionUsage(home); } catch { usage = { source: "unavailable", confidence: "unknown", windows: [] }; }
  }
  const value = { provider: "codex", readOnly: true, loggedIn, authMode, observedAt: Date.now(), usage };
  cache.set(id, { at: Date.now(), value }); return value;
}
