// SPDX-License-Identifier: Apache-2.0
// Original protocol client for the installed WorkBuddy broker. No token extraction.
import net from 'node:net';
import crypto from 'node:crypto';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

export class WorkBuddyIpc {
  pending = new Map();
  nextId = 1;
  socket;
  buffer = Buffer.alloc(0);
  handshake;
  async connect() {
    const discovery = JSON.parse(await fs.readFile(path.join(os.homedir(), '.workbuddy-ai', 'wbipc', 'endpoint.json'), 'utf8'));
    if (!/^\\\\\.\\pipe\\wbipc-[a-f0-9]+$/.test(discovery.endpoint)) throw new Error('Unexpected IPC endpoint');
    const nonce = crypto.randomBytes(16).toString('base64url');
    let challengeNonce;
    const proof = side => {
      const fields = [side === 'server' ? 'wbipc-s' : 'wbipc-c', '1', discovery.endpoint, nonce, challengeNonce];
      const parts = fields.flatMap(value => {
        const data = Buffer.from(value); const size = Buffer.alloc(4); size.writeUInt32BE(data.length);
        return [size, data];
      });
      return crypto.createHmac('sha256', discovery.ticket).update(Buffer.concat(parts)).digest('base64url');
    };
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => { this.close(); reject(new Error('IPC handshake timeout')); }, 7000);
      this.handshake = message => {
        if (message.type === 'session_challenge') {
          challengeNonce = message.server_nonce;
          const expected = Buffer.from(proof('server'));
          const actual = Buffer.from(message.server_proof || '');
          if (actual.length !== expected.length || !crypto.timingSafeEqual(actual, expected)) {
            clearTimeout(timer); this.close(); reject(new Error('IPC server authentication failed')); return;
          }
          this.send({ type: 'session_prove', client_proof: proof('client') });
        } else if (message.type === 'session_hello_ack') {
          clearTimeout(timer); this.handshake = undefined; this.pipes = message.pipes; resolve();
        } else {
          clearTimeout(timer); this.close(); reject(new Error('IPC handshake rejected'));
        }
      };
      this.socket = net.createConnection(discovery.endpoint);
      this.socket.on('connect', () => this.send({ type: 'session_hello', protocol_min: 1, protocol_max: 1,
        client_nonce: nonce, ticket_id: crypto.createHash('sha256').update(discovery.ticket).digest('hex').slice(0, 16),
        client: { kind: 'loadout', id: 'workbuddy-feasibility-probe', version: '0.1' } }));
      this.socket.on('data', chunk => {
        this.buffer = Buffer.concat([this.buffer, chunk]);
        if (this.buffer.length > 2 * 1024 * 1024) { this.close(); return; }
        let end;
        while ((end = this.buffer.indexOf(10)) >= 0) {
          const line = this.buffer.subarray(0, end); this.buffer = this.buffer.subarray(end + 1);
          if (!line.length) continue;
          let message;
          try { message = JSON.parse(line); } catch { this.close(); return; }
          if (this.handshake) this.handshake(message);
          else if (this.pending.has(message.id)) {
            const waiter = this.pending.get(message.id); this.pending.delete(message.id); clearTimeout(waiter.timer);
            if (message.error) waiter.reject(new Error('IPC request failed: ' + String(message.error.code)));
            else waiter.resolve(message.result);
          }
        }
      });
      this.socket.on('error', () => { clearTimeout(timer); reject(new Error('IPC connection failed')); this.close(); });
      this.socket.on('close', () => { clearTimeout(timer); reject(new Error('IPC connection closed')); this.close(); });
    });
    const pipe = await this.call('broker/GetPipe', { pipe: 'wb.request' });
    if (!pipe.methods.includes('http.fetch')) throw new Error('HTTP broker method unavailable');
    this.channel = pipe.channel;
  }
  send(message) { this.socket.write(JSON.stringify(message) + '\n'); }
  call(method, params, mode) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { this.pending.delete(id); this.send({ jsonrpc: '2.0', method: '$/cancel', params: { id } }); reject(new Error('IPC request timeout')); }, 55000);
      this.pending.set(id, { resolve, reject, timer });
      this.send({ jsonrpc: '2.0', id, method, params, ...(mode ? { mode } : {}) });
    });
  }
  async fetch(method, target, body, query) {
    if (!['/v3/config', '/v2/chat/completions'].includes(target)) throw new Error('Probe path is not allowlisted');
    return this.call(this.channel + '/http.fetch', { method, path: target,
      headers: { accept: 'application/json', 'content-type': 'application/json' },
      ...(query ? { query } : {}), ...(body ? { body_b64: Buffer.from(JSON.stringify(body)).toString('base64') } : {}) }, 'call');
  }
  close() {
    this.socket?.destroy();
    for (const waiter of this.pending.values()) { clearTimeout(waiter.timer); waiter.reject(new Error('IPC closed')); }
    this.pending.clear();
  }
}

async function main() {
  if (process.env.LOADOUT_LIVE_TESTS !== '1') throw new Error('LOADOUT_LIVE_TESTS=1 required');
  const action = process.argv[2] || 'catalog';
  if (!['catalog', 'smoke'].includes(action)) throw new Error('Unknown probe action');
  const ipc = new WorkBuddyIpc();
  try {
    await ipc.connect();
    console.log(JSON.stringify({ status: 'IPC_AUTHENTICATED', backend_request_available: ipc.pipes.includes('wb.request') }));
    const response = action === 'catalog'
      ? await ipc.fetch('GET', '/v3/config', undefined, { repos: '' })
      : await ipc.fetch('POST', '/v2/chat/completions', { model: 'deepseek-v4.1-flash',
          messages: [{ role: 'system', content: 'You are a helpful assistant.' }, { role: 'user', content: 'Reply with exactly LOADOUT_OK.' }],
          max_tokens: 256, stream: true });
    const raw = Buffer.from(response.body_b64, 'base64').toString('utf8');
    let payload; try { payload = JSON.parse(raw); } catch { payload = undefined; }
    const summary = { action, http_status: response.status, bytes: Buffer.byteLength(raw), content_type: response.headers?.['content-type'] };
    if (payload && typeof payload === 'object') {
      summary.application_code = payload.code;
      const data = payload.data?.data || payload.data || payload;
      if (action === 'catalog') {
        const models = data.models;
        summary.model_count = Array.isArray(models) ? models.length : 0;
        summary.target_present = Array.isArray(models) && models.some(m => m.id === 'deepseek-v4.1-flash');
      } else {
        summary.response_model = data.model;
        summary.has_choices = Array.isArray(data.choices) && data.choices.length > 0;
      }
      if (payload.code && typeof payload.msg === 'string') summary.message = payload.msg.slice(0, 160);
    } else if (action === 'smoke') {
      const events = raw.split('\n').filter(line => line.startsWith('data:'));
      const models = new Set(); let content = ''; let errors = 0;
      for (const line of events) {
        try {
          const event = JSON.parse(line.slice(5));
          if (event.model) models.add(event.model);
          if (event.error) errors++;
          for (const choice of event.choices || []) content += choice.delta?.content || '';
        } catch {}
      }
      summary.events = events.length; summary.response_models = [...models]; summary.error_events = errors;
      summary.marker_returned = content.includes('LOADOUT_OK'); summary.done = raw.includes('data: [DONE]');
    }
    console.log(JSON.stringify(summary));
  } finally { ipc.close(); }
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch(error => { console.log(JSON.stringify({ status: 'FAILED', error: error.message })); process.exitCode = 1; });
}
