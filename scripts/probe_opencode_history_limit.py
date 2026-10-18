# SPDX-License-Identifier: Apache-2.0
"""Opt-in maximal message-count history recall check."""
import json
import os
from pathlib import Path
import time
import urllib.request
import uuid
from workbuddy_bridge_key import protect


if __name__ == '__main__':
    if os.environ.get('LOADOUT_LIVE_TESTS') != '1': raise RuntimeError('Live opt-in required')
    root=Path(os.environ['LOCALAPPDATA'])/'LoadoutOpenCodeBridge'
    key=json.loads(protect((root/'secrets.dpapi').read_bytes(),True))['bridge']
    token='FIRST_'+uuid.uuid4().hex[:12]
    history=[{'role':'system','content':'Answer briefly. No tools.'}]
    for index in range(31):
        history.extend([{'role':'user','content':f'Entry {index}, reply with its label.'},
                        {'role':'assistant','content':token if index==0 else 'entry-'+str(index)}])
    history.append({'role':'user','content':'Repeat the exact text of your FIRST assistant reply in this conversation. Output only that label, no tools.'})
    started=time.monotonic()
    request=urllib.request.Request('http://127.0.0.1:18789/v1/chat/completions',json.dumps({'model':'deepseek-v4.1-flash-free','messages':history}).encode(),headers={'Authorization':'Bearer '+key,'Content-Type':'application/json'})
    with urllib.request.urlopen(request,timeout=60) as response: result=json.load(response)
    text=result['choices'][0]['message']['content']
    report={'messages':len(history),'text':text,'expected':token,'model':result['model'],
            'passed':text.strip()==token,'seconds':round(time.monotonic()-started,3)}
    output=Path(__file__).resolve().parent.parent/'docs/research/opencode-deepseek-history-limit-fixed-20261018.json'
    output.write_text(json.dumps(report,indent=2),encoding='utf-8')
    print(json.dumps(report))
    if not report['passed']: raise SystemExit(1)
