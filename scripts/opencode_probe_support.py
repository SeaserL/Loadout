# SPDX-License-Identifier: Apache-2.0
"""Isolated official server for explicitly opted-in live probes."""
from contextlib import contextmanager
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
from opencode_bridge import Official
from workbuddy_bridge_key import protect


@contextmanager
def isolated_server():
    if os.environ.get('LOADOUT_LIVE_TESTS') != '1':
        raise RuntimeError('Live opt-in required')
    deployed = Path(os.environ['LOCALAPPDATA']) / 'LoadoutOpenCodeBridge'
    scripts = Path(__file__).resolve().parent
    with tempfile.TemporaryDirectory(prefix='loadout-opencode-probe-') as temp:
        root = Path(temp)
        config = json.loads((deployed / 'binding.json').read_text(encoding='utf-8-sig'))
        sockets = [socket.socket(), socket.socket(), socket.socket()]
        for sock in sockets: sock.bind(('127.0.0.1', 0))
        config.update(port=sockets[0].getsockname()[1], official_port=sockets[1].getsockname()[1])
        config['official_ports'] = [sock.getsockname()[1] for sock in sockets[1:]]
        for sock in sockets: sock.close()
        (root / 'binding.json').write_text(json.dumps(config), encoding='utf-8')
        (root / 'workspace').mkdir()
        for name in ('opencode.exe', 'spawn_detached.exe'):
            shutil.copy2(deployed / name, root / name)
        for name in ('opencode_bridge.py', 'opencode_protocol.py', 'opencode_events.py', 'opencode_controls.mjs', 'opencode_bridge_setup.py', 'workbuddy_bridge_key.py', 'Start-OpenCodeBridge.ps1'):
            shutil.copy2(scripts / name, root / name)
        env = dict(os.environ, LOADOUT_OPENCODE_DIR=str(root))
        subprocess.run([config['python'], str(root / 'opencode_bridge_setup.py'), 'init'], env=env, check=True, capture_output=True)
        keys = json.loads(protect((root / 'secrets.dpapi').read_bytes(), True))
        try:
            subprocess.run(['powershell', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', str(root / 'Start-OpenCodeBridge.ps1'), '-BridgeDir', str(root)], check=True, capture_output=True, timeout=60)
            yield Official(config['official_port'], keys['official']), root
        finally:
            for name, expected in [('bridge', Path(config['python']).resolve()), ('workers/1/official', root / 'opencode.exe'), ('official', root / 'opencode.exe')]:
                pid_file = root / (name + '.pid')
                if not pid_file.exists(): continue
                pid = int(pid_file.read_text(encoding='utf-8-sig').strip())
                command = "$p=Get-Process -Id " + str(pid) + " -ErrorAction SilentlyContinue; if($p){if($p.Path -ne $env:EXPECTED_EXE){throw 'Ownership mismatch'}; Stop-Process -Id $p.Id -Force; $p.WaitForExit()}"
                subprocess.run(['powershell', '-NoProfile', '-Command', command], env=dict(os.environ, EXPECTED_EXE=str(expected)), check=True, capture_output=True, timeout=15)
