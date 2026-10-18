# SPDX-License-Identifier: Apache-2.0
import tempfile
import unittest
import json
import threading
import urllib.request
import urllib.error
from http.server import ThreadingHTTPServer
from pathlib import Path
from opencode_bridge import ProtectedOfficial, make_handler
from opencode_protocol import BridgeError
from opencode_events import EventState


class Worker:
    def __init__(self): self.calls = 0; self.fail = False
    def run(self, *args, **kwargs):
        self.calls += 1
        if self.fail: raise BridgeError(429, 'official_access_restricted', 1800)
        yield 'delta', {'content': 'synthetic'}
        yield 'result', {}


class ProtectionTests(unittest.TestCase):
    def test_http_rejection_has_retry_after_without_worker_call(self):
        with tempfile.TemporaryDirectory() as root:
            worker = Worker()
            guard = ProtectedOfficial(worker, Path(root)/'admission.json', lambda: 1000)
            guard.next_at = 1120
            server = ThreadingHTTPServer(('127.0.0.1', 0), make_handler(guard, 'synthetic'))
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            try:
                for stream in (False, True):
                    body = json.dumps({'model':'deepseek-v4.1-flash-free','messages':[{'role':'user','content':'synthetic'}],'stream':stream}).encode()
                    req = urllib.request.Request(f'http://127.0.0.1:{server.server_port}/v1/chat/completions', body, headers={'Authorization':'Bearer synthetic','Content-Type':'application/json'})
                    with self.assertRaises(urllib.error.HTTPError) as caught:
                        urllib.request.urlopen(req, timeout=3)
                    with caught.exception as response:
                        self.assertEqual(response.code, 429)
                        self.assertEqual(response.headers['Retry-After'], '120')
                        self.assertEqual(json.load(response)['error']['type'], 'bridge_minimum_interval')
                self.assertEqual(worker.calls, 0)
            finally:
                server.shutdown(); server.server_close(); thread.join(3)

    def test_interval_cancellation_and_restart(self):
        with tempfile.TemporaryDirectory() as root:
            clock = [1000]; worker = Worker(); path = Path(root)/'admission.json'
            guard = ProtectedOfficial(worker, path, lambda: clock[0])
            request = guard.run([]); next(request)
            with self.assertRaises(BridgeError) as caught: list(guard.run([]))
            self.assertEqual(caught.exception.code, 'bridge_request_in_progress')
            request.close()
            guard = ProtectedOfficial(worker, path, lambda: clock[0])
            with self.assertRaises(BridgeError) as caught: list(guard.run([]))
            self.assertEqual(caught.exception.retry_after, 120)
            self.assertEqual(worker.calls, 1)
            clock[0] += 120
            list(guard.run([])); self.assertEqual(worker.calls, 2)

    def test_403_stops_next_request_and_survives_restart(self):
        with tempfile.TemporaryDirectory() as root:
            clock = [1000]; worker = Worker(); worker.fail = True; path = Path(root)/'admission.json'
            guard = ProtectedOfficial(worker, path, lambda: clock[0])
            with self.assertRaises(BridgeError): list(guard.run([]))
            clock[0] += 120
            guard = ProtectedOfficial(worker, path, lambda: clock[0]); worker.fail = False
            with self.assertRaises(BridgeError) as caught: list(guard.run([]))
            self.assertEqual(caught.exception.code, 'bridge_cooling_down')
            self.assertEqual(caught.exception.retry_after, 1680)
            self.assertEqual(worker.calls, 1)
            clock[0] = 2800; list(guard.run([])); self.assertEqual(worker.calls, 2)

    def test_bad_persistence_fails_closed_and_403_is_classified(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root)/'admission.json'; path.write_text('invalid')
            worker = Worker(); guard = ProtectedOfficial(worker, path, lambda: 1000)
            with self.assertRaises(BridgeError): list(guard.run([]))
            self.assertEqual(worker.calls, 0)
        state = EventState('s', 'deepseek-v4.1-flash-free', [])
        with self.assertRaises(BridgeError) as caught:
            state.accept({'type':'session.step.failed','data':{'sessionID':'s','error':{'status':403,'message':'sensitive upstream text'}}})
        self.assertEqual(caught.exception.code, 'official_access_restricted')
        self.assertNotIn('sensitive', str(caught.exception))

if __name__ == '__main__': unittest.main()
