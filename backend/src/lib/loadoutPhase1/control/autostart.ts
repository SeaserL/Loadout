// SPDX-License-Identifier: Apache-2.0
// Start at sign-in (ADR-002): one value under the per-user Run key that starts the window program
// in the background. It lives here because the console talks to nothing but the backend.
import path from "node:path";
import { execFile } from "node:child_process";
import { RpcError, instanceDir } from "./shared";
import { instanceConfig } from "./system";

const RUN_KEY = "HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
// PowerShell, not reg.exe: reg prints in the console code page and garbles a Chinese folder name.
// Every value goes in through the environment, never into the script text.
const SCRIPT = [
  "$ErrorActionPreference = 'Stop'",
  "$ProgressPreference = 'SilentlyContinue'",
  "[Console]::OutputEncoding = [Text.Encoding]::UTF8",
  "$key = $env:LOADOUT_AUTOSTART_KEY",
  "if ($env:LOADOUT_AUTOSTART_ACTION -eq 'set') {",
  "  if (-not (Test-Path -LiteralPath $key)) { $null = New-Item -Path $key -Force }",
  "  Set-ItemProperty -LiteralPath $key -Name 'Loadout' -Value $env:LOADOUT_AUTOSTART_COMMAND",
  "} elseif ($env:LOADOUT_AUTOSTART_ACTION -eq 'remove') {",
  "  Remove-ItemProperty -LiteralPath $key -Name 'Loadout' -ErrorAction SilentlyContinue",
  "}",
  "$value = (Get-ItemProperty -LiteralPath $key -Name 'Loadout' -ErrorAction SilentlyContinue).Loadout",
  "if ($value) { [Console]::Out.Write($value) }",
].join("\n");
const ENCODED = Buffer.from(SCRIPT, "utf16le").toString("base64");

// Tests use a throwaway key; runtime.mjs accepts nothing but HKCU:\Software\Loadout-Test\<name>.
const key = () => (instanceConfig().config?.autostartKey as string | undefined) ?? RUN_KEY;
/** What runs at sign-in: this package's window program, for this instance, with no window. */
const command = () => `"${path.join(process.cwd(), "Loadout.exe")}" --instance "${instanceDir()}" --background`;

function registry(action: "read" | "set" | "remove"): Promise<string> {
  const shell = path.join(process.env.SystemRoot ?? "C:\\Windows", "System32", "WindowsPowerShell", "v1.0", "powershell.exe");
  return new Promise((resolve, reject) => {
    execFile(
      shell,
      ["-NoProfile", "-NonInteractive", "-EncodedCommand", ENCODED],
      {
        env: { ...process.env, LOADOUT_AUTOSTART_KEY: key(), LOADOUT_AUTOSTART_ACTION: action, LOADOUT_AUTOSTART_COMMAND: command() },
        windowsHide: true,
        timeout: 20000,
        encoding: "utf8",
      },
      (error, stdout) => (error ? reject(new RpcError("internal", "没能读写开机启动设置")) : resolve(String(stdout).trim())),
    );
  });
}

export async function autostartStatus() {
  return { enabled: (await registry("read")) === command() };
}
export async function autostartSet(enabled: boolean) {
  if (enabled) await registry("set");
  // Only our own value goes: another program's entry under this name is not ours to delete.
  else if ((await registry("read")) === command()) await registry("remove");
  return autostartStatus();
}
