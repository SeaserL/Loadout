// SPDX-License-Identifier: Apache-2.0
import net from 'node:net';
import fs from 'node:fs/promises';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

const execute = promisify(execFile);
const root = path.join(process.env.LOCALAPPDATA, 'LoadoutWorkBuddyBridge');
const pipe = '\\\\.\\pipe\\loadout-workbuddy-watch-' + crypto.createHash('sha256').update(root.toLowerCase()).digest('hex').slice(0, 24);
const singleton = net.createServer(socket => socket.destroy());
singleton.on('error', () => { process.exitCode = 1; });
singleton.listen(pipe, async () => {
  // Node can run without a console. Short-lived PowerShell/CLI children receive private
  // stdio and windowsHide; background processes still use the platform detached launcher.
  const options = { windowsHide: true, timeout: 45000, maxBuffer: 64 * 1024 };
  for (;;) {
    let state = 'waiting_for_loadout';
    try {
      const config = JSON.parse((await fs.readFile(path.join(root, 'autostart.json'), 'utf8')).replace(/^\uFEFF/, ''));
      const { stdout } = await execute(config.loadout, ['daemon', 'status', '--json'], options);
      if (JSON.parse(stdout).gateway?.listening === true) {
        const powershell = path.join(process.env.SystemRoot, 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe');
        await execute(powershell, ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', path.join(root, 'Start-WorkBuddyBridge.ps1')], options);
        state = 'ready';
      }
    } catch { state = 'retry_pending'; }
    try {
      await fs.writeFile(path.join(root, 'autostart-status.json'), JSON.stringify({ state, checked_at: new Date().toISOString(), watcher_pid: process.pid }));
    } catch { /* Status errors must not stop recovery. */ }
    await new Promise(resolve => setTimeout(resolve, 15000));
  }
});
