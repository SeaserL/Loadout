# SPDX-License-Identifier: Apache-2.0
"""Explicitly opted-in isolated/installed Loadout end-to-end verification."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import urllib.request
import uuid
from workbuddy_bridge_key import protect


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--loadout', required=True)
    p.add_argument('--bridge-dir', required=True)
    p.add_argument('--production', action='store_true')
    p.add_argument('--model', default='big-pickle', choices=['big-pickle', 'deepseek-v4.1-flash-free'])
    p.add_argument('--history', action='store_true')
    args = p.parse_args()
    if os.environ.get('LOADOUT_LIVE_TESTS') != '1': raise RuntimeError('Live opt-in required')
    unique = uuid.uuid4().hex
    prefix = [str(Path(args.loadout).resolve()), '--json']
    if not args.production: prefix += ['--pipe-name', '\\\\.\\pipe\\loadout-opencode-test-' + unique]
    def cli(*words, input_text=None):
        r = subprocess.run(prefix + list(words), input=input_text, capture_output=True, text=True,
                           encoding='utf-8', timeout=60)
        if r.returncode: raise RuntimeError('CLI failed: ' + ' '.join(words[:2]))
        return json.loads(r.stdout) if r.stdout.strip() else None
    name = 'opencode-verify-' + unique[:8]
    client = account = started = False
    exact = 'opencode-free/main/' + args.model
    with tempfile.TemporaryDirectory(prefix='loadout-opencode-test-') as temp:
        try:
            if not args.production:
                cli('daemon','start','--data-dir',temp,'--port','0','--instance-id',unique,'--secret-prefix','loadout-e2e-'+unique+':')
                started = True
                kind = next(k for k in cli('provider','kinds') if k['kind']=='opencode-bridge')
                if not kind.get('requires_transport_opt_in'): raise RuntimeError('Panel opt-in metadata missing')
                cli('provider','add','opencode-free','--kind','opencode-bridge')
                secret = json.loads(protect((Path(args.bridge_dir)/'secrets.dpapi').read_bytes(),True))['bridge']
                cli('account','add','opencode-free','main','--api-key-stdin',input_text=secret+'\n')
                account = True
                if exact in json.dumps(cli('route','status')): raise RuntimeError('Route before opt-in')
                cli('account','transport','opencode-free','main','on','--i-understand-the-risk','--confirm','main')
            status = cli('daemon','status')
            key = cli('client','create',name)['key']
            client = True
            cli('client','policy',name,'--allow',exact)
            for streaming in (False, True):
                body = {'model':exact,'messages':[{'role':'user','content':'Reply exactly LOADOUT_OFFICIAL_API_OK. Do not use tools.'}],'stream':streaming}
                if args.history:
                    body['messages'] = [{'role':'system','content':'Answer briefly. Do not use tools.'}, {'role':'user','content':'Give me a synthetic marker.'}, {'role':'assistant','content':'LOADOUT_OFFICIAL_API_OK'}, {'role':'user','content':'Repeat your previous answer exactly.'}]
                req = urllib.request.Request('http://'+status['gateway']['addr']+'/v1/chat/completions',
                    json.dumps(body).encode(),headers={'Authorization':'Bearer '+key,'Content-Type':'application/json'})
                with urllib.request.urlopen(req,timeout=60) as res: raw=res.read().decode()
                if streaming:
                    chunks=[json.loads(line[5:]) for line in raw.splitlines() if line.startswith('data:') and line[5:].strip()!='[DONE]']
                    text=''.join(c['choices'][0]['delta'].get('content','') for c in chunks if c.get('choices'))
                    if '[DONE]' not in raw: raise RuntimeError('Missing DONE')
                else: text=json.loads(raw)['choices'][0]['message']['content']
                if text.strip()!='LOADOUT_OFFICIAL_API_OK': raise RuntimeError('Response mismatch')
                print(json.dumps({'scope':'production' if args.production else 'isolated','stream':streaming,'exact_model':exact,'marker_verified':True,'history':args.history}),flush=True)
        finally:
            if client: cli('client','revoke',name)
            if account: cli('account','remove','opencode-free','main')
            if started: cli('daemon','stop')


if __name__=='__main__':
    try: main()
    except Exception as error:
        print(json.dumps({'status':'FAILED','error':type(error).__name__,
                          'detail':str(error) if isinstance(error, RuntimeError) else 'Request failed; raw details suppressed'}))
        raise SystemExit(1)
