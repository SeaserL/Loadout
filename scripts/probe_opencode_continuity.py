# SPDX-License-Identifier: Apache-2.0
"""Opt-in finite six-minute bridge continuity probe, with synthetic history only."""
import json
import os
from pathlib import Path
import time
import urllib.request
import uuid
from workbuddy_bridge_key import protect


def main():
    if os.environ.get('LOADOUT_LIVE_TESTS') != '1': raise RuntimeError('Live opt-in required')
    root=Path(os.environ['LOCALAPPDATA'])/'LoadoutOpenCodeBridge'
    key=json.loads(protect((root/'secrets.dpapi').read_bytes(),True))['bridge']
    model='deepseek-v4.1-flash-free'
    token='CONTINUITY_'+uuid.uuid4().hex[:12]
    history=[{'role':'system','content':'Follow the user and answer briefly. Do not use tools.'},
             {'role':'user','content':f'Remember {token}. Reply exactly ACK.'}]
    report={'model':model,'rounds':[],'interval_seconds':30}
    output=Path(__file__).resolve().parent.parent/'docs/research/opencode-deepseek-continuity-20261018.json'
    start=time.monotonic()
    for index in range(12):
        if index: time.sleep(30)
        expected='ACK' if index==0 else token
        body={'model':model,'messages':history}
        req=urllib.request.Request('http://127.0.0.1:18789/v1/chat/completions',json.dumps(body).encode(),headers={'Authorization':'Bearer '+key,'Content-Type':'application/json'})
        began=time.monotonic()
        row={'round':index+1}
        try:
            with urllib.request.urlopen(req,timeout=60) as response: result=json.load(response)
            text=result['choices'][0]['message']['content']
            row.update(returned_model=result['model'],passed=result['model']==model and text.strip()==expected,
                       seconds=round(time.monotonic()-began,3),message_count=len(history))
            if row['passed']:
                history.extend([{'role':'assistant','content':text},{'role':'user','content':'Repeat the token from the first user message exactly. Do not use tools.'}])
        except Exception as error:
            row.update(passed=False,error_type=type(error).__name__,seconds=round(time.monotonic()-began,3))
        report['rounds'].append(row)
        report['elapsed_seconds']=round(time.monotonic()-start,3)
        output.write_text(json.dumps(report,indent=2),encoding='utf-8')
        print(json.dumps(row),flush=True)
    report['passed']=all(row['passed'] for row in report['rounds'])
    output.write_text(json.dumps(report,indent=2),encoding='utf-8')
    if not report['passed']: raise SystemExit(1)


if __name__=='__main__': main()
