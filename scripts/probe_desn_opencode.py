# SPDX-License-Identifier: Apache-2.0
"""Bounded isolated DESN131 gateway comparison; no production writes."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
from probe_opencode_403 import official_cli, now, MODEL, PROMPT, DEPLOYED

REV = 'b48d93ef91a6eae3c71f04b99a49294e7af9d28a'
OUT = Path(__file__).resolve().parents[1] / ('docs/research/desn-opencode-hotspot-20261018.json' if os.environ.get('LOADOUT_HOTSPOT_EXTENDED') == '1' else 'docs/research/desn-opencode-live-20261018.json')


def main():
    if os.environ.get('LOADOUT_LIVE_TESTS') != '1':
        raise RuntimeError('Explicit live opt-in required')
    result = {'revision': REV, 'at': now(), 'model': MODEL, 'source_modified': False,
              'upstream': 'https://opencode.ai/zen/v1', 'upstream_auth': 'public',
              'model_fallback': False, 'production_changed': False, 'cases': []}
    result['official_sha256'] = hashlib.sha256((DEPLOYED/'opencode.exe').read_bytes()).hexdigest()
    with tempfile.TemporaryDirectory(prefix='loadout-desn-probe-') as temp:
        root = Path(temp)
        source = root/'opencode-zen-proxy.mjs'
        with urllib.request.urlopen(f'https://raw.githubusercontent.com/DESN131/opencode-zen-proxy/{REV}/opencode-zen-proxy.mjs', timeout=30) as response:
            source.write_bytes(response.read())
        result['source_sha256'] = hashlib.sha256(source.read_bytes()).hexdigest()
        node = shutil.which('node')
        result['node_version'] = subprocess.check_output([node, '--version'], text=True).strip()
        result['cases'].append(official_cli())
        print(json.dumps(result['cases'][-1]), flush=True)
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        env = {k:v for k,v in os.environ.items() if not k.startswith(('OPENCODE', 'NODE_', 'HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY'))}
        env.update(PORT=str(port), HOST='127.0.0.1')
        spawned = subprocess.run([str(DEPLOYED/'spawn_detached.exe'), node, str(source)], cwd=root,
                                 env=env, capture_output=True, text=True, check=True, timeout=10)
        pid = int(spawned.stdout.strip())
        result['isolated_pid'] = pid
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

        def call(path, body=None):
            request = urllib.request.Request(f'http://127.0.0.1:{port}'+path,
                data=json.dumps(body).encode() if body is not None else None,
                headers={'Content-Type':'application/json'})
            try:
                with opener.open(request, timeout=45) as response:
                    return response.status, response.read().decode()
            except urllib.error.HTTPError as error:
                return error.code, error.read().decode()

        try:
            deadline = time.monotonic()+15
            while True:
                try:
                    status, body = call('/health')
                    if status == 200 and json.loads(body).get('ok'): break
                except OSError: pass
                if time.monotonic()>deadline: raise RuntimeError('Startup timeout')
                time.sleep(.2)
            result['health_ok'] = True
            status, body = call('/v1/models')
            result['model_catalog_status'] = status
            if status == 200:
                result['exact_model_listed'] = MODEL in [m['id'] for m in json.loads(body).get('data',[])]
            start = time.monotonic()
            at = now()
            status, body = call('/v1/chat/completions', {'model':MODEL,
                'messages':[{'role':'user','content':PROMPT}], 'stream':False})
            try: parsed = json.loads(body)
            except ValueError: parsed = {'raw_error':body[:2000]}
            text = ((parsed.get('choices') or [{}])[0].get('message') or {}).get('content')
            case = {'path':'desn_proxy_chat', 'at':at, 'status':status, 'response':parsed,
                    'seconds':round(time.monotonic()-start,3),
                    'passed':status==200 and (text or '').strip()=='HELLO'}
            result['cases'].append(case)
            result['extended_tests'] = 'pending' if case['passed'] else 'skipped: basic inference failed'
            print(json.dumps(case), flush=True)
            if case['passed'] and os.environ.get('LOADOUT_HOTSPOT_EXTENDED') == '1':
                from verify_hotspot_capabilities import verify_capabilities
                result['capabilities'] = verify_capabilities(f'http://127.0.0.1:{port}/v1', 'public', MODEL)
                result['extended_tests'] = 'completed'
        except Exception as error:
            result['probe_error'] = type(error).__name__+': '+str(error)
        finally:
            command = "$p=Get-CimInstance Win32_Process -Filter 'ProcessId="+str(pid)+"';if($p){if($p.ExecutablePath -ne $env:EXPECTED_EXE -or -not $p.CommandLine.Contains($env:EXPECTED_SCRIPT)){throw 'Ownership mismatch'};Stop-Process -Id $p.ProcessId -Force}"
            subprocess.run(['powershell','-NoProfile','-Command',command],
                env=dict(os.environ,EXPECTED_EXE=node,EXPECTED_SCRIPT=str(source)),
                capture_output=True,check=True,timeout=15)
            result['isolated_process_stopped'] = True
    result['temporary_directory_removed'] = not root.exists()
    result['cases'].append(official_cli())
    print(json.dumps(result['cases'][-1]), flush=True)
    OUT.write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding='utf-8')
    print(OUT, flush=True)


if __name__ == '__main__':
    main()
