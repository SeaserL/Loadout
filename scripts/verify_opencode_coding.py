# SPDX-License-Identifier: Apache-2.0
"""Opt-in real OpenCode coding client through an isolated Loadout gateway."""
import json
import hashlib
import argparse
import uuid
import os
from pathlib import Path
import subprocess
import shutil
import tempfile
import threading
import time
import urllib.request
import urllib.error
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from opencode_probe_support import isolated_server
from verify_opencode_agent import isolated_gateway
from workbuddy_bridge_key import protect


def coding_client(base, key, exact, report):
    # Metadata-only observation. Request bodies and credentials stay in memory.
    class Relay(BaseHTTPRequestHandler):
        def log_message(self, *_): pass
        def do_POST(self):
            raw = self.rfile.read(int(self.headers['Content-Length']))
            body = json.loads(raw)
            start = time.monotonic()
            record = {'bytes': len(raw), 'fields': sorted(body),
                      'roles': [m['role'] for m in body.get('messages', [])],
                      'message_fields': [sorted(m) for m in body.get('messages', [])],
                      'content_types': sorted({type(m.get('content')).__name__ for m in body.get('messages', [])}),
                      'tools': [t['function']['name'] for t in body.get('tools', [])],
                      'max_tokens': body.get('max_tokens'), 'max_completion_tokens':body.get('max_completion_tokens'), 'tool_choice': body.get('tool_choice')}
            report['requests'].append(record)
            request = urllib.request.Request(base + '/chat/completions', raw,
                      headers={'Authorization': 'Bearer ' + key, 'Content-Type': 'application/json'})
            try: response = urllib.request.urlopen(request, timeout=65)
            except urllib.error.HTTPError as error: response = error
            with response:
                record['status'] = response.status
                record['route_verified'] = response.headers.get('x-loadout-route') == exact
                self.send_response(response.status)
                self.send_header('Content-Type', response.headers.get('Content-Type', 'application/json'))
                self.send_header('Connection', 'close'); self.end_headers()
                if response.status != 200:
                    payload = response.read()
                    try: record['error'] = json.loads(payload).get('error')
                    except ValueError: record['error'] = 'non_json_error'
                    self.wfile.write(payload)
                else:
                    for line in response:
                        if line.startswith(b'data:') and line[5:].strip() != b'[DONE]':
                            value = json.loads(line[5:])
                            if value.get('error'): record['stream_error'] = value['error']['type']
                            for choice in value.get('choices', []):
                                if choice.get('delta', {}).get('content'):
                                    record.setdefault('first_text_seconds', round(time.monotonic()-start,3))
                                    record['text_chunks'] = record.get('text_chunks',0)+1
                                if choice.get('finish_reason'): record['finish'] = choice['finish_reason']
                        self.wfile.write(line); self.wfile.flush()
                    record['seconds'] = round(time.monotonic()-start,3)
                self.close_connection = True
    server = ThreadingHTTPServer(('127.0.0.1', 0), Relay)
    thread = threading.Thread(target=server.serve_forever, daemon=True); thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix='loadout-coding-client-') as temp:
            root = Path(temp); work = root / 'workspace'; work.mkdir()
            (work / 'calculator.py').write_text('def total(values):\n    raise NotImplementedError()\n', encoding='utf-8')
            (work / 'test_calculator.py').write_text('import unittest\nfrom calculator import total\nclass Tests(unittest.TestCase):\n    def test_total(self):\n        self.assertEqual(total([1, 2, 3]), 6)\n        self.assertEqual(total([]), 0)\n        self.assertEqual(total([-4, 2]), -2)\nif __name__ == "__main__": unittest.main()\n', encoding='utf-8')
            test_hash = hashlib.sha256((work/'test_calculator.py').read_bytes()).hexdigest()
            env = dict(os.environ)
            for name in list(env):
                if name.startswith(('OPENCODE_', 'XDG_', 'ANTHROPIC_', 'OPENAI_')): env.pop(name)
            for name, folder in [('XDG_DATA_HOME','data'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_STATE_HOME','state'),('OPENCODE_TEST_HOME','home'),('OPENCODE_CONFIG_DIR','config/opencode')]:
                env[name] = str(root / folder)
            config = {'model': 'loadout/coding', 'providers': {'loadout': {
                'env': ['LOADOUT_CODING_KEY'], 'package': '@opencode/ai/providers/openai-compatible',
                'settings': {'baseURL': f'http://127.0.0.1:{server.server_port}/v1'},
                'models': {'coding': {'modelID': exact, 'capabilities': {'tools': True, 'input': ['text'], 'output': ['text']}, 'limit': {'context': 64000, 'output': 4096}}}}},
                'permissions': [{'action':'*','resource':'*','effect':'allow'}]}
            plugin_dir = root / 'client-plugin'
            shutil.copytree(Path(__file__).parent / 'opencode_loadout_client',plugin_dir)
            config['plugins'] = [{'package':str(plugin_dir),'options':{'noRetry':True}}]
            env.update(LOADOUT_CODING_KEY=key, OPENCODE_CONFIG_CONTENT=json.dumps(config), OPENCODE_DB=':memory:',
                       OPENCODE_DISABLE_PROJECT_CONFIG='1', OPENCODE_DISABLE_CLAUDE_CODE='1', OPENCODE_DISABLE_AUTOUPDATE='1')
            binary = Path(os.environ['LOCALAPPDATA']) / 'LoadoutOpenCodeBridge/opencode.exe'
            prompt = 'Work only inside the current directory. First read calculator.py and test_calculator.py. Run the existing tests and observe the failure. Then implement total(values) correctly. Run python -m unittest -v again. Do not modify the tests. Finish only after tests pass. Use tools for each step; do not merely describe changes.'
            start = time.monotonic()
            run = subprocess.run([str(binary),'run','--standalone','--model','loadout/coding','--format','json',prompt],
                                 cwd=work, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=150)
            report['client_exit'] = run.returncode
            report['seconds'] = round(time.monotonic()-start, 3)
            # Only synthetic task output; redact the temporary credential defensively.
            report['client_output'] = (run.stdout + run.stderr).replace(key, '[REDACTED]')[-24000:]
            verify = subprocess.run(['python','-m','unittest','-v'], cwd=work, capture_output=True, text=True)
            report['independent_test_exit'] = verify.returncode
            report['result_source'] = (work / 'calculator.py').read_text(encoding='utf-8')
            report['tests_unchanged'] = hashlib.sha256((work/'test_calculator.py').read_bytes()).hexdigest() == test_hash
            events = [json.loads(line) for line in run.stdout.splitlines() if line.startswith('{')]
            calls = [e['part'] for e in events if e.get('type') == 'tool_use']
            report['tool_calls'] = [{'tool':c['tool'],'status':c['state']['status'],'exit':c['state'].get('metadata',{}).get('metadata',{}).get('exit')} for c in calls]
            report['tool_rounds'] = sum(e.get('type')=='step_finish' and e.get('part',{}).get('reason')=='tool-calls' for e in events)
            hidden = subprocess.run(['python','-c','from calculator import total; assert total(iter([1,2,3]))==6; assert total([0.5,-0.25])==0.25; assert total([10**30,-10**30,7])==7'],cwd=work,capture_output=True)
            report['hidden_checks_passed'] = hidden.returncode == 0
            report['passed'] = all((run.returncode==0,verify.returncode==0,report['tests_unchanged'],report['hidden_checks_passed'],report['tool_rounds']>=3,any(c['tool']=='shell' and c['exit']==0 for c in report['tool_calls']),any(c['tool']=='shell' and c['exit'] not in (None,0) for c in report['tool_calls'])))
    finally:
        server.shutdown(); server.server_close(); thread.join()


def main():
    if os.environ.get('LOADOUT_LIVE_TESTS') != '1': raise RuntimeError('Live opt-in required')
    parser=argparse.ArgumentParser();parser.add_argument('--production',action='store_true');args=parser.parse_args()
    report = {'requests': [], 'scope': 'production-loadout-real-opencode' if args.production else 'isolated-loadout-real-opencode'}
    try:
        if args.production:
            cli=str(Path('target/v3/release/loadout.exe').resolve())
            def command(*words):
                return json.loads(subprocess.run([cli,'--json',*words],capture_output=True,text=True,encoding='utf-8',check=True).stdout)
            name='coding-verify-'+uuid.uuid4().hex[:8]
            key=command('client','create',name)['key']
            try:
                exact='opencode-free/main/deepseek-v4.1-flash-free'
                command('client','policy',name,'--allow',exact)
                coding_client('http://'+command('daemon','status')['gateway']['addr']+'/v1',key,exact,report)
            finally:command('client','revoke',name)
        else:
            with isolated_server() as (api, root):
                config = json.loads((root / 'binding.json').read_text())
                key = json.loads(protect((root / 'secrets.dpapi').read_bytes(), True))['bridge']
                with isolated_gateway('target/v3/release/loadout.exe', f"http://127.0.0.1:{config['port']}/v1", key) as target:
                    coding_client(*target, report)
                report['remaining_sessions'] = len(api.call('GET','/api/session')['data'])
    finally:
        output = Path('docs/research/opencode-coding-agent-'+('production-' if args.production else '')+'20261018.json')
        output.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
        print(json.dumps({k:v for k,v in report.items() if k!='client_output'}, ensure_ascii=True), flush=True)
    if not report.get('passed'):raise RuntimeError('Coding client acceptance failed; see receipt')


if __name__ == '__main__': main()
