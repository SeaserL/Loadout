# SPDX-License-Identifier: Apache-2.0
"""Bounded native SSE subscription and exact-model event state machine."""
import json
import queue
import socket
import threading
import urllib.request
from opencode_protocol import BridgeError, NAMESPACE, arguments


class EventFeed:
    def __init__(self, api):
        self.events = queue.Queue(maxsize=512)
        self.failed = threading.Event()
        self.closed = threading.Event()
        request = urllib.request.Request(api.base + '/api/event', headers={'Authorization': api.auth})
        try:
            self.response = urllib.request.urlopen(request, timeout=20)
            if 'text/event-stream' not in self.response.headers.get('Content-Type', ''):
                self.response.close()
                raise BridgeError(502, 'invalid_official_event_stream')
        except BridgeError: raise
        except Exception: raise BridgeError(503, 'official_events_unavailable') from None
        self.thread = threading.Thread(target=self._read, daemon=True)
        self.thread.start()
        try:
            first = self.events.get(timeout=5)
            if first.get('type') != 'server.connected':
                raise BridgeError(502, 'official_event_handshake_failed')
        except Exception:
            self.close()
            raise BridgeError(503, 'official_event_handshake_failed') from None

    def _read(self):
        try:
            data, size = [], 0
            while not self.closed.is_set():
                line = self.response.readline(2 * 1024 * 1024 + 1)
                if not line: break
                size += len(line)
                if size > 2 * 1024 * 1024: break
                if line.strip() == b'':
                    if data:
                        event = json.loads(b'\n'.join(data))
                        if isinstance(event, str): event = json.loads(event)
                        if not isinstance(event, dict) or not isinstance(event.get('type'), str): break
                        self.events.put_nowait(event)
                    data, size = [], 0
                elif line.startswith(b'data:'):
                    data.append(line[5:].lstrip())
        except Exception:
            pass
        finally:
            if not self.closed.is_set(): self.failed.set()

    def next(self):
        if self.failed.is_set(): raise BridgeError(502, 'official_event_stream_lost')
        try: return self.events.get(timeout=.2)
        except queue.Empty: return None

    def close(self):
        self.closed.set()
        try: self.response.fp.raw._sock.shutdown(socket.SHUT_RDWR)
        except (AttributeError, OSError): pass
        self.response.close()
        self.thread.join(timeout=1)


class EventState:
    def __init__(self, session, model, tools, parallel=True):
        self.session, self.model = session, model
        self.names = {NAMESPACE + '_' + t['function']['name']: t['function']['name'] for t in tools}
        self.parallel = parallel
        self.message = None
        self.text = {}
        self.text_ended = set()
        self.calls = {}
        self.calls_ended = set()
        self.done = False
        self.finish = None
        self.output_bytes = 0

    def accept(self, event):
        data, typ = event.get('data', {}), event['type']
        if data.get('sessionID') != self.session: return []
        self.output_bytes += len(json.dumps(data).encode())
        if self.output_bytes > 4 * 1024 * 1024:
            raise BridgeError(502, 'official_output_too_large')
        if typ == 'session.step.failed':
            error = data.get('error', {})
            if error.get('status') == 403:
                raise BridgeError(429, 'official_access_restricted', 1800)
            raise BridgeError(502, 'official_model_failed')
        if typ == 'session.step.started':
            if self.message is not None: raise BridgeError(502, 'unexpected_agent_continuation')
            model = data.get('model', {})
            if model.get('id') != self.model or model.get('providerID') != 'opencode':
                raise BridgeError(502, 'model_mismatch')
            self.message = data['assistantMessageID']
            return []
        relevant = typ.startswith(('session.text.', 'session.tool.', 'session.step.'))
        if not relevant: return []
        if not self.message or data.get('assistantMessageID') != self.message:
            raise BridgeError(502, 'official_event_order_invalid')
        if typ == 'session.text.started':
            ordinal = data['ordinal']
            if ordinal in self.text: raise BridgeError(502, 'duplicate_text_block')
            self.text[ordinal] = ''
        elif typ == 'session.text.delta':
            ordinal = data['ordinal']
            if ordinal not in self.text or ordinal in self.text_ended:
                raise BridgeError(502, 'official_text_delta_invalid')
            self.text[ordinal] += data['delta']
            return [{'content': data['delta']}] if data['delta'] else []
        elif typ == 'session.text.ended':
            ordinal = data['ordinal']
            if self.text.get(ordinal) != data['text']:
                raise BridgeError(502, 'official_text_delta_lost')
            self.text_ended.add(ordinal)
        elif typ == 'session.tool.input.started':
            ident, name = data['id'], data['name']
            if name not in self.names: raise BridgeError(422, 'agent_tool_not_requested')
            if ident in self.calls: raise BridgeError(502, 'duplicate_tool_call')
            if len(self.calls) >= 32: raise BridgeError(502, 'too_many_tool_calls')
            if self.calls and not self.parallel: raise BridgeError(422, 'parallel_tool_calls_not_allowed')
            call = {'index': len(self.calls), 'id': ident, 'type': 'function',
                    'function': {'name': self.names[name], 'arguments': ''}}
            self.calls[ident] = call
            return [{'tool_calls': [dict(call, function=dict(call['function']))]}]
        elif typ == 'session.tool.input.ended':
            ident = data['id']
            if ident not in self.calls or ident in self.calls_ended:
                raise BridgeError(502, 'official_tool_order_invalid')
            try: arguments(data['text'])
            except BridgeError: raise BridgeError(502, 'official_tool_arguments_invalid') from None
            self.calls[ident]['function']['arguments'] = data['text']
            self.calls_ended.add(ident)
            return [{'tool_calls': [{'index': self.calls[ident]['index'], 'function': {'arguments': data['text']}}]}]
        elif typ == 'session.tool.failed':
            raise BridgeError(502, 'official_tool_failed')
        elif typ == 'session.step.streamed' and self.calls:
            if set(self.calls) != self.calls_ended: raise BridgeError(502, 'incomplete_tool_arguments')
            if self.text_ended != set(self.text): raise BridgeError(502, 'incomplete_text_output')
            self.finish, self.done = 'tool_calls', True
        elif typ == 'session.step.ended':
            if data.get('finish') not in ('stop', 'length') or self.calls:
                raise BridgeError(422, 'agent_action_not_supported')
            if (not self.text and data['finish'] != 'length') or self.text_ended != set(self.text):
                raise BridgeError(502, 'incomplete_text_output')
            self.finish, self.done = data['finish'], True
        return []

    def result(self):
        if not self.done: raise BridgeError(502, 'incomplete_official_output')
        text = ''.join(self.text[k] for k in sorted(self.text))
        if not text and not self.calls and self.finish != 'length': raise BridgeError(502, 'empty_model_output')
        message = {'role': 'assistant', 'content': text or None}
        if self.calls:
            message['tool_calls'] = [{k: v for k, v in call.items() if k != 'index'} for call in self.calls.values()]
        return {'message': message, 'finish_reason': self.finish}
