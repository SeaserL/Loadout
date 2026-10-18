// SPDX-License-Identifier: Apache-2.0
import http from 'node:http';
import crypto from 'node:crypto';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { WorkBuddyIpc } from './workbuddy_ipc_probe.mjs';

export const MODEL = 'deepseek-v4.1-flash';
const ROOT = path.join(process.env.LOCALAPPDATA || os.tmpdir(), 'LoadoutWorkBuddyBridge');
const AUTH_FILE = path.join(process.env.LOCALAPPDATA || '', 'CodeBuddyExtension', 'Data', 'Public', 'auth', 'workbuddy-desktop-ai.info');
const message = (code, text) => ({ error: { type: code, message: text } });
function json(response, status, body) { response.writeHead(status, { 'content-type': 'application/json', 'cache-control': 'no-store' }); response.end(JSON.stringify(body)); }

async function accountHash() {
  // Read only the non-secret account identifier. Token fields remain encrypted and unused.
  const session = JSON.parse(await fs.readFile(AUTH_FILE, 'utf8'));
  const uid = session.account?.uid;
  if (typeof uid !== 'string' || !uid) throw new Error('Account identifier unavailable');
  return crypto.createHash('sha256').update('loadout-workbuddy-account\0' + uid).digest('hex');
}

export function decodeResponse(raw) {
  const chunks = []; let done = false;
  for (const line of raw.split(/\r?\n/)) {
    if (!line.startsWith('data:')) continue;
    const value = line.slice(5).trim();
    if (value === '[DONE]') { done = true; continue; }
    let chunk; try { chunk = JSON.parse(value); } catch { throw new Error('invalid_stream'); }
    if (chunk.error || chunk.code && chunk.code !== 0) throw new Error('upstream_error');
    if (chunk.model && chunk.model !== MODEL) throw new Error('model_mismatch');
    for (const choice of chunk.choices || []) {
      if (choice.delta?.tool_calls?.length || choice.delta?.function_call?.name || choice.delta?.function_call?.arguments) throw new Error('unsupported_output');
    }
    chunks.push(chunk);
  }
  if (!done || !chunks.some(c => c.model === MODEL) || !chunks.some(c => c.choices?.some(v => v.finish_reason))) throw new Error('incomplete_stream');
  return chunks;
}

export function createBridge({ key, boundAccountHash, readAccountHash = accountHash, fetchUpstream }) {
  let busy = false;
  const expected = crypto.createHash('sha256').update('Bearer ' + key).digest();
  return http.createServer(async (request, response) => {
    let ownsSlot = false;
    const supplied = crypto.createHash('sha256').update(request.headers.authorization || '').digest();
    if (!crypto.timingSafeEqual(supplied, expected)) return json(response, 401, message('authentication_error', 'Bridge key required'));
    if (request.headers.origin) return json(response, 403, message('origin_rejected', 'Browser origins are not supported'));
    try {
      if (await readAccountHash() !== boundAccountHash) return json(response, 409, message('account_changed', 'WorkBuddy account changed; rebind required'));
      if (request.method === 'GET' && request.url === '/health') return json(response, 200, { status: 'ok', mode: 'buffered', upstream_verified_at: '2026-10-17' });
      if (request.method === 'GET' && request.url === '/v1/models') return json(response, 200, { object: 'list', data: [{ id: MODEL, object: 'model', owned_by: 'workbuddy', name: 'DeepSeek V4.1 Flash (buffered IPC)' }] });
      if (request.method !== 'POST' || request.url !== '/v1/chat/completions') return json(response, 404, message('not_found', 'Unsupported endpoint'));
      if (busy) { response.setHeader('retry-after', '2'); return json(response, 429, message('busy', 'One request is already running')); }
      busy = true;
      ownsSlot = true;
      let bytes = 0; const parts = [];
      for await (const chunk of request) {
        bytes += chunk.length;
        if (bytes > 256 * 1024) return json(response, 413, message('request_too_large', 'Request limit is 256 KiB'));
        parts.push(chunk);
      }
      let body;
      try { body = JSON.parse(Buffer.concat(parts).toString('utf8')); } catch { return json(response, 400, message('invalid_json', 'Invalid JSON')); }
      if (!body || body.model !== MODEL) return json(response, 400, message('model_not_supported', 'Only deepseek-v4.1-flash is supported'));
      if (body.tools?.length || body.functions || body.tool_choice || body.previous_response_id || body.store === true || (body.n !== undefined && body.n !== 1)) return json(response, 400, message('unsupported_feature', 'Tools, stateful requests and multiple choices are not supported'));
      if (!Array.isArray(body.messages) || !body.messages.length || body.messages.some(m => !m || !['system', 'user', 'assistant'].includes(m.role) || typeof m.content !== 'string' || m.tool_calls || m.function_call)) return json(response, 400, message('unsupported_messages', 'Only text system, user and assistant messages are supported'));
      const maxTokens = body.max_tokens ?? body.max_completion_tokens ?? 4096;
      if (!Number.isInteger(maxTokens) || maxTokens < 1 || maxTokens > 8192) return json(response, 400, message('output_limit', 'max_tokens must be between 1 and 8192'));
      const upstream = { ...body, max_tokens: maxTokens, stream: true, stream_options: { include_usage: true } };
      delete upstream.max_completion_tokens;
      // Each API call is stateless; never invent or reuse a previous_response_id.
      const ipc = fetchUpstream ? undefined : new WorkBuddyIpc();
      const onClose = () => { if (!response.writableEnded) ipc?.close(); };
      response.on('close', onClose);
      let result;
      try {
        if (ipc) await ipc.connect();
        result = fetchUpstream ? await fetchUpstream(upstream) : await ipc.fetch('POST', '/v2/chat/completions', upstream);
      } finally { response.off('close', onClose); ipc?.close(); }
      if (response.destroyed) return;
      if (result.status !== 200) {
        const status = [401, 403, 429].includes(result.status) ? result.status : 502;
        if (/^\d+$/.test(result.headers?.['retry-after'] || '')) response.setHeader('retry-after', result.headers['retry-after']);
        return json(response, status, message('upstream_error', 'WorkBuddy rejected the request'));
      }
      const raw = Buffer.from(result.body_b64, 'base64').toString('utf8');
      let chunks;
      try { chunks = decodeResponse(raw); } catch { return json(response, 502, message('invalid_upstream_response', 'Incomplete, unsupported or different-model response')); }
      response.setHeader('x-loadout-stream-mode', 'buffered');
      if (body.stream === true) {
        response.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-store' }); response.end(raw);
      } else {
        let content = ''; let reasoning = ''; let usage; let finish = 'stop';
        for (const chunk of chunks) {
          if (chunk.usage) usage = chunk.usage;
          for (const choice of chunk.choices || []) {
            content += choice.delta?.content || ''; reasoning += choice.delta?.reasoning_content || '';
            if (choice.finish_reason) finish = choice.finish_reason;
          }
        }
        json(response, 200, { id: chunks[0].id || 'chatcmpl-' + crypto.randomUUID(), object: 'chat.completion',
          created: chunks[0].created || Math.floor(Date.now() / 1000), model: MODEL,
          choices: [{ index: 0, message: { role: 'assistant', content, ...(reasoning ? { reasoning_content: reasoning } : {}) }, finish_reason: finish }], ...(usage ? { usage } : {}) });
      }
    } catch { if (!response.headersSent && !response.destroyed) json(response, 503, message('client_unavailable', 'WorkBuddy client session or IPC is unavailable')); }
    finally { if (ownsSlot) busy = false; }
  });
}

async function main() {
  if (process.argv[2] === 'bind') {
    await fs.mkdir(ROOT, { recursive: true });
    const state = { account_hash: await accountHash(), port: 18788, python: process.argv[3] };
    if (!state.python || !path.isAbsolute(state.python)) throw new Error('Absolute Python path required');
    await fs.writeFile(path.join(ROOT, 'binding.json'), JSON.stringify(state));
    console.log('WorkBuddy desktop account bound (identifier hashed)'); return;
  }
  const config = JSON.parse(await fs.readFile(path.join(ROOT, 'binding.json'), 'utf8'));
  const key = execFileSync(config.python, [path.join(path.dirname(fileURLToPath(import.meta.url)), 'workbuddy_bridge_key.py'), 'read'], { encoding: 'utf8', windowsHide: true, timeout: 10000 }).trim();
  if (key.length < 32) throw new Error('Invalid bridge key');
  const server = createBridge({ key, boundAccountHash: config.account_hash });
  server.requestTimeout = 60000; server.headersTimeout = 10000;
  server.on('error', () => { console.error('Bridge listener failed'); process.exitCode = 1; });
  server.listen(config.port, '127.0.0.1', () => { console.log('WorkBuddy bridge ready on loopback'); });
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main().catch(() => { console.error('WorkBuddy bridge startup failed'); process.exitCode = 1; });
