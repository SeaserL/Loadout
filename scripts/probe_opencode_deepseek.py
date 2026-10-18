# SPDX-License-Identifier: Apache-2.0
"""Opt-in finite exact-model and conversation durability probe."""
import json
import os
from pathlib import Path
import time
import uuid
from opencode_bridge import Official
from workbuddy_bridge_key import protect


def main():
    if os.environ.get('LOADOUT_LIVE_TESTS') != '1': raise RuntimeError('Live opt-in required')
    root = Path(os.environ['LOCALAPPDATA'])/'LoadoutOpenCodeBridge'
    keys = json.loads(protect((root/'secrets.dpapi').read_bytes(), True))
    api = Official(18790, keys['official'])
    model = 'deepseek-v4.1-flash-free'
    marker = 'REMEMBER_' + uuid.uuid4().hex[:12]
    report = {'version': api.call('GET','/api/info').get('version'), 'model': model, 'rounds': [], 'sessions_deleted': False}
    sessions = []
    output = Path('docs/research/opencode-deepseek-durability-20261018.json')
    def save(): output.write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
    def session():
        sid = api.call('POST','/api/session',{'title':'Loadout synthetic durability probe',
            'model':{'id':model,'providerID':'opencode'},
            'permissions':[{'action':'*','resource':'*','effect':'ask'}]})['data']['id']
        sessions.append(sid)
        return sid
    def turn(sid,prompt,expected,label):
        previous = {m['id'] for m in api.call('GET',f'/api/session/{sid}/message')['data']}
        start=time.monotonic()
        api.call('POST',f'/api/session/{sid}/prompt',{'text':prompt})
        result={'label':label,'expected':expected}
        while time.monotonic()-start<90:
            if api.call('GET',f'/api/session/{sid}/permission').get('data'):
                result.update(error='tool_permission_requested',passed=False)
                break
            messages=[m for m in api.call('GET',f'/api/session/{sid}/message')['data'] if m['id'] not in previous]
            failed=next((m for m in messages if m.get('error')),None)
            if failed:
                error=failed['error']
                result.update(error={k:error.get(k) for k in ('type','message','status')},passed=False)
                break
            assistant=next((m for m in messages if m.get('type')=='assistant' and m.get('finish')),None)
            if assistant:
                text=''.join(p.get('text','') for p in assistant.get('content',[]) if p.get('type')=='text')
                result.update(text=text,returned_model=assistant.get('model'),finish=assistant['finish'],tokens=assistant.get('tokens'))
                result['passed']=text.strip()==expected and assistant.get('model',{}).get('id')==model and assistant['finish']=='stop'
                break
            time.sleep(.3)
        else: result.update(error='timeout',passed=False)
        result['seconds']=round(time.monotonic()-start,3)
        report['rounds'].append(result)
        save()
        print(json.dumps(result,ensure_ascii=False),flush=True)
        if not result['passed']: api.call('POST',f'/api/session/{sid}/interrupt',{})
        return result['passed']
    try:
        a=session()
        if not turn(a,f'Remember this synthetic token for this conversation: {marker}. Reply exactly ACK. Do not use tools.','ACK','initial'): return
        if not turn(a,'What synthetic token did I ask you to remember? Reply with only the token. Do not use tools.',marker,'same_session_recall'): return
        turn(a,'只回复：连续调用正常。不要使用工具。','连续调用正常','chinese')
        turn(a,'What is 173 + 284? Reply only with the integer. Do not use tools.','457','arithmetic')
        print('Idle check: waiting 30 seconds',flush=True)
        time.sleep(30)
        turn(a,'Repeat the synthetic token from the first message, and nothing else. Do not use tools.',marker,'idle_recall')
        b=session()
        turn(b,'If this conversation has no earlier user messages, reply exactly FRESH. Otherwise reply OLD. Do not use tools.','FRESH','independent_session')
    finally:
        for sid in sessions:
            api.call('POST',f'/api/session/{sid}/interrupt',{})
            api.call('DELETE',f'/api/session/{sid}')
        report['sessions_deleted']=True
        save()


if __name__=='__main__': main()
