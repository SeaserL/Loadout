# SPDX-License-Identifier: Apache-2.0
"""Finite opt-in restart test using a separate official binary and memory database."""
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.request
import urllib.error
import uuid
from opencode_bridge import Official
from workbuddy_bridge_key import protect


def main():
    if os.environ.get('LOADOUT_LIVE_TESTS') != '1':
        raise RuntimeError('Live opt-in required')
    deployed = Path(os.environ['LOCALAPPDATA']) / 'LoadoutOpenCodeBridge'
    scripts = Path(__file__).resolve().parent
    model = 'deepseek-v4.1-flash-free'
    report = {'model': model, 'isolated': True, 'rounds': []}
    output = scripts.parent / 'docs/research/opencode-deepseek-restart-20261018.json'
    with tempfile.TemporaryDirectory(prefix='loadout-opencode-restart-') as temp:
        root = Path(temp)
        config = json.loads((deployed / 'binding.json').read_text(encoding='utf-8-sig'))
        sockets = [socket.socket(), socket.socket()]
        for sock in sockets: sock.bind(('127.0.0.1', 0))
        config.update(port=sockets[0].getsockname()[1], official_port=sockets[1].getsockname()[1])
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
        def start():
            subprocess.run(['powershell', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', str(root / 'Start-OpenCodeBridge.ps1'), '-BridgeDir', str(root)], check=True, capture_output=True, timeout=60)
        def stop():
            for name, expected in [('bridge', Path(config['python']).resolve()), ('official', root / 'opencode.exe')]:
                pid_file = root / (name + '.pid')
                if not pid_file.exists(): continue
                pid = int(pid_file.read_text(encoding='utf-8-sig').strip())
                command = "$p=Get-Process -Id " + str(pid) + " -ErrorAction SilentlyContinue; if($p){if($p.Path -ne $env:EXPECTED_EXE){throw 'Ownership mismatch'}; Stop-Process -Id $p.Id -Force; $p.WaitForExit()}"
                subprocess.run(['powershell', '-NoProfile', '-Command', command], env=dict(os.environ, EXPECTED_EXE=str(expected)), check=True, capture_output=True, timeout=15)
        def complete(messages, label):
            body = {'model': model, 'messages': messages}
            req = urllib.request.Request(f"http://127.0.0.1:{config['port']}/v1/chat/completions", json.dumps(body).encode(), headers={'Authorization':'Bearer '+keys['bridge'], 'Content-Type':'application/json'})
            started = time.monotonic()
            with urllib.request.urlopen(req, timeout=60) as response: result=json.load(response)
            text=result['choices'][0]['message']['content']
            report['rounds'].append({'label':label,'model':result['model'],'text':text,'seconds':round(time.monotonic()-started,3)})
            print(json.dumps(report['rounds'][-1]), flush=True)
            return text
        try:
            start()
            api=Official(config['official_port'], keys['official'])
            old=api.call('POST','/api/session',{'model':{'id':model,'providerID':'opencode'}})['data']['id']
            token='RESTART_'+uuid.uuid4().hex[:12]
            history=[{'role':'user','content':f'Reply with exactly {token}. Do not use tools.'}]
            answer=complete(history,'before_restart')
            assert answer.strip()==token
            history += [{'role':'assistant','content':answer},{'role':'user','content':'Repeat your previous answer exactly. Do not use tools.'}]
            stop()
            start()
            req=urllib.request.Request(api.base+'/api/session/'+old, headers={'Authorization':api.auth})
            try:
                urllib.request.urlopen(req,timeout=5).close()
                raise RuntimeError('Memory session survived restart unexpectedly')
            except urllib.error.HTTPError as error:
                report['old_session_http_status']=error.code
                assert error.code==404
            answer=complete(history,'full_history_after_restart')
            assert answer.strip()==token
            report['passed']=True
        finally:
            stop()
            report['isolated_processes_stopped']=True
            output.write_text(json.dumps(report,indent=2),encoding='utf-8')


if __name__=='__main__': main()
