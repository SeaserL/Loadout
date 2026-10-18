# SPDX-License-Identifier: Apache-2.0
"""Opt-in, isolated third-party gateway versus official CLI live control."""
import hashlib
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request

from probe_opencode_403 import official_cli, now, MODEL, PROMPT, DEPLOYED

ROOT = Path(tempfile.gettempdir()) / 'loadout-opencode2api-live-83d623d'
REV = '83d623d541c47a17cd832ee4f53bd7c95f3a18fa'
OUT = Path(__file__).resolve().parents[1] / 'docs/research/opencode2api-live-20261018.json'


def main():
    if os.environ.get('LOADOUT_LIVE_TESTS') != '1':
        raise RuntimeError('Explicit live opt-in required')
    binary = ROOT / 'opencode2api.exe'
    result = {'revision': REV, 'at': now(), 'model': MODEL,
              'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'source_patch': 'Disable manager.startAvailabilityChecks only',
              'proxy': 'direct', 'upstream_credentials': 'anonymous public only',
              'production_changed': False, 'cases': []}
    result['cases'].append(official_cli())
    print(json.dumps(result['cases'][-1]), flush=True)
    with tempfile.TemporaryDirectory(prefix='loadout-o2a-state-') as temp:
        state = Path(temp)
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        key = secrets.token_urlsafe(32)
        cfg = json.loads((ROOT / ('opencode2api-' + REV) / 'config.example.json').read_text())
        cfg.update(listen=f'127.0.0.1:{port}', server_keys=[key], zen_keys=[], go_keys=[],
                   anonymous=True, proxies=['direct'], proxyfile='', prefer='zen')
        cfg['webui']['enabled'] = False
        cfg['retry'] = {'max_attempts': 1, 'timeout_seconds': 45}
        cfg['models']['refresh_seconds'] = 86400
        cfg['models']['free_only'] = True
        cfg['logging'].update(level='debug', dump_request_bodies=False)
        cfg['rotation']['sota']['order'] = []
        cfg['rotation']['sweet']['order'] = []
        (state / 'config.json').write_text(json.dumps(cfg), encoding='utf-8')
        env = {k: v for k, v in os.environ.items() if not k.startswith(('OPENCODE', 'ZEN_', 'GO_KEYS', 'SERVER_KEYS', 'LISTEN_', 'WEBUI_', 'HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY'))}
        spawned = subprocess.run([str(DEPLOYED / 'spawn_detached.exe'), str(binary), '-config', str(state / 'config.json')],
                                 cwd=state, env=env, capture_output=True, text=True, check=True, timeout=10)
        pid = int(spawned.stdout.strip())
        result['isolated_pid'] = pid
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

        def request(path, body=None):
            req = urllib.request.Request(f'http://127.0.0.1:{port}' + path,
                data=json.dumps(body).encode() if body is not None else None,
                headers={'Authorization': 'Bearer ' + key, 'Content-Type': 'application/json'})
            try:
                with opener.open(req, timeout=55) as response:
                    return response.status, response.read().decode()
            except urllib.error.HTTPError as error:
                return error.code, error.read().decode()

        try:
            deadline = time.monotonic() + 40
            while True:
                try:
                    status, body = request('/v1/models')
                    models = [m['id'] for m in json.loads(body).get('data', [])]
                    if status == 200 and MODEL in models:
                        break
                except (OSError, ValueError):
                    pass
                if time.monotonic() > deadline:
                    raise RuntimeError('Model discovery timeout')
                time.sleep(.3)
            result['exact_model_listed'] = True
            started = time.monotonic()
            status, body = request('/v1/chat/completions', {'model': MODEL,
                'messages': [{'role': 'user', 'content': PROMPT}], 'stream': False})
            parsed = json.loads(body)
            text = ((parsed.get('choices') or [{}])[0].get('message') or {}).get('content')
            case = {'path': 'opencode2api_json', 'at': now(), 'status': status,
                    'seconds': round(time.monotonic() - started, 3), 'response': parsed,
                    'passed': status == 200 and (text or '').strip() == 'HELLO'}
            result['cases'].append(case)
            print(json.dumps(case), flush=True)
            result['extended_tests'] = 'pending' if case['passed'] else 'skipped: basic inference failed'
        except Exception as error:
            result['probe_error'] = type(error).__name__ + ': ' + str(error)
        finally:
            command = "$p=Get-Process -Id " + str(pid) + " -ErrorAction SilentlyContinue;if($p){if($p.Path -ne $env:EXPECTED_EXE){throw 'Ownership mismatch'};Stop-Process -Id $p.Id -Force;$p.WaitForExit()}"
            subprocess.run(['powershell', '-NoProfile', '-Command', command],
                           env=dict(os.environ, EXPECTED_EXE=str(binary)), check=True, capture_output=True, timeout=15)
            result['isolated_process_stopped'] = True
    result['cases'].append(official_cli())
    print(json.dumps(result['cases'][-1]), flush=True)
    OUT.write_text(json.dumps(result, indent=2, ensure_ascii=False), encoding='utf-8')
    print(str(OUT), flush=True)


if __name__ == '__main__':
    main()
