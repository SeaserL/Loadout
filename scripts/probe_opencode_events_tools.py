# SPDX-License-Identifier: Apache-2.0
"""Native event timing and non-executing MCP tool handoff probe."""
import json
from pathlib import Path
import queue
import secrets
import threading
import time
import urllib.request
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from opencode_probe_support import isolated_server


def main():
    report = {'model': 'deepseek-v4.1-flash-free', 'tool_executions': 0}
    key = secrets.token_urlsafe(32)
    class MCP(BaseHTTPRequestHandler):
        def log_message(self, *_): pass
        def do_GET(self):
            self.send_error(405)
        def do_POST(self):
            if self.headers.get('Authorization') != 'Bearer ' + key:
                self.send_error(401); return
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            method = body.get('method')
            if 'id' not in body:
                self.send_response(202); self.end_headers(); return
            if method == 'initialize':
                result = {'protocolVersion': '2024-11-05', 'capabilities': {'tools': {}}, 'serverInfo': {'name': 'loadout-probe', 'version': '1'}}
            elif method == 'tools/list':
                result = {'tools': [{'name': 'lookup_code', 'description': 'Return the secret synthetic validation code for a city. Must call to know its value.',
                          'inputSchema': {'type': 'object', 'properties': {'city': {'type': 'string'}}, 'required': ['city'], 'additionalProperties': False}}]}
            elif method == 'tools/call':
                report['tool_executions'] += 1
                result = {'content': [{'type': 'text', 'text': 'Probe does not execute tools'}], 'isError': True}
            else: result = {}
            raw = json.dumps({'jsonrpc': '2.0', 'id': body['id'], 'result': result}).encode()
            self.send_response(200); self.send_header('Content-Type', 'application/json'); self.send_header('Content-Length', str(len(raw))); self.end_headers(); self.wfile.write(raw)
    mcp = ThreadingHTTPServer(('127.0.0.1', 0), MCP)
    threading.Thread(target=mcp.serve_forever, daemon=True).start()
    try:
        with isolated_server() as (api, root):
            events = queue.Queue()
            response = urllib.request.urlopen(urllib.request.Request(api.base + '/api/event', headers={'Authorization': api.auth}), timeout=60)
            def reader():
                try:
                    for raw in response:
                        if raw.startswith(b'data:'):
                            data = json.loads(raw[5:])
                            if isinstance(data, str): data = json.loads(data)
                            events.put((time.monotonic(), data))
                except Exception: pass
            threading.Thread(target=reader, daemon=True).start()
            connected=events.get(timeout=10)
            report['first_event_type']=connected[1].get('type')
            def create():
                return api.call('POST','/api/session',{'model':{'id':report['model'],'providerID':'opencode'},'permissions':[{'action':'*','resource':'*','effect':'ask'}]})['data']['id']
            sid=create(); start=time.monotonic(); captured=[]
            try:
                api.call('POST',f'/api/session/{sid}/prompt',{'text':'Write 40 numbered short sentences about rain. Start immediately. Do not use tools.'})
                while time.monotonic()-start<50:
                    when,event=events.get(timeout=10)
                    if sid not in json.dumps(event): continue
                    captured.append({'at':round(when-start,3),'event':event})
                    if event.get('type') in ('session.step.ended','session.step.failed'): break
                report['stream_events']=captured
            finally:
                api.call('POST',f'/api/session/{sid}/interrupt',{});api.call('DELETE',f'/api/session/{sid}')
            api.call('PUT','/api/experimental/mcp/loadoutprobe',{'config':{'type':'remote','url':f'http://127.0.0.1:{mcp.server_port}/mcp','headers':{'Authorization':'Bearer '+key},'oauth':False,'codemode':False}})
            time.sleep(.5)
            report['mcp_status']=api.call('GET','/api/mcp')
            sid=create(); start=time.monotonic(); captured=[]
            try:
                api.call('POST',f'/api/session/{sid}/prompt',{'text':'Use the loadoutprobe lookup_code tool for city Sydney. Then reply only with the returned code. Do not guess the code and do not use other tools.'})
                while time.monotonic()-start<40:
                    when,event=events.get(timeout=10)
                    if sid not in json.dumps(event): continue
                    captured.append({'at':round(when-start,3),'event':event})
                    pending=api.call('GET',f'/api/session/{sid}/permission')['data']
                    if pending:
                        report['permission']=pending; break
                    if event.get('type')=='session.step.failed': break
                report['tool_events']=captured
                report['tool_messages']=api.call('GET',f'/api/session/{sid}/message')['data']
                if report.get('permission'):
                    info=api.call('GET',f'/api/session/{sid}')['data']
                    history=list(reversed(report['tool_messages']))
                    token='TOOL_RESULT_'+uuid.uuid4().hex[:12]
                    now=int(time.time()*1000)
                    for message in history:
                        if message['type']!='assistant': continue
                        message['time']['completed']=now
                        message['finish']='tool-calls'
                        for part in message['content']:
                            if part['type']=='tool':
                                part['state']={'status':'completed','input':part['state']['input'],'content':[{'type':'text','text':token}]}
                                part['time']['completed']=now
                    api.call('POST',f'/api/session/{sid}/interrupt',{})
                    api.call('DELETE',f'/api/session/{sid}')
                    api.call('POST','/api/experimental/session/import',{'info':info,'messages':history,'location':info['location']})
                    previous={m['id'] for m in history}
                    api.call('POST',f'/api/session/{sid}/synthetic',{'text':'','resume':True})
                    started=time.monotonic()
                    while time.monotonic()-started<30:
                        current=api.call('GET',f'/api/session/{sid}/message')['data']
                        done=next((m for m in current if m['id'] not in previous and (m.get('finish') or m.get('error'))),None)
                        if done:
                            text=''.join(p.get('text','') for p in done.get('content',[]) if p['type']=='text')
                            report['tool_roundtrip']={'text':text,'expected':token,'model':done.get('model'),'finish':done.get('finish'),'error':done.get('error'),'passed':text.strip()==token}
                            break
                        if api.call('GET',f'/api/session/{sid}/permission')['data']:
                            report['tool_roundtrip']={'passed':False,'error':'another_tool_request'};break
                        time.sleep(.2)
            finally:
                api.call('POST',f'/api/session/{sid}/interrupt',{});api.call('DELETE',f'/api/session/{sid}')
                api.call('DELETE','/api/experimental/mcp/loadoutprobe')
            report['remaining_sessions']=len(api.call('GET','/api/session')['data'])
            report['database_files']=len(list((root/'data').rglob('*.db')))
            response.close()
    finally:
        mcp.shutdown();mcp.server_close()
        path=Path(__file__).resolve().parent.parent/'docs/research/opencode-events-tools-probe-20261018.json'
        path.write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
        print('Receipt:',path)


if __name__=='__main__': main()
