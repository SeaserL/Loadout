// SPDX-License-Identifier: Apache-2.0
// Loaded before upstream modules. Defense in depth for this mock-only experiment.
import net from 'node:net';
import tls from 'node:tls';
import { syncBuiltinESMExports } from 'node:module';
const allowed = new Set(['127.0.0.1', '::1', 'localhost']);
globalThis.phase1Denied = 0;
globalThis.phase1DeniedHosts = new Set();
function check(host) {
  if (!allowed.has(String(host || 'localhost'))) {
    globalThis.phase1Denied++;
    globalThis.phase1DeniedHosts.add(String(host));
    throw new Error('PHASE1_NON_LOOPBACK_DENIED');
  }
}
const connect = net.Socket.prototype.connect;
net.Socket.prototype.connect = function (...args) {
  let a = args[0];
  if (Array.isArray(a)) a = a[0];
  if (typeof a === 'object') {
    if (a.path) throw new Error('PHASE1_SOCKET_PATH_DENIED');
    check(a.host);
  } else check(typeof args[1] === 'string' ? args[1] : 'localhost');
  return connect.apply(this, args);
};
const tlsConnect = tls.connect;
tls.connect = function (...args) {
  const a = args[0];
  check(typeof a === 'object' ? a.host : args[1]);
  return tlsConnect.apply(this, args);
};
const fetchOriginal = globalThis.fetch;
globalThis.fetch = function(input, init) {
  const url=new URL(input instanceof Request ? input.url : String(input));
  check(url.hostname);
  if(allowed.has(url.hostname))globalThis.phase1OnDispatch?.();
  return fetchOriginal(input, init);
};
syncBuiltinESMExports();
