# SPDX-License-Identifier: Apache-2.0
"""Streaming text and external-function adapter for an official OpenCode server."""
import base64
import hmac
import json
import math
import os
from pathlib import Path
import threading
import time
import urllib.request
import urllib.error
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from opencode_protocol import BridgeError, MODEL, MODELS, NAMESPACE, parse_request, parse_tools, native_history, request_controls
from opencode_events import EventFeed, EventState
import secrets as random_secrets
import select
import socket
import queue

LIMIT = 65536


class Official:
    def __init__(self, port, password):
        self.base = f'http://127.0.0.1:{port}'
        self.auth = 'Basic ' + base64.b64encode(('opencode:' + password).encode()).decode()
        self.lock = threading.Lock()
        self.healthy = True
        self.mcp_url = None
        self.mcp_key = random_secrets.token_urlsafe(32)
        self.tools = []
        self.control_session = None
        self.controls = {}
        self.controls_applied = False

    def call(self, method, path, body=None):
        data = json.dumps(body).encode() if body is not None else None
        req = urllib.request.Request(self.base + path, data=data, method=method,
            headers={'Authorization': self.auth, 'Content-Type': 'application/json'})
        try:
            with urllib.request.urlopen(req, timeout=5) as response:
                raw = response.read(2 * 1024 * 1024 + 1)
                if len(raw) > 2 * 1024 * 1024:
                    raise BridgeError(502, 'official_response_too_large')
                return json.loads(raw) if raw else None
        except BridgeError:
            raise
        except Exception:
            raise BridgeError(503, 'official_server_unavailable') from None

    def run(self, messages, model=MODEL, tools=None, parallel=True, cancelled=lambda: False, controls=None):
        if not self.lock.acquire(blocking=False):
            raise BridgeError(429, 'bridge_busy')
        session, feed, registered, result = None, None, False, None
        try:
            if not self.healthy:
                raise BridgeError(503, 'session_cleanup_failed_restart_required')
            self.tools = tools or []
            if self.tools:
                if not self.mcp_url: raise BridgeError(503, 'tool_catalog_unavailable')
                registered = True
                self.call('PUT', '/api/experimental/mcp/' + NAMESPACE, {'config': {
                    'type': 'remote', 'url': self.mcp_url,
                    'headers': {'Authorization': 'Bearer ' + self.mcp_key}, 'oauth': False, 'codemode': False}})
                deadline = time.monotonic() + 5
                while time.monotonic() < deadline:
                    entries = self.call('GET', '/api/mcp')['data']
                    if any(e['name'] == NAMESPACE and e['status']['status'] == 'connected' for e in entries): break
                    time.sleep(.05)
                else: raise BridgeError(503, 'tool_catalog_unavailable')
                # The official tool registry debounces MCP catalog changes by 100 ms.
                time.sleep(.2)
            created = self.call('POST', '/api/session', {
                'title': 'Loadout transient request',
                'model': {'id': model, 'providerID': 'opencode'},
                'permissions': [{'action': '*', 'resource': '*', 'effect': 'ask'}]})
            session = created['data']['id']
            self.control_session, self.controls, self.controls_applied = session, controls or {}, False
            if self.tools: self.controls['external_tools'] = [NAMESPACE + '_' + t['function']['name'] for t in self.tools]
            prompt = messages[-1]['role'] == 'user'
            history = native_history(messages[:-1] if prompt else messages, model)
            if history:
                self.call('DELETE', f'/api/session/{session}')
                self.call('POST', '/api/experimental/session/import', {
                    'info': created['data'], 'messages': history, 'location': created['data']['location']})
                stored = self.call('GET', f'/api/session/{session}/message?limit=100')['data']
                if {m['id'] for m in stored} != {m['id'] for m in history}:
                    raise BridgeError(502, 'history_import_incomplete')
            feed = EventFeed(self)
            state = EventState(session, model, self.tools, parallel)
            self.call('POST', f'/api/session/{session}/' + ('prompt' if prompt else 'synthetic'),
                      {'text': messages[-1]['content'] if prompt else ''})
            deadline = time.monotonic() + 45
            while time.monotonic() < deadline:
                if cancelled(): raise BridgeError(499, 'client_disconnected')
                event = feed.next()
                if event is None: continue
                for delta in state.accept(event):
                    if self.controls and not self.controls_applied: raise BridgeError(502, 'request_controls_not_applied')
                    yield ('delta', delta)
                if not state.done: continue
                result = state.result()
                if self.controls and not self.controls_applied: raise BridgeError(502, 'request_controls_not_applied')
                choice = self.controls.get('tool_choice')
                if choice == 'required' or isinstance(choice, dict):
                    if not state.calls: raise BridgeError(502, 'required_tool_not_called')
                    if isinstance(choice, dict) and any(NAMESPACE + '_' + c['function']['name'] != choice['function']['name'] for c in state.calls.values()):
                        raise BridgeError(502, 'forced_tool_mismatch')
                if state.calls:
                    permission_deadline = time.monotonic() + 2
                    while time.monotonic() < permission_deadline:
                        pending = self.call('GET', f'/api/session/{session}/permission')['data']
                        by_id = {p.get('source', {}).get('id'): p for p in pending}
                        if set(by_id) - set(state.calls): raise BridgeError(422, 'agent_tool_not_requested')
                        if set(by_id) == set(state.calls):
                            for ident, call in state.calls.items():
                                permission = by_id[ident]
                                if permission['action'] != NAMESPACE + '_' + call['function']['name'] or permission.get('metadata', {}).get('input') != json.loads(call['function']['arguments']):
                                    raise BridgeError(502, 'tool_permission_mismatch')
                            break
                        time.sleep(.05)
                    else: raise BridgeError(502, 'tool_handoff_not_ready')
                break
            else: raise BridgeError(504, 'official_model_timeout')
        finally:
            if feed:
                try: feed.close()
                except Exception: self.healthy = False
            if session:
                try:
                    self.call('POST', f'/api/session/{session}/interrupt', {})
                    self.call('DELETE', f'/api/session/{session}')
                except Exception: self.healthy = False
            if registered:
                try: self.call('DELETE', '/api/experimental/mcp/' + NAMESPACE)
                except Exception: self.healthy = False
            self.tools = []
            self.control_session, self.controls, self.controls_applied = None, {}, False
            self.lock.release()
        if not self.healthy: raise BridgeError(503, 'session_cleanup_failed_restart_required')
        yield ('result', result)

    def complete(self, messages, model=MODEL, tools=None, parallel=True):
        for kind, value in self.run(messages, model, tools, parallel):
            if kind == 'result': return value


class OfficialPool:
    """Bounded admission over independent persistent official processes."""
    def __init__(self, workers, waiting=8, wait_seconds=45):
        self.workers = workers
        self.available = queue.Queue()
        for worker in workers: self.available.put(worker)
        self.admission = threading.BoundedSemaphore(len(workers) + waiting)
        self.wait_seconds = wait_seconds

    def run(self, messages, model=MODEL, tools=None, parallel=True, cancelled=lambda: False, controls=None):
        if not self.admission.acquire(blocking=False):
            raise BridgeError(429, 'bridge_queue_full')
        worker = None
        try:
            deadline = time.monotonic() + self.wait_seconds
            while worker is None:
                if cancelled(): raise BridgeError(499, 'client_disconnected')
                if not any(w.healthy for w in self.workers):
                    raise BridgeError(503, 'session_cleanup_failed_restart_required')
                if time.monotonic() >= deadline: raise BridgeError(429, 'bridge_queue_timeout')
                try: worker = self.available.get(timeout=.1)
                except queue.Empty: continue
                if not worker.healthy: worker = None
            if cancelled(): raise BridgeError(499, 'client_disconnected')
            yield from worker.run(messages, model, tools, parallel, cancelled, controls)
        finally:
            if worker is not None and worker.healthy: self.available.put(worker)
            self.admission.release()


class ProtectedOfficial:
    """Conservative bridge-wide admission; no probe or automatic inference retry."""
    def __init__(self, worker, state_path, clock=time.time):
        self.workers = [worker]
        self.worker, self.path, self.clock = worker, Path(state_path), clock
        self.lock = threading.Lock()
        self.next_at = self.blocked_until = 0
        if self.path.exists():
            try:
                state = json.loads(self.path.read_text())
                self.next_at = float(state['next_at'])
                self.blocked_until = float(state['blocked_until'])
                if not all(math.isfinite(x) for x in (self.next_at, self.blocked_until)):
                    raise ValueError('Invalid deadline')
            except (ValueError, KeyError, OSError, TypeError):
                self.blocked_until = clock() + 1800

    def save(self):
        temp = self.path.with_suffix('.tmp')
        temp.write_text(json.dumps({'next_at': self.next_at, 'blocked_until': self.blocked_until}))
        temp.replace(self.path)

    def run(self, *args, **kwargs):
        if not self.lock.acquire(blocking=False):
            raise BridgeError(429, 'bridge_request_in_progress', 120)
        try:
            now = self.clock()
            deadline = max(self.next_at, self.blocked_until)
            if now < deadline:
                raise BridgeError(429, 'bridge_cooling_down' if now < self.blocked_until else 'bridge_minimum_interval', max(1, math.ceil(deadline-now)))
            self.next_at = now + 120
            self.save()
            try:
                yield from self.worker.run(*args, **kwargs)
            except BridgeError as error:
                if error.code == 'official_access_restricted':
                    self.blocked_until = self.clock() + 1800
                    self.save()
                raise
        finally:
            self.lock.release()


def make_handler(official, key):
    workers = getattr(official, 'workers', [official])
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def reply(self, status, payload, retry_after=None):
            raw = json.dumps(payload).encode()
            self.send_response(status)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(raw)))
            if retry_after is not None: self.send_header('Retry-After', str(retry_after))
            self.end_headers()
            self.wfile.write(raw)

        def authenticated(self):
            if not hmac.compare_digest(self.headers.get('Authorization', ''), 'Bearer ' + key):
                self.reply(401, {'error': {'type': 'authentication_error', 'message': 'Invalid bridge key'}})
                return False
            return True

        def do_GET(self):
            if self.path.startswith('/internal/control/'):
                self.control(False); return
            if self.path == '/internal/mcp':
                self.send_response(405)
                self.end_headers()
                return
            if not self.authenticated():
                return
            try:
                if self.path == '/health':
                    for worker in workers:
                        info = worker.call('GET', '/api/info')
                        if not worker.healthy or info.get('version') != '2.0.32':
                            raise BridgeError(503, 'official_version_or_cleanup_mismatch')
                        plugins = worker.call('GET', '/api/plugin')['data']
                        if not any(p.get('id') == 'loadout.request-controls' and p.get('state', {}).get('status') == 'active' for p in plugins):
                            raise BridgeError(503, 'request_controls_plugin_not_ready')
                    self.reply(200, {'status': 'ok', 'version': '2.0.32', 'workers': len(workers)})
                elif self.path == '/v1/models':
                    self.reply(200, {'object': 'list', 'data': [{'id': model, 'object': 'model', 'owned_by': 'opencode'} for model in MODELS]})
                else:
                    self.reply(404, {'error': {'message': 'Not found'}})
            except BridgeError as err:
                self.reply(err.status, {'error': {'message': err.code, 'type': err.code}})

        def cancelled(self):
            try:
                readable, _, _ = select.select([self.connection], [], [], 0)
                return bool(readable) and self.connection.recv(1, socket.MSG_PEEK) == b''
            except OSError: return True

        def control(self, acknowledge):
            official = next((w for w in workers if self.path == '/internal/control/' + str(w.control_session) and w.control_session is not None), workers[0])
            if not hmac.compare_digest(self.headers.get('Authorization', ''), official.auth):
                self.reply(401, {'error': 'unauthorized'}); return
            if self.path != '/internal/control/' + str(official.control_session) or official.control_session is None:
                self.reply(404, {'error': 'inactive_session'}); return
            if not acknowledge:
                self.reply(200, official.controls); return
            length = int(self.headers.get('Content-Length', '0'))
            if not 0 < length <= 4096: raise BridgeError(413, 'invalid_control_size')
            observed = json.loads(self.rfile.read(length))
            if any(observed.get(k) != v for k, v in official.controls.items()):
                raise BridgeError(502, 'request_control_mismatch')
            if official.controls_applied: raise BridgeError(409, 'automatic_continuation_not_allowed')
            official.controls_applied = True
            self.reply(200, {'ok': True})

        def mcp(self):
            official = next((w for w in workers if hmac.compare_digest(self.headers.get('Authorization', ''), 'Bearer ' + w.mcp_key)), workers[0])
            if not hmac.compare_digest(self.headers.get('Authorization', ''), 'Bearer ' + official.mcp_key):
                self.reply(401, {'error': 'unauthorized'}); return
            length = int(self.headers.get('Content-Length', '0'))
            if not 0 < length <= LIMIT: raise BridgeError(413, 'invalid_request_size')
            body = json.loads(self.rfile.read(length))
            if 'id' not in body:
                self.send_response(202); self.end_headers(); return
            method = body.get('method')
            if method == 'initialize':
                result = {'protocolVersion': '2024-11-05', 'capabilities': {'tools': {}},
                          'serverInfo': {'name': 'loadout-external-tools', 'version': '1'}}
            elif method == 'tools/list':
                result = {'tools': [{'name': t['function']['name'], 'description': t['function'].get('description', ''),
                          'inputSchema': t['function']['parameters']} for t in official.tools]}
            elif method == 'tools/call':
                official.healthy = False
                result = {'isError': True, 'content': [{'type': 'text', 'text': 'External tools must be executed by the API client'}]}
            else: result = {}
            self.reply(200, {'jsonrpc': '2.0', 'id': body['id'], 'result': result})

        def do_POST(self):
            committed, iterator = False, None
            try:
                if self.path.startswith('/internal/control/'):
                    self.control(True); return
                if self.path == '/internal/mcp':
                    self.mcp(); return
                if not self.authenticated(): return
                if self.path != '/v1/chat/completions':
                    raise BridgeError(404, 'unsupported_endpoint')
                length = int(self.headers.get('Content-Length', '0'))
                if not 0 < length <= LIMIT or self.headers.get('Transfer-Encoding'):
                    raise BridgeError(413, 'invalid_request_size')
                self.connection.settimeout(10)
                body = json.loads(self.rfile.read(length))
                messages = parse_request(body)
                response = {'id': 'chatcmpl-' + uuid.uuid4().hex, 'created': int(time.time()), 'model': body['model']}
                def emit(payload):
                    nonlocal committed
                    if not committed:
                        self.send_response(200)
                        self.send_header('Content-Type', 'text/event-stream')
                        self.send_header('Cache-Control', 'no-cache')
                        self.send_header('Connection', 'close')
                        self.end_headers()
                        self.close_connection = True
                        committed = True
                    self.wfile.write(('data: ' + (payload if isinstance(payload, str) else json.dumps(payload)) + '\n\n').encode())
                    self.wfile.flush()
                iterator = official.run(messages, body['model'], parse_tools(body), body.get('parallel_tool_calls', True), self.cancelled, request_controls(body))
                for kind, value in iterator:
                    if kind == 'delta' and body.get('stream', False):
                        if not committed: value = dict(value, role='assistant')
                        emit(dict(response, object='chat.completion.chunk', choices=[{'index': 0, 'delta': value, 'finish_reason': None}]))
                    if kind == 'result':
                        if body.get('stream', False):
                            emit(dict(response, object='chat.completion.chunk', choices=[{'index': 0, 'delta': {}, 'finish_reason': value['finish_reason']}]))
                            emit('[DONE]')
                        else:
                            self.reply(200, dict(response, object='chat.completion', choices=[dict(value, index=0)]))
            except (BrokenPipeError, ConnectionResetError, TimeoutError):
                pass
            except Exception as err:
                error = err if isinstance(err, BridgeError) else BridgeError(400, 'invalid_request') if isinstance(err, (ValueError, TypeError)) else BridgeError(503, 'bridge_failure')
                if committed:
                    try:
                        self.wfile.write(('data: ' + json.dumps({'error': {'message': error.code, 'type': error.code}}) + '\n\n').encode())
                        self.wfile.flush()
                    except OSError: pass
                elif error.status != 499:
                    self.reply(error.status, {'error': {'message': error.code, 'type': error.code, 'retry_after': error.retry_after}}, error.retry_after)
            finally:
                if iterator: iterator.close()

    return Handler


if __name__ == '__main__':
    from workbuddy_bridge_key import protect
    root = Path(os.environ['LOADOUT_OPENCODE_DIR'])
    config = json.loads((root / 'binding.json').read_text(encoding='utf-8-sig'))
    secrets = json.loads(protect((root / 'secrets.dpapi').read_bytes(), True))
    ports = [config['official_port']]
    if not 1 <= len(ports) <= 2 or len(set(ports)) != len(ports): raise RuntimeError('Invalid worker ports')
    workers = [Official(port, secrets['official']) for port in ports]
    for worker in workers: worker.mcp_url = f"http://127.0.0.1:{config['port']}/internal/mcp"
    official = ProtectedOfficial(workers[0], root / 'admission.json')
    server = ThreadingHTTPServer(('127.0.0.1', config['port']), make_handler(official, secrets['bridge']))
    server.serve_forever()
