# SPDX-License-Identifier: Apache-2.0
"""Finite exact-model capability gates and real coding acceptance on hotspot."""
import json
import os
from pathlib import Path
import uuid
from verify_opencode_agent import invoke, TOOL, isolated_gateway
from verify_opencode_coding import coding_client
from opencode_probe_support import isolated_server
from workbuddy_bridge_key import protect


def verify_capabilities(base, key, model, resume_checks=None):
    report = {'model': model, 'checks': [], 'passed': False}
    def run(name, body, check):
        if resume_checks:
            prior = next((c for c in resume_checks if c['name']==name and c['passed']), None)
            if prior:
                report['checks'].append(dict(prior, reused_from_prior_run=True))
                return None
        body['model'] = model
        result, timing = invoke(base, key, body)
        item = {'name': name, 'timing': timing, 'finish': result['finish_reason'], 'passed': bool(check(result, timing))}
        report['checks'].append(item)
        print(json.dumps(item), flush=True)
        if not item['passed']: raise RuntimeError('Capability assertion: '+name)
        return result
    try:
        run('realtime', {'messages':[{'role':'user','content':'Write 30 numbered short sentences about rain. Do not use tools.'}], 'stream':True},
            lambda r,t: r['finish_reason']=='stop' and t['model']==model.rsplit('/',1)[-1] and t['text_chunks']>2 and t['last_text']-t['first_text']>.3)
        for stream in (False, True):
            run('max_tokens_16_'+str(stream), {'messages':[{'role':'user','content':'Write a long detailed essay about rain. Do not use tools.'}], 'max_tokens':16,'stream':stream},
                lambda r,t: r['finish_reason']=='length')
        for choice in ('none','required',{'type':'function','function':{'name':'lookup_code'}}):
            def check(r,t):
                calls=r['message'].get('tool_calls',[])
                if choice=='none': return not calls and r['finish_reason']=='stop'
                return r['finish_reason']=='tool_calls' and len(calls)==1 and calls[0]['function']['name']=='lookup_code' and json.loads(calls[0]['function']['arguments'])=={'city':'Sydney'}
            run('tool_choice_'+json.dumps(choice), {'messages':[{'role':'user','content':'Say hello. If a tool is required, call lookup_code for Sydney exactly once.'}],
                'tools':[TOOL], 'tool_choice':choice, 'max_tokens':512, 'stream':True},check)
        messages=[]
        codes=[]
        for city in ('Sydney','Melbourne','Perth'):
            messages.append({'role':'user','content':'Call lookup_code for '+city+' exactly once.'})
            def check(r,t):
                calls=r['message'].get('tool_calls',[])
                return r['finish_reason']=='tool_calls' and len(calls)==1 and calls[0]['function']['name']=='lookup_code' and json.loads(calls[0]['function']['arguments'])=={'city':city}
            result=run('continuous_tool_'+city, {'messages':messages,'tools':[TOOL], 'tool_choice':{'type':'function','function':{'name':'lookup_code'}},'max_tokens':512,'stream':True},check)
            call=result['message']['tool_calls'][0]
            code='CODE_'+uuid.uuid4().hex[:10];codes.append(code)
            messages.extend([result['message'],{'role':'tool','tool_call_id':call['id'],'content':code}])
        messages.append({'role':'user','content':'Reply only with the three codes returned by lookup_code, in chronological order, separated by a single space.'})
        run('continuous_tool_final', {'messages':messages,'tools':[TOOL],'tool_choice':'none','max_tokens':1024,'stream':True},
            lambda r,t: r['finish_reason']=='stop' and r['message']['content'].strip()==' '.join(codes))
        report['passed']=True
        report['coding']={'requests':[]}
        coding_client(base,key,model,report['coding'])
        report['coding_route_header_required']='/' in model
        if '/' in model:
            report['coding']['route_checks_passed']=all(r.get('route_verified') for r in report['coding']['requests'])
        print(json.dumps({'stage':'coding','passed':report['coding'].get('passed'),'tool_rounds':report['coding'].get('tool_rounds')}),flush=True)
    except Exception as error:
        report['error']=error.read().decode() if hasattr(error,'read') else type(error).__name__+': '+str(error)
        print(json.dumps({'stage':'failed','error':report['error']}),flush=True)
    return report


def main():
    if os.environ.get('LOADOUT_LIVE_TESTS')!='1':raise RuntimeError('Live opt-in required')
    report={'scope':'hotspot-isolated-loadout','production_changed':False}
    try:
        with isolated_server() as (api,root):
            cfg=json.loads((root/'binding.json').read_text())
            key=json.loads(protect((root/'secrets.dpapi').read_bytes(),True))['bridge']
            with isolated_gateway('target/v3/release/loadout.exe',f"http://127.0.0.1:{cfg['port']}/v1",key) as target:
                prior_path=Path('docs/research/opencode-loadout-hotspot-initial-failure-20261018.json')
                prior=json.loads(prior_path.read_text(encoding='utf-8'))['capabilities']['checks'] if os.environ.get('LOADOUT_RESUME_HOTSPOT')=='1' else None
                report['capabilities']=verify_capabilities(*target,resume_checks=prior)
            report['remaining_sessions']=len(api.call('GET','/api/session')['data'])
            report['mcp_servers']=api.call('GET','/api/mcp')['data']
            report['database_files']=len(list((root/'data').rglob('*.db')))
    finally:
        Path('docs/research/opencode-loadout-hotspot-20261018.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')


if __name__=='__main__':main()
