# SPDX-License-Identifier: Apache-2.0
"""Finite, isolated live tests of actual upstream output and tool selection."""
import json
import os
from pathlib import Path
from opencode_probe_support import isolated_server
from verify_opencode_agent import invoke, TOOL
from workbuddy_bridge_key import protect


def verify(base, key, model):
    report = []
    for field in ('max_tokens', 'max_completion_tokens'):
        for stream in (False, True):
            body = {'model': model, 'messages': [{'role':'user','content':'Write a detailed 2000 word essay about rain. Do not use tools.'}], field: 16, 'stream':stream}
            result, timing = invoke(base, key, body)
            item = {'parameter':field,'stream':stream,'finish':result['finish_reason'],'characters':len(result['message']['content'] or ''),'timing':timing}
            report.append(item); print(json.dumps(item),flush=True)
            assert result['finish_reason'] == 'length'
    for choice in ('none', 'required', {'type':'function','function':{'name':'lookup_code'}}):
        body = {'model':model, 'messages':[{'role':'user','content':'Say hello. Use lookup_code for Sydney only if tool selection requires a tool.'}],
                'tools':[TOOL], 'tool_choice':choice, 'max_tokens':512, 'stream':True}
        try: result,timing = invoke(base,key,body)
        except Exception as error:
            item={'tool_choice':choice,'error':error.read().decode() if hasattr(error,'read') else str(error)}
            report.append(item); print(json.dumps(item),flush=True)
            continue
        calls = result['message'].get('tool_calls',[])
        assert (not calls and result['finish_reason']=='stop') if choice=='none' else (len(calls)>0 and all(c['function']['name']=='lookup_code' for c in calls))
        item={'tool_choice':choice,'finish':result['finish_reason'],'calls':len(calls),'timing':timing}
        report.append(item);print(json.dumps(item),flush=True)
    return report


if __name__ == '__main__':
    if os.environ.get('LOADOUT_LIVE_TESTS') != '1': raise RuntimeError('Live opt-in required')
    with isolated_server() as (api, root):
        config=json.loads((root/'binding.json').read_text())
        key=json.loads(protect((root/'secrets.dpapi').read_bytes(),True))['bridge']
        report=verify(f"http://127.0.0.1:{config['port']}/v1",key,'deepseek-v4.1-flash-free')
        Path('docs/research/opencode-controls-20261018.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
