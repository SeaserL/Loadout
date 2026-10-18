# SPDX-License-Identifier: Apache-2.0
"""Strict Chat/native message conversion for the official Agent bridge."""
import json
import re
import time
import uuid

MODEL = 'big-pickle'
MODELS = (MODEL, 'deepseek-v4.1-flash-free')
NAMESPACE = 'loadout'


class BridgeError(Exception):
    def __init__(self, status, code, retry_after=None):
        self.status, self.code, self.retry_after = status, code, retry_after


def valid_name(name):
    return isinstance(name, str) and re.fullmatch(r'[A-Za-z0-9_-]{1,48}', name) is not None


def arguments(value):
    try:
        result = json.loads(value)
        if not isinstance(result, dict): raise ValueError()
        return result
    except (ValueError, TypeError):
        raise BridgeError(400, 'invalid_tool_arguments') from None


def parse_tools(body):
    tools = body.get('tools', [])
    if not isinstance(tools, list) or len(tools) > 32:
        raise BridgeError(400, 'invalid_tools')
    names = set()
    for tool in tools:
        if not isinstance(tool, dict) or set(tool) != {'type', 'function'} or tool['type'] != 'function':
            raise BridgeError(400, 'function_tools_only')
        fn = tool['function']
        if not isinstance(fn, dict) or set(fn) - {'name', 'description', 'parameters', 'strict'}:
            raise BridgeError(400, 'invalid_function')
        if not valid_name(fn.get('name')) or fn['name'] in names:
            raise BridgeError(400, 'invalid_tool_name')
        if fn.get('strict', False) is not False:
            raise BridgeError(400, 'strict_tools_unsupported')
        if not isinstance(fn.get('description', ''), str) or not isinstance(fn.get('parameters'), dict) or fn['parameters'].get('type') != 'object':
            raise BridgeError(400, 'object_tool_schema_required')
        names.add(fn['name'])
    choice = body.get('tool_choice', 'auto')
    if isinstance(choice, dict):
        if set(choice) != {'type', 'function'} or choice['type'] != 'function' or not isinstance(choice['function'], dict) or set(choice['function']) != {'name'} or choice['function']['name'] not in names:
            raise BridgeError(400, 'invalid_tool_choice')
    elif choice not in ('auto', 'none', 'required'):
        raise BridgeError(400, 'invalid_tool_choice')
    if choice == 'required' and not tools: raise BridgeError(400, 'tool_choice_requires_tools')
    if not isinstance(body.get('parallel_tool_calls', True), bool):
        raise BridgeError(400, 'invalid_parallel_tool_calls')
    return tools if body.get('tool_choice') != 'none' else []


def parse_request(body):
    allowed = {'model', 'messages', 'stream', 'stream_options', 'tools', 'tool_choice', 'parallel_tool_calls', 'max_tokens', 'max_completion_tokens', 'store'}
    if not isinstance(body, dict) or set(body) - allowed:
        raise BridgeError(400, 'unsupported_parameter')
    if body.get('model') not in MODELS:
        raise BridgeError(400, 'unsupported_model')
    if body.get('store', False) is not False: raise BridgeError(400, 'stateful_unsupported')
    if 'max_tokens' in body and 'max_completion_tokens' in body: raise BridgeError(400, 'conflicting_token_limits')
    for field in ('max_tokens', 'max_completion_tokens'):
        if field in body and (type(body[field]) is not int or not 1 <= body[field] <= 32768):
            raise BridgeError(400, 'invalid_token_limit')
    if not isinstance(body.get('stream', False), bool):
        raise BridgeError(400, 'invalid_stream')
    if 'stream_options' in body and body['stream_options'] not in ({}, {'include_usage': True}, {'include_usage': False}):
        raise BridgeError(400, 'unsupported_stream_options')
    parse_tools(body)
    if body.get('tools') and body['model'] != 'deepseek-v4.1-flash-free':
        raise BridgeError(400, 'tools_not_verified_for_model')
    messages = body.get('messages')
    if not isinstance(messages, list) or not 1 <= len(messages) <= 64:
        raise BridgeError(400, 'invalid_messages')
    pending, seen = set(), set()
    previous = None
    for index, msg in enumerate(messages):
        if not isinstance(msg, dict): raise BridgeError(400, 'invalid_message')
        role, content = msg.get('role'), msg.get('content')
        allowed_fields = {'role', 'content'} | ({'tool_calls', 'reasoning_content'} if role == 'assistant' else {'tool_call_id'} if role == 'tool' else set())
        if set(msg) - allowed_fields:
            raise BridgeError(400, 'unsupported_message_field')
        if msg.get('reasoning_content') not in (None, ''):
            raise BridgeError(400, 'reasoning_history_unsupported')
        if role == 'tool':
            call_id = msg.get('tool_call_id')
            if not isinstance(call_id, str) or call_id not in pending or not isinstance(content, str):
                raise BridgeError(400, 'unmatched_tool_result')
            pending.remove(call_id)
        else:
            if pending: raise BridgeError(400, 'missing_tool_results')
            if role == 'system':
                if index != 0: raise BridgeError(400, 'invalid_message_order')
            elif role == 'user':
                if previous not in (None, 'system', 'assistant', 'tool', 'user'):
                    raise BridgeError(400, 'invalid_message_order')
            elif role == 'assistant':
                if previous not in ('user', 'tool'): raise BridgeError(400, 'invalid_message_order')
            else: raise BridgeError(400, 'unsupported_role')
            calls = msg.get('tool_calls', [])
            if not isinstance(calls, list) or len(calls) > 32:
                raise BridgeError(400, 'invalid_tool_calls')
            if not isinstance(content, str) and not (role == 'assistant' and calls and content is None):
                raise BridgeError(400, 'text_only')
            for call in calls:
                if body['model'] != 'deepseek-v4.1-flash-free':
                    raise BridgeError(400, 'tools_not_verified_for_model')
                if not isinstance(call, dict) or set(call) != {'id', 'type', 'function'} or call['type'] != 'function':
                    raise BridgeError(400, 'invalid_tool_call')
                ident, fn = call['id'], call['function']
                if not isinstance(ident, str) or not 1 <= len(ident) <= 256 or ident in seen:
                    raise BridgeError(400, 'duplicate_or_invalid_tool_call_id')
                if not isinstance(fn, dict) or set(fn) != {'name', 'arguments'} or not valid_name(fn['name']):
                    raise BridgeError(400, 'invalid_tool_function')
                arguments(fn['arguments'])
                seen.add(ident); pending.add(ident)
        previous = role
    if pending or previous not in ('user', 'tool'):
        raise BridgeError(400, 'incomplete_conversation')
    if previous == 'user' and not messages[-1]['content'].strip():
        raise BridgeError(400, 'empty_prompt')
    return messages


def request_controls(body):
    controls = {}
    for field in ('max_tokens', 'max_completion_tokens'):
        if field in body: controls['max_tokens'] = body[field]
    if 'tool_choice' in body:
        choice = body['tool_choice']
        controls['tool_choice'] = {'type': 'function', 'function': {'name': NAMESPACE + '_' + choice['function']['name']}} if isinstance(choice, dict) else choice
    return controls


def native_history(messages, model):
    now = int(time.time() * 1000) - len(messages) * 2
    history, pending = [], {}
    for index, message in enumerate(messages):
        if message['role'] == 'tool':
            part = pending.pop(message['tool_call_id'])
            part['state'].update(status='completed', content=[{'type': 'text', 'text': message['content']}])
            continue
        created = now + index * 2
        record = {'id': 'msg_' + uuid.uuid4().hex, 'time': {'created': created}, 'type': message['role']}
        if message['role'] == 'assistant':
            content = [{'type': 'text', 'text': message['content']}] if message.get('content') else []
            for call in message.get('tool_calls', []):
                part = {'type': 'tool', 'id': call['id'], 'name': NAMESPACE + '_' + call['function']['name'],
                        'executed': False, 'state': {'status': 'running', 'input': arguments(call['function']['arguments']), 'metadata': {}},
                        'time': {'created': created, 'ran': created, 'completed': created + 1}}
                pending[call['id']] = part; content.append(part)
            record.update(agent='build', model={'id': model, 'providerID': 'opencode'},
                          finish='tool-calls' if message.get('tool_calls') else 'stop', content=content)
            record['time']['completed'] = created + 1
        else:
            record['text'] = message['content']
        history.append(record)
    if pending: raise BridgeError(400, 'missing_tool_results')
    return history
