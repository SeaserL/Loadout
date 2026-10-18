# SPDX-License-Identifier: Apache-2.0
"""Opt-in real streaming and client-executed function roundtrip verification."""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import time
import urllib.request
import uuid
import tempfile
from contextlib import contextmanager
from opencode_probe_support import isolated_server
from workbuddy_bridge_key import protect

TOOL = {'type': 'function', 'function': {'name': 'lookup_code', 'description': 'Get the synthetic code for a city. The value is only known by calling this tool.',
        'parameters': {'type': 'object', 'properties': {'city': {'type': 'string'}}, 'required': ['city'], 'additionalProperties': False}}}


@contextmanager
def isolated_gateway(binary, bridge_base, bridge_key):
    unique = uuid.uuid4().hex
    prefix = [str(Path(binary).resolve()), '--json', '--pipe-name', '\\\\.\\pipe\\loadout-agent-' + unique]
    def cli(*words, secret=None):
        run = subprocess.run(prefix + list(words), input=secret, capture_output=True, text=True, encoding='utf-8', timeout=60)
        if run.returncode: raise RuntimeError('Isolated CLI failed: ' + ' '.join(words[:2]))
        return json.loads(run.stdout) if run.stdout.strip() else None
    started = account = client = False
    with tempfile.TemporaryDirectory(prefix='loadout-agent-gateway-') as temp:
        try:
            cli('daemon','start','--data-dir',temp,'--port','0','--instance-id',unique,'--secret-prefix','loadout-e2e-'+unique+':')
            started = True
            cli('provider','add','opencode-free','--kind','opencode-bridge','--base-url',bridge_base)
            cli('account','add','opencode-free','main','--api-key-stdin',secret=bridge_key+'\n')
            account = True
            cli('account','transport','opencode-free','main','on','--i-understand-the-risk','--confirm','main')
            key = cli('client','create','agent-verify')['key']; client = True
            exact = 'opencode-free/main/deepseek-v4.1-flash-free'
            cli('client','policy','agent-verify','--allow',exact)
            yield 'http://' + cli('daemon','status')['gateway']['addr'] + '/v1', key, exact
        finally:
            if client: cli('client','revoke','agent-verify')
            if account: cli('account','remove','opencode-free','main')
            if started:
                cli('daemon','stop')
                # IPC acknowledges shutdown before SQLite handles have necessarily closed.
                time.sleep(5)


def invoke(base, key, body):
    start = time.monotonic()
    request = urllib.request.Request(base + '/chat/completions', json.dumps(body).encode(),
              headers={'Authorization': 'Bearer ' + key, 'Content-Type': 'application/json'})
    with urllib.request.urlopen(request, timeout=65) as response:
        if '/' in body['model']:
            assert response.headers.get('x-loadout-route') == body['model']
        if not body.get('stream'):
            result = json.load(response)
            return result['choices'][0], {'total': round(time.monotonic()-start, 3), 'model': result['model']}
        text, calls, times, finish, done, model = '', {}, [], None, False, None
        for line in response:
            if not line.startswith(b'data:'): continue
            raw = line[5:].strip()
            if raw == b'[DONE]': done = True; break
            value = json.loads(raw)
            if value.get('error'): raise RuntimeError('SSE error: ' + value['error'].get('type', 'unknown'))
            model = value.get('model')
            for choice in value.get('choices', []):
                delta = choice.get('delta', {})
                if delta.get('content'):
                    text += delta['content']; times.append(round(time.monotonic()-start, 3))
                for part in delta.get('tool_calls', []):
                    target = calls.setdefault(part['index'], {'id': '', 'type': 'function', 'function': {'name': '', 'arguments': ''}})
                    if part.get('id'): target['id'] = part['id']
                    for field in ('name', 'arguments'):
                        if part.get('function', {}).get(field): target['function'][field] += part['function'][field]
                if choice.get('finish_reason'): finish = choice['finish_reason']
        if not done or not finish: raise RuntimeError('Incomplete stream')
        message = {'role': 'assistant', 'content': text or None}
        if calls: message['tool_calls'] = [calls[i] for i in sorted(calls)]
        return {'message': message, 'finish_reason': finish}, {'model': model, 'total': round(time.monotonic()-start, 3),
                'text_chunks': len(times), 'first_text': times[0] if times else None, 'last_text': times[-1] if times else None}


def verify(base, key, model):
    report = {'model': model, 'roundtrips': []}
    expected_model = model.rsplit('/', 1)[-1]
    result, timing = invoke(base, key, {'model': model, 'messages': [{'role': 'user', 'content': 'Write 40 numbered short sentences about rain. Start immediately. Do not use tools.'}], 'stream': True})
    if result['finish_reason'] != 'stop' or timing['model'] != expected_model:
        raise RuntimeError('Realtime boundary mismatch: ' + json.dumps({'finish':result['finish_reason'],'returned_model':timing['model'],'requested_model':model,'timing':timing}))
    assert timing['text_chunks'] > 2 and timing['total'] - timing['first_text'] > .3
    report['realtime'] = timing
    print(json.dumps({'stage':'realtime','timing':timing}),flush=True)
    for stream in (False, True):
        messages = [{'role': 'user', 'content': 'Call lookup_code for city Sydney exactly once, then reply only with the code returned by the tool. Do not guess it or use other tools.'}]
        body = {'model': model, 'messages': messages, 'tools': [TOOL], 'tool_choice': 'auto', 'parallel_tool_calls': False, 'stream': stream}
        result, timing = invoke(base, key, body)
        assert result['finish_reason'] == 'tool_calls' and timing['model'] == expected_model
        call = result['message']['tool_calls'][0]
        assert len(result['message']['tool_calls']) == 1 and call['function']['name'] == 'lookup_code'
        assert json.loads(call['function']['arguments']) == {'city': 'Sydney'}
        token = 'CLIENT_RESULT_' + uuid.uuid4().hex[:12]
        messages += [result['message'], {'role': 'tool', 'tool_call_id': call['id'], 'content': token}]
        result, final_timing = invoke(base, key, body)
        if result['finish_reason'] != 'stop' or result['message']['content'].strip() != token:
            failure={'stage':'tool_result_continuation','stream':stream,'result':result,'expected':token,'timing':final_timing}
            Path('docs/research/opencode-agent-continuation-failure-20261018.json').write_text(json.dumps(failure,indent=2),encoding='utf-8')
            raise RuntimeError('Synthetic continuation mismatch: '+json.dumps(failure))
        assert final_timing['model'] == expected_model
        report['roundtrips'].append({'stream': stream, 'call_id_preserved': True, 'arguments_verified': True,
                    'external_result_verified': True, 'call': timing, 'continuation': final_timing})
        print(json.dumps({'stage':'tool_roundtrip','stream':stream,'passed':True}),flush=True)
    messages = [{'role':'user','content':'Call lookup_code twice in the same response, once for Sydney and once for Melbourne. After both results, output Sydney code then Melbourne code separated by one space. Do not use other tools.'}]
    body = {'model':model,'messages':messages,'tools':[TOOL],'stream':True}
    result, _ = invoke(base,key,body)
    calls = result['message'].get('tool_calls',[])
    assert result['finish_reason']=='tool_calls' and len(calls)==2
    assert {json.loads(c['function']['arguments'])['city'] for c in calls}=={'Sydney','Melbourne'}
    codes = {city:'CODE_'+uuid.uuid4().hex[:10] for city in ('Sydney','Melbourne')}
    messages.append(result['message'])
    for call in reversed(calls):
        city=json.loads(call['function']['arguments'])['city']
        messages.append({'role':'tool','tool_call_id':call['id'],'content':codes[city]})
    result, _ = invoke(base,key,body)
    assert result['finish_reason']=='stop' and result['message']['content'].strip()==codes['Sydney']+' '+codes['Melbourne']
    report['parallel_reverse_result_order_verified']=True
    return report


def main():
    if os.environ.get('LOADOUT_LIVE_TESTS') != '1': raise RuntimeError('Live opt-in required')
    parser = argparse.ArgumentParser()
    parser.add_argument('--production', action='store_true')
    parser.add_argument('--isolated-loadout')
    args = parser.parse_args()
    if args.production:
        cli = str(Path('target/v3/release/loadout.exe').resolve())
        def command(*words):
            return json.loads(subprocess.run([cli, '--json', *words], capture_output=True, text=True, encoding='utf-8', check=True).stdout)
        name = 'opencode-agent-verify-' + uuid.uuid4().hex[:8]
        key = command('client', 'create', name)['key']
        try:
            exact = 'opencode-free/main/deepseek-v4.1-flash-free'
            command('client', 'policy', name, '--allow', exact)
            status = command('daemon', 'status')
            report = verify('http://' + status['gateway']['addr'] + '/v1', key, exact)
        finally: command('client', 'revoke', name)
    else:
        with isolated_server() as (api, root):
            config = json.loads((root/'binding.json').read_text(encoding='utf-8'))
            key = json.loads(protect((root/'secrets.dpapi').read_bytes(), True))['bridge']
            base = f"http://127.0.0.1:{config['port']}/v1"
            if args.isolated_loadout:
                with isolated_gateway(args.isolated_loadout, base, key) as (gateway, client_key, exact):
                    report = verify(gateway, client_key, exact)
            else:
                report = verify(base, key, 'deepseek-v4.1-flash-free')
            # Disconnect after a real first text delta; cancellation must interrupt the native session.
            body = {'model': 'deepseek-v4.1-flash-free', 'messages': [{'role': 'user', 'content': 'Write 300 numbered sentences about rain. Do not use tools.'}], 'stream': True}
            req = urllib.request.Request(base+'/chat/completions', json.dumps(body).encode(), headers={'Authorization':'Bearer '+key})
            response = urllib.request.urlopen(req, timeout=60)
            while True:
                line = response.readline()
                if b'"content"' in line: break
                if not line: raise RuntimeError('No cancellation boundary')
            response.fp.raw._sock.shutdown(socket.SHUT_RDWR); response.close()
            deadline = time.monotonic()+8
            while time.monotonic()<deadline:
                if not api.call('GET','/api/session')['data']: break
                time.sleep(.1)
            else: raise RuntimeError('Disconnected session leaked')
            report['cancel_cleanup_passed'] = True
            report['remaining_sessions'] = len(api.call('GET','/api/session')['data'])
            report['mcp_servers'] = api.call('GET','/api/mcp')['data']
            report['database_files'] = len(list((root/'data').rglob('*.db')))
    scope = 'production' if args.production else 'isolated-loadout' if args.isolated_loadout else 'isolated'
    report['scope'] = scope
    output = Path('docs/research/opencode-agent-' + scope + '-20261018.json')
    output.write_text(json.dumps(report, indent=2), encoding='utf-8')
    print(json.dumps(report), flush=True)


if __name__ == '__main__': main()
