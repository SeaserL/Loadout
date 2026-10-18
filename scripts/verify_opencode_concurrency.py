# SPDX-License-Identifier: Apache-2.0
"""Opt-in concurrent real tools and coding clients through isolated Loadout."""
from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import threading
import time
import uuid
import urllib.request
import socket
from opencode_probe_support import isolated_server
from opencode_bridge import Official
from opencode_events import EventFeed
from probe_opencode_403 import error_summary
from workbuddy_bridge_key import protect
from verify_opencode_agent import isolated_gateway, invoke
from verify_opencode_coding import coding_client


def verify(base,key,model,apis,report):
    report.update(tool_flows=[],coding=[],stage='consecutive-users')
    result,_=invoke(base,key,{'model':model,'messages':[{'role':'user','content':'Remember the first code: FIRST_93.'},
        {'role':'user','content':'Remember the second code: SECOND_47.'},
        {'role':'user','content':'Reply only with both codes in order separated by one space. Do not use tools.'}], 'stream':True})
    assert result['message']['content'].strip()=='FIRST_93 SECOND_47'
    report['consecutive_users_passed']=True
    print(json.dumps({'stage':'consecutive-users','passed':True}),flush=True)
    report['stage']='concurrent-tools'
    barrier=threading.Barrier(2)
    def flow(index):
        field='left_value' if index==0 else 'right_value'
        value='INPUT_'+uuid.uuid4().hex[:8];answer='RESULT_'+uuid.uuid4().hex[:8]
        tool={'type':'function','function':{'name':'probe_value','description':'Return a code for this exact input.',
             'parameters':{'type':'object','properties':{field:{'type':'string'}},'required':[field],'additionalProperties':False}}}
        messages=[{'role':'user','content':'Call probe_value once using '+field+'='+value+'. After its result reply only with the result code.'}]
        body={'model':model,'messages':messages,'tools':[tool],'stream':True,'max_tokens':512,
              'tool_choice':{'type':'function','function':{'name':'probe_value'}}}
        barrier.wait();start=time.monotonic()
        result,timing=invoke(base,key,body)
        calls=result['message'].get('tool_calls',[])
        assert result['finish_reason']=='tool_calls' and len(calls)==1
        assert calls[0]['function']['name']=='probe_value' and json.loads(calls[0]['function']['arguments'])=={field:value}
        messages.extend([result['message'],{'role':'tool','tool_call_id':calls[0]['id'],'content':answer},
                         {'role':'user','content':'Use the returned result.'},{'role':'user','content':'Reply only with that result code.'}])
        body['tool_choice']='none'
        final,_=invoke(base,key,body)
        assert final['message']['content'].strip()==answer
        return {'index':index,'start':start,'end':time.monotonic(),'same_name_distinct_schema_passed':True,'tool_result_isolated':True,'timing':timing}
    with ThreadPoolExecutor(max_workers=2) as executor:
        futures=[executor.submit(flow,i) for i in range(2)]
        both_active=False
        while not all(f.done() for f in futures):
            active=[len(api.call('GET','/api/session')['data']) for api in apis]
            both_active |= all(n>0 for n in active)
            time.sleep(.1)
        report['tool_flows']=[f.result() for f in futures]
    assert both_active
    report['two_native_workers_active_observed']=both_active
    print(json.dumps({'stage':'concurrent-tools','passed':True}),flush=True)
    report['stage']='queue'
    def queued(index):
        marker='QUEUE_'+str(index)
        result,_=invoke(base,key,{'model':model,'messages':[{'role':'user','content':'Reply only '+marker+'. Do not use tools.'}],'stream':True})
        assert result['message']['content'].strip()==marker
        return True
    with ThreadPoolExecutor(max_workers=3) as executor:
        report['three_requests_with_queue']=all(executor.map(queued,range(3)))
    report['stage']='cancellation'
    def cancel_stream():
        request=urllib.request.Request(base+'/chat/completions',json.dumps({'model':model,'messages':[{'role':'user','content':'Write 300 numbered sentences about rain. Do not use tools.'}],'stream':True}).encode(),headers={'Authorization':'Bearer '+key,'Content-Type':'application/json'})
        with urllib.request.urlopen(request,timeout=65) as response:
            for line in response:
                if b'"content"' in line:
                    response.fp.raw._sock.shutdown(socket.SHUT_RDWR)
                    return True
        raise RuntimeError('No cancellation boundary')
    with ThreadPoolExecutor(max_workers=2) as executor:
        cancelled=executor.submit(cancel_stream)
        survivor=executor.submit(queued,9)
        report['cancel_does_not_break_other']=cancelled.result() and survivor.result()
    report['stage']='coding'
    def code(index):
        result={'requests':[],'index':index};start=time.monotonic()
        coding_client(base,key,model,result)
        result['interval']=[start,time.monotonic()]
        assert result['passed'] and all(r['route_verified'] for r in result['requests'])
        return result
    with ThreadPoolExecutor(max_workers=2) as executor:
        report['coding']=list(executor.map(code,range(2)))
    a,b=[r['interval'] for r in report['coding']]
    assert min(a[1],b[1])>max(a[0],b[0])
    report['coding_overlapped']=True
    report['passed']=True
    return report


def main():
    if os.environ.get('LOADOUT_LIVE_TESTS')!='1':raise RuntimeError('Live opt-in required')
    report={'scope':'isolated-loadout','production_changed':False}
    try:
        with isolated_server() as (api,root):
            cfg=json.loads((root/'binding.json').read_text())
            keys=json.loads(protect((root/'secrets.dpapi').read_bytes(),True))
            apis=[Official(p,keys['official']) for p in cfg['official_ports']]
            feeds=[EventFeed(a) for a in apis]
            errors=[];stop=threading.Event()
            def observe():
                while not stop.is_set():
                    for index,feed in enumerate(feeds):
                        while not feed.events.empty():
                            event=feed.events.get_nowait()
                            if event['type']=='session.step.failed':
                                errors.append({'worker':index,'error':error_summary(event.get('data',{}).get('error',{}))})
                    stop.wait(.05)
            observer=threading.Thread(target=observe,daemon=True);observer.start()
            try:
                with isolated_gateway(os.environ.get('LOADOUT_VERIFY_BINARY','target/release/loadout.exe'),f"http://127.0.0.1:{cfg['port']}/v1",keys['bridge']) as target:
                    report['result']={}
                    verify(*target,apis,report['result'])
            finally:
                stop.set();observer.join()
                for feed in feeds:feed.close()
                report['upstream_errors']=errors
                report['cleanup']=[{'sessions':len(a.call('GET','/api/session')['data']),'mcp':a.call('GET','/api/mcp')['data']} for a in apis]
    except Exception as error:
        report['error']=error.read().decode() if hasattr(error,'read') else type(error).__name__+': '+str(error)
        print(json.dumps({'error':report['error']}),flush=True)
    finally:
        Path('docs/research/opencode-concurrency-isolated-20261018.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
    print(json.dumps({'passed':report.get('result',{}).get('passed'),'cleanup':report.get('cleanup')}),flush=True)


if __name__=='__main__':main()
