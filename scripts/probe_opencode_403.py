# SPDX-License-Identifier: Apache-2.0
"""Bounded official CLI/API versus bridge comparison; never changes production."""
from contextlib import contextmanager
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import tempfile
import time
from opencode_bridge import Official
from opencode_events import EventFeed
from opencode_probe_support import isolated_server
from verify_opencode_agent import invoke
from workbuddy_bridge_key import protect

MODEL = 'deepseek-v4.1-flash-free'
PROMPT = 'Reply exactly HELLO. Do not use tools.'
DEPLOYED = Path(os.environ['LOCALAPPDATA']) / 'LoadoutOpenCodeBridge'


def now(): return datetime.now(timezone.utc).isoformat()


def environment(root):
    env = dict(os.environ)
    for name in list(env):
        if name.startswith(('OPENCODE_', 'XDG_', 'OPENAI_', 'ANTHROPIC_', 'LOADOUT_OPENCODE_')): env.pop(name)
    for name, folder in [('XDG_DATA_HOME','data'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_STATE_HOME','state'),('OPENCODE_TEST_HOME','home'),('OPENCODE_CONFIG_DIR','config/opencode')]:
        env[name] = str(root / folder)
    env.update(OPENCODE_DB=':memory:', OPENCODE_DISABLE_PROJECT_CONFIG='1', OPENCODE_DISABLE_CLAUDE_CODE='1', OPENCODE_DISABLE_AUTOUPDATE='1',
               OPENCODE_CONFIG_CONTENT=json.dumps({'permissions':[{'action':'*','resource':'*','effect':'ask'}]}))
    return env


def error_summary(error):
    return {k: error[k] for k in ('type','status','message') if k in error}


def official_cli(coding=False):
    record={'path':'official_cli_native_read' if coding else 'official_cli_no_plugin','at':now(),'model':MODEL,'plugins':[]}
    with tempfile.TemporaryDirectory(prefix='loadout-403-cli-') as temp:
        root=Path(temp); work=root/'workspace';work.mkdir()
        env=environment(root)
        prompt=PROMPT
        if coding:
            (work/'probe.txt').write_text('SYNTHETIC_CONTROL_7F92',encoding='utf-8')
            prompt='Work only in the current directory. Read probe.txt using the read tool and report its exact content. Do not modify files or use other tools.'
            env['OPENCODE_CONFIG_CONTENT']=json.dumps({'permissions':[{'action':'*','resource':'*','effect':'ask'},{'action':'read','resource':'*','effect':'allow'}]})
        start=time.monotonic()
        try:
            run=subprocess.run([str(DEPLOYED/'opencode.exe'),'run','--standalone','--model','opencode/'+MODEL,'--format','json',prompt],
                               cwd=work,env=env,capture_output=True,text=True,encoding='utf-8',errors='replace',timeout=60)
            events=[json.loads(line) for line in run.stdout.splitlines() if line.startswith('{')]
            record.update(exit=run.returncode,errors=[error_summary(e.get('error',{})) for e in events if e.get('type')=='error'],
                          text=''.join(e.get('part',{}).get('text','') for e in events if e.get('type')=='text'))
            record['tool_calls']=[e['part'].get('tool') for e in events if e.get('type')=='tool_use']
            record['passed']=run.returncode==0 and (('SYNTHETIC_CONTROL_7F92' in record['text'] and 'read' in record['tool_calls']) if coding else record['text'].strip()=='HELLO') and not record['errors']
            if not events:record['diagnostic']=run.stderr[-1000:]
        except subprocess.TimeoutExpired:record.update(passed=False,error='timeout')
        record['seconds']=round(time.monotonic()-start,3)
    return record


@contextmanager
def native_server():
    with tempfile.TemporaryDirectory(prefix='loadout-403-native-') as temp:
        root=Path(temp);(root/'workspace').mkdir()
        for name in ('opencode.exe','spawn_detached.exe'):shutil.copy2(DEPLOYED/name,root/name)
        with socket.socket() as sock:
            sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
        password=secrets.token_urlsafe(32)
        env=dict(environment(root),OPENCODE_SERVER_PASSWORD=password)
        spawned=subprocess.run([str(root/'spawn_detached.exe'),str(root/'opencode.exe'),'serve','--hostname','127.0.0.1','--port',str(port)],
                               cwd=root/'workspace',env=env,capture_output=True,text=True,check=True,timeout=10)
        pid=int(spawned.stdout.strip());api=Official(port,password)
        try:
            deadline=time.monotonic()+15
            while True:
                try:api.call('GET','/api/info');break
                except Exception:
                    if time.monotonic()>deadline:raise
                    time.sleep(.2)
            yield api
        finally:
            command="$p=Get-Process -Id "+str(pid)+" -ErrorAction SilentlyContinue;if($p){if($p.Path -ne $env:EXPECTED_EXE){throw 'Ownership mismatch'};Stop-Process -Id $p.Id -Force;$p.WaitForExit()}"
            subprocess.run(['powershell','-NoProfile','-Command',command],env=dict(os.environ,EXPECTED_EXE=str(root/'opencode.exe')),check=True,capture_output=True,timeout=15)


def native_call(api, label):
    record={'path':label,'at':now(),'model':MODEL,'errors':[],'text':''}
    session=api.call('POST','/api/session',{'model':{'id':MODEL,'providerID':'opencode'},'permissions':[{'action':'*','resource':'*','effect':'ask'}]})['data']['id']
    feed=EventFeed(api);start=time.monotonic()
    try:
        api.call('POST',f'/api/session/{session}/prompt',{'text':PROMPT})
        deadline=time.monotonic()+40
        while time.monotonic()<deadline:
            e=feed.next()
            if not e or e.get('data',{}).get('sessionID')!=session:continue
            data=e['data']
            if e['type']=='session.step.started':record['selected_model']=data.get('model')
            if e['type']=='session.text.delta':record['text']+=data['delta']
            if e['type']=='session.step.failed':record['errors'].append(error_summary(data.get('error',{})));break
            if e['type']=='session.step.ended':record['finish']=data.get('finish');break
            if e['type']=='session.tool.input.started':record['unexpected_tool']=data.get('name');break
        record['seconds']=round(time.monotonic()-start,3)
        record['passed']=record.get('finish')=='stop' and record['text'].strip()=='HELLO' and not record['errors']
    finally:
        feed.close();api.call('POST',f'/api/session/{session}/interrupt',{});api.call('DELETE',f'/api/session/{session}')
    return record


def bridge_call(api,root):
    record={'path':'candidate_bridge','at':now(),'model':MODEL}
    cfg=json.loads((root/'binding.json').read_text());key=json.loads(protect((root/'secrets.dpapi').read_bytes(),True))['bridge']
    feed=EventFeed(api)
    try:
        result,timing=invoke(f"http://127.0.0.1:{cfg['port']}/v1",key,{'model':MODEL,'messages':[{'role':'user','content':PROMPT}]})
        record.update(result=result,timing=timing,passed=result['finish_reason']=='stop')
    except Exception as error:
        record.update(passed=False,error=json.loads(error.read()) if hasattr(error,'read') else type(error).__name__)
    finally:
        record['upstream_errors']=[]
        while not feed.events.empty():
            e=feed.events.get_nowait()
            if e['type']=='session.step.failed':record['upstream_errors'].append(error_summary(e.get('data',{}).get('error',{})))
        feed.close()
    record['remaining_sessions']=len(api.call('GET','/api/session')['data'])
    return record


def main():
    if os.environ.get('LOADOUT_LIVE_TESTS')!='1':raise RuntimeError('Live opt-in required')
    report={'binary_sha256':hashlib.sha256((DEPLOYED/'opencode.exe').read_bytes()).hexdigest(),'version':'2.0.32','prompt':PROMPT,'cases':[]}
    def save(record):
        report['cases'].append(record)
        Path('docs/research/opencode-403-control-20261018.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
        print(json.dumps(record),flush=True)
    save(official_cli())
    with native_server() as api:save(native_call(api,'official_serve_no_plugin'))
    with isolated_server() as (api,root):save(bridge_call(api,root))
    save(official_cli())
    save(official_cli(coding=True))
    with isolated_server() as (api,root):save(bridge_call(api,root))


if __name__=='__main__':main()
