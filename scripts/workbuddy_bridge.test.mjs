// SPDX-License-Identifier: Apache-2.0
import test from 'node:test';
import assert from 'node:assert/strict';
import { createBridge, decodeResponse, MODEL } from './workbuddy_bridge.mjs';

const stream = model => `data: ${JSON.stringify({ id: 'synthetic', model, choices: [{ index: 0, delta: { content: 'OK' }, finish_reason: 'stop' }] })}\n\ndata: [DONE]\n\n`;
const key = 'synthetic-local-test-key-not-a-real-secret';
async function fixture(t, fetchUpstream) {
  const server = createBridge({ key, boundAccountHash: 'synthetic-account', readAccountHash: async () => 'synthetic-account', fetchUpstream });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise(resolve => { server.close(resolve); server.closeAllConnections(); }));
  const base = `http://127.0.0.1:${server.address().port}`;
  return (body, authorized = true) => fetch(base + '/v1/chat/completions', { method: 'POST', headers: { 'content-type': 'application/json', ...(authorized ? { authorization: 'Bearer ' + key } : {}) }, body: JSON.stringify(body) });
}
const body = { model: MODEL, messages: [{ role: 'user', content: 'test' }], stream: false };
test('rejects empty or different-model success rather than accepting fallback', () => {
  assert.throws(() => decodeResponse(stream('default-model')), /model_mismatch/);
  assert.throws(() => decodeResponse('data: [DONE]\n'), /incomplete_stream/);
  assert.throws(() => decodeResponse(stream(MODEL).replace('data: [DONE]', '')), /incomplete_stream/);
});
test('empty upstream function_call placeholder is not a tool invocation', () => {
  const raw = stream(MODEL).replace('"content":"OK"', '"content":"OK","function_call":{},"tool_calls":[]');
  assert.equal(decodeResponse(raw).length, 1);
});
test('authentication and unsupported model never call upstream', async t => {
  let calls = 0;
  const send = await fixture(t, async () => { calls++; throw new Error('must not call'); });
  assert.equal((await send(body, false)).status, 401);
  assert.equal((await send({ ...body, model: 'auto' })).status, 400);
  assert.equal((await send({ ...body, tools: [{ type: 'function' }] })).status, 400);
  assert.equal(calls, 0);
});
test('text result aggregates and independent calls do not acquire hidden state', async t => {
  const seen = [];
  const send = await fixture(t, async value => { seen.push(value); return { status: 200, body_b64: Buffer.from(stream(MODEL)).toString('base64') }; });
  for (let i = 0; i < 2; i++) {
    const response = await send(body); assert.equal(response.status, 200);
    assert.equal(response.headers.get('x-loadout-stream-mode'), 'buffered');
    assert.equal((await response.json()).choices[0].message.content, 'OK');
  }
  assert.equal(seen.length, 2);
  assert.ok(seen.every(value => value.model === MODEL && !('previous_response_id' in value)));
});
test('rejected concurrent requests cannot release another requests slot', async t => {
  let release; let entered;
  const ready = new Promise(resolve => { entered = resolve; });
  const send = await fixture(t, async () => { entered(); await new Promise(resolve => { release = resolve; }); return { status: 200, body_b64: Buffer.from(stream(MODEL)).toString('base64') }; });
  const first = send(body); await ready;
  assert.equal((await send(body)).status, 429);
  assert.equal((await send(body)).status, 429);
  release(); assert.equal((await first).status, 200);
});
test('different-model payload is not returned to caller', async t => {
  const send = await fixture(t, async () => ({ status: 200, body_b64: Buffer.from(stream('default-model')).toString('base64') }));
  const response = await send(body); assert.equal(response.status, 502);
  assert.equal((await response.json()).error.type, 'invalid_upstream_response');
});
test('account switching rejects requests before reaching upstream', async t => {
  let calls = 0;
  const server = createBridge({ key, boundAccountHash: 'first', readAccountHash: async () => 'second', fetchUpstream: async () => { calls++; } });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise(resolve => { server.close(resolve); server.closeAllConnections(); }));
  const response = await fetch(`http://127.0.0.1:${server.address().port}/v1/models`, { headers: { authorization: 'Bearer ' + key } });
  assert.equal(response.status, 409); assert.equal(calls, 0);
});
