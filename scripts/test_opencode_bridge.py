# SPDX-License-Identifier: Apache-2.0
import json
import threading
import unittest
import urllib.request
import urllib.error
from unittest.mock import patch
from http.server import ThreadingHTTPServer
from opencode_bridge import Official, OfficialPool, make_handler
from opencode_protocol import BridgeError, parse_request, parse_tools, native_history
from opencode_events import EventState

MODEL='deepseek-v4.1-flash-free'
TOOLS=[{'type':'function','function':{'name':'lookup','description':'Lookup','parameters':{'type':'object','properties':{'city':{'type':'string'}},'required':['city']}}}]
CALL={'id':'call_external','type':'function','function':{'name':'lookup','arguments':'{"city":"Sydney"}'}}
HISTORY=[{'role':'user','content':'lookup'}, {'role':'assistant','content':None,'tool_calls':[CALL]}, {'role':'tool','tool_call_id':'call_external','content':'RESULT'}]

def event(typ,**fields):
    return {'type':'session.'+typ,'data':dict(sessionID='ses_owned',assistantMessageID='msg_owned',**fields)}

def state(tools=None):
    s=EventState('ses_owned',MODEL,tools or [])
    s.accept(event('step.started',model={'id':MODEL,'providerID':'opencode'}))
    return s

class Tests(unittest.TestCase):
    def test_consecutive_user_preserves_order_and_boundaries(self):
        messages=[{'role':'user','content':'first\nmessage'}, {'role':'user','content':'second'},
                  {'role':'assistant','content':'ack'}, {'role':'user','content':'third'}, {'role':'user','content':'fourth'}]
        parsed=parse_request({'model':MODEL,'messages':messages})
        self.assertEqual(parsed,messages)
        history=native_history(parsed,MODEL)
        self.assertEqual([m['type'] for m in history],['user','user','assistant','user','user'])
        self.assertEqual([m['text'] for m in history if m['type']=='user'],['first\nmessage','second','third','fourth'])

    def test_pool_capacity_cancellation_and_reuse(self):
        class Worker:
            healthy=True
            def run(self,*args):
                yield 'delta',id(self)
        workers=[Worker(),Worker()];pool=OfficialPool(workers,waiting=0)
        a=pool.run([]);b=pool.run([])
        self.assertNotEqual(next(a),next(b))
        with self.assertRaises(BridgeError) as error:next(pool.run([]))
        self.assertEqual(error.exception.code,'bridge_queue_full')
        a.close()
        with self.assertRaises(BridgeError) as error:next(pool.run([],cancelled=lambda:True))
        self.assertEqual(error.exception.code,'client_disconnected')
        c=pool.run([]);next(c);c.close();b.close()
        self.assertEqual(pool.available.qsize(),2)

    def test_pool_wait_timeout_and_unhealthy_worker(self):
        class Worker:
            healthy=True
            def run(self,*args):yield 'delta',{}
        worker=Worker();pool=OfficialPool([worker],waiting=1,wait_seconds=.01)
        active=pool.run([]);next(active)
        with self.assertRaises(BridgeError) as error:next(pool.run([]))
        self.assertEqual(error.exception.code,'bridge_queue_timeout')
        active.close();worker.healthy=False
        with self.assertRaises(BridgeError) as error:next(pool.run([]))
        self.assertEqual(error.exception.code,'session_cleanup_failed_restart_required')

    def test_waiting_cancel_does_not_release_active_worker(self):
        class Worker:
            healthy=True
            def run(self,*args):yield 'delta',{}
        pool=OfficialPool([Worker()],waiting=1);active=pool.run([]);next(active)
        entered=threading.Event();cancel=threading.Event();errors=[]
        def cancelled():entered.set();return cancel.is_set()
        def waiting():
            try:list(pool.run([],cancelled=cancelled))
            except BridgeError as error:errors.append(error.code)
        thread=threading.Thread(target=waiting);thread.start()
        self.assertTrue(entered.wait(2))
        with self.assertRaises(BridgeError) as error:next(pool.run([]))
        self.assertEqual(error.exception.code,'bridge_queue_full')
        cancel.set();thread.join(2);self.assertFalse(thread.is_alive())
        self.assertEqual(errors,['client_disconnected']);self.assertEqual(pool.available.qsize(),0)
        active.close();self.assertEqual(pool.available.qsize(),1)

    def test_pool_control_and_catalog_are_worker_scoped(self):
        workers=[Official(1,'synthetic'),Official(2,'synthetic')]
        for i,w in enumerate(workers):
            w.control_session='ses_'+str(i);w.controls={'max_tokens':16+i}
            w.tools=[{'function':{'name':'tool_'+str(i),'parameters':{'type':'object'}}}]
        server=ThreadingHTTPServer(('127.0.0.1',0),make_handler(OfficialPool(workers),'client'))
        thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
        base='http://127.0.0.1:'+str(server.server_port)
        try:
            for i,w in enumerate(workers):
                req=urllib.request.Request(base+'/internal/control/'+w.control_session,headers={'Authorization':w.auth})
                with urllib.request.urlopen(req) as response:self.assertEqual(json.load(response),w.controls)
                req=urllib.request.Request(base+'/internal/mcp',json.dumps({'id':1,'method':'tools/list'}).encode(),headers={'Authorization':'Bearer '+w.mcp_key})
                with urllib.request.urlopen(req) as response:self.assertEqual(json.load(response)['result']['tools'][0]['name'],'tool_'+str(i))
            req=urllib.request.Request(base+'/internal/control/ses_1',json.dumps(workers[0].controls).encode(),headers={'Authorization':workers[1].auth})
            with self.assertRaises(urllib.error.HTTPError):urllib.request.urlopen(req)
            self.assertFalse(any(w.controls_applied for w in workers))
        finally:server.shutdown();server.server_close();thread.join()

    def test_text_and_tool_history(self):
        self.assertEqual(parse_request({'model':MODEL,'messages':HISTORY,'tools':TOOLS}),HISTORY)
        native=native_history(HISTORY,MODEL)
        self.assertEqual(len(native),2)
        part=native[1]['content'][0]
        self.assertEqual(part['id'],CALL['id'])
        self.assertEqual(part['name'],'loadout_lookup')
        self.assertEqual(part['state']['content'][0]['text'],'RESULT')
        self.assertIn('completed',native[1]['time'])
        self.assertEqual(native[1]['finish'],'tool-calls')

    def test_invalid_tool_history_rejected(self):
        for messages in [HISTORY[:-1], HISTORY+[HISTORY[-1]], [HISTORY[0],HISTORY[-1]],
                         HISTORY[:2]+[dict(HISTORY[-1],tool_call_id='other')],
                         [HISTORY[0],dict(HISTORY[1],tool_calls=[CALL,CALL]),HISTORY[-1]]]:
            with self.subTest(messages=messages),self.assertRaises(BridgeError):
                parse_request({'model':MODEL,'messages':messages})

    def test_unsupported_semantics(self):
        base={'model':MODEL,'messages':[{'role':'user','content':'hello'}]}
        for change in [{'max_tokens':0},{'tools':[{'type':'web_search'}]}, {'tool_choice':'required'},
                       {'stream':'yes'},{'messages':[{'role':'user','content':[]}]},
                       {'tools':[{'type':'function','function':dict(TOOLS[0]['function'],strict=True)}]},
                       {'tools':TOOLS,'model':'big-pickle'}, {'messages':HISTORY,'model':'big-pickle'}]:
            with self.subTest(change=change),self.assertRaises(BridgeError): parse_request(dict(base,**change))
        self.assertEqual(parse_tools(dict(base,tools=TOOLS,tool_choice='none')),[])

    def test_history_limits(self):
        history=[{'role':'system','content':'brief'}]+[{'role': 'user' if i%2==0 else 'assistant','content':'x'} for i in range(63)]
        self.assertEqual(len(parse_request({'model':MODEL,'messages':history})),64)
        with self.assertRaises(BridgeError): parse_request({'model':MODEL,'messages':history+[{'role':'assistant','content':'x'},{'role':'user','content':'y'}]})

    def test_control_parameters(self):
        from opencode_protocol import request_controls
        base={'model':MODEL,'messages':[{'role':'user','content':'hello'}]}
        for field in ('max_tokens','max_completion_tokens'):
            for value in (1,16,32768):
                body=dict(base,**{field:value},store=False)
                parse_request(body)
                self.assertEqual(request_controls(body),{'max_tokens':value})
            for value in (0,-1,True,None,1.5,'16',32769):
                with self.assertRaises(BridgeError):parse_request(dict(base,**{field:value}))
        for body in (dict(base,max_tokens=1,max_completion_tokens=1),dict(base,store=True)):
            with self.assertRaises(BridgeError):parse_request(body)
        for choice in ('auto','none','required',{'type':'function','function':{'name':'lookup'}}):
            parse_request(dict(base,tools=TOOLS,tool_choice=choice))
        for choice in ({'type':'function','function':{'name':'missing'}},{'type':'function','function':None},[]):
            with self.assertRaises(BridgeError):parse_request(dict(base,tools=TOOLS,tool_choice=choice))

    def test_reasoning_only_length_is_valid(self):
        s=state();s.accept(event('step.ended',finish='length'))
        self.assertEqual(s.result(),{'message':{'role':'assistant','content':None},'finish_reason':'length'})
        s=state()
        with self.assertRaises(BridgeError):s.accept(event('step.ended',finish='stop'))

    def test_empty_reasoning_placeholder_only(self):
        for value in (None, ''):
            history=[HISTORY[0],dict(HISTORY[1],reasoning_content=value),HISTORY[2]]
            parse_request({'model':MODEL,'messages':history})
        with self.assertRaises(BridgeError):
            parse_request({'model':MODEL,'messages':[HISTORY[0],dict(HISTORY[1],reasoning_content='not empty'),HISTORY[2]]})

    def test_controls_auth_session_match_and_one_attempt(self):
        api=Official(1,'synthetic-official')
        api.control_session='ses_owned';api.controls={'max_tokens':16}
        server=ThreadingHTTPServer(('127.0.0.1',0),make_handler(api,'synthetic-client'))
        thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
        base='http://127.0.0.1:'+str(server.server_port)+'/internal/control/'
        def request(session,auth,body=None):
            req=urllib.request.Request(base+session,json.dumps(body).encode() if body is not None else None,headers={'Authorization':auth})
            with urllib.request.urlopen(req,timeout=3) as res:return json.load(res)
        try:
            for session,auth,status in [('ses_owned','Bearer synthetic-client',401),('ses_other',api.auth,404)]:
                with self.assertRaises(urllib.error.HTTPError) as err:request(session,auth)
                self.assertEqual(err.exception.code,status)
            self.assertEqual(request('ses_owned',api.auth),{'max_tokens':16})
            with self.assertRaises(urllib.error.HTTPError):request('ses_owned',api.auth,{'max_tokens':17})
            self.assertFalse(api.controls_applied)
            self.assertTrue(request('ses_owned',api.auth,{'max_tokens':16})['ok'])
            with self.assertRaises(urllib.error.HTTPError) as err:request('ses_owned',api.auth,{'max_tokens':16})
            self.assertEqual(err.exception.code,409)
        finally:server.shutdown();server.server_close();thread.join()

    def test_real_deltas_and_final_integrity(self):
        s=state();s.accept(event('text.started',ordinal=0))
        self.assertEqual(s.accept(event('text.delta',ordinal=0,delta='Hello ')),[{'content':'Hello '}])
        self.assertFalse(s.done)
        s.accept(event('text.delta',ordinal=0,delta='世界'))
        s.accept(event('text.ended',ordinal=0,text='Hello 世界'))
        s.accept(event('step.ended',finish='stop'))
        self.assertEqual(s.result()['message']['content'],'Hello 世界')

    def test_lost_delta_no_false_success(self):
        s=state();s.accept(event('text.started',ordinal=0))
        s.accept(event('text.delta',ordinal=0,delta='part'))
        with self.assertRaises(BridgeError): s.accept(event('text.ended',ordinal=0,text='part missing'))
        self.assertFalse(s.done)

    def test_wrong_model_and_cross_session(self):
        s=EventState('ses_owned',MODEL,[])
        self.assertEqual(s.accept({'type':'session.step.failed','data':{'sessionID':'other'}}),[])
        with self.assertRaises(BridgeError):s.accept(event('step.started',model={'id':'big-pickle','providerID':'opencode'}))

    def test_tool_id_and_arguments_preserved(self):
        s=state(TOOLS)
        chunk=s.accept(event('tool.input.started',id=CALL['id'],name='loadout_lookup'))[0]
        self.assertEqual(chunk['tool_calls'][0]['id'],CALL['id'])
        s.accept(event('tool.input.ended',id=CALL['id'],text=CALL['function']['arguments']))
        s.accept(event('step.streamed'))
        self.assertEqual(s.result()['message']['tool_calls'],[CALL])
        self.assertEqual(s.result()['finish_reason'],'tool_calls')

    def test_unknown_tool_and_incomplete_arguments(self):
        with self.assertRaises(BridgeError): state(TOOLS).accept(event('tool.input.started',id='a',name='shell'))
        s=state(TOOLS);s.accept(event('tool.input.started',id='a',name='loadout_lookup'))
        with self.assertRaises(BridgeError): s.accept(event('step.streamed'))
        with self.assertRaises(BridgeError): s.accept(event('tool.input.ended',id='a',text='{'))

    def test_parallel_false_rejected(self):
        s=state(TOOLS);s.parallel=False
        s.accept(event('tool.input.started',id='a',name='loadout_lookup'))
        with self.assertRaises(BridgeError):s.accept(event('tool.input.started',id='b',name='loadout_lookup'))

    def test_text_before_tool_must_be_complete(self):
        s=state(TOOLS);s.accept(event('text.started',ordinal=0))
        s.accept(event('text.delta',ordinal=0,delta='Using tool'))
        s.accept(event('tool.input.started',id='a',name='loadout_lookup'))
        s.accept(event('tool.input.ended',id='a',text='{}'))
        with self.assertRaises(BridgeError):s.accept(event('step.streamed'))

    def test_mcp_never_executes_client_tool(self):
        api=Official(1,'synthetic')
        server=ThreadingHTTPServer(('127.0.0.1',0),make_handler(api,'client-key'))
        thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
        try:
            url='http://127.0.0.1:'+str(server.server_port)+'/internal/mcp'
            body=json.dumps({'jsonrpc':'2.0','id':1,'method':'tools/call','params':{'name':'arbitrary'}}).encode()
            with self.assertRaises(urllib.error.HTTPError):
                urllib.request.urlopen(urllib.request.Request(url,body,headers={'Authorization':'Bearer client-key'}))
            with urllib.request.urlopen(urllib.request.Request(url,body,headers={'Authorization':'Bearer '+api.mcp_key})) as res:
                self.assertTrue(json.load(res)['result']['isError'])
            self.assertFalse(api.healthy)
        finally:server.shutdown();server.server_close();thread.join()

    def test_cleanup_and_cancellation(self):
        api=Official(1,'synthetic');calls=[]
        def call(method,path,body=None):
            calls.append((method,path))
            if path=='/api/session':return {'data':{'id':'ses_owned'}}
        api.call=call
        with patch('opencode_bridge.EventFeed') as feed:
            with self.assertRaises(BridgeError) as error:
                list(api.run([{'role':'user','content':'hello'}],cancelled=lambda:True))
            self.assertEqual(error.exception.code,'client_disconnected')
            feed.return_value.close.assert_called_once()
        self.assertIn(('POST','/api/session/ses_owned/interrupt'),calls)
        self.assertIn(('DELETE','/api/session/ses_owned'),calls)
        self.assertFalse(any('reply' in p for _,p in calls))
        self.assertFalse(api.lock.locked())

    def test_cleanup_failure_blocks_and_no_result(self):
        api=Official(1,'synthetic')
        def call(method,path,body=None):
            if path=='/api/session':return {'data':{'id':'ses_owned'}}
            if path.endswith('/interrupt'):raise BridgeError(503,'offline')
        api.call=call
        with patch('opencode_bridge.EventFeed'):
            with self.assertRaises(BridgeError):list(api.run([{'role':'user','content':'x'}],cancelled=lambda:True))
        self.assertFalse(api.healthy)
        with self.assertRaises(BridgeError) as error:list(api.run([{'role':'user','content':'x'}]))
        self.assertEqual(error.exception.code,'session_cleanup_failed_restart_required')

    def test_http_stream_flush_before_completion_and_error(self):
        release=threading.Event();finished=threading.Event()
        class Mock:
            healthy=True
            mcp_key='private'
            fail=False
            def run(self,*args):
                try:
                    yield 'delta',{'content':'FIRST'}
                    if not release.wait(3):raise RuntimeError('client did not receive first chunk')
                    if self.fail:raise BridgeError(502,'official_event_stream_lost')
                    yield 'result',{'message':{'role':'assistant','content':'FIRST'},'finish_reason':'stop'}
                finally:finished.set()
        mock=Mock();server=ThreadingHTTPServer(('127.0.0.1',0),make_handler(mock,'synthetic'))
        thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
        try:
            base='http://127.0.0.1:'+str(server.server_port)
            with self.assertRaises(urllib.error.HTTPError) as error:urllib.request.urlopen(base+'/v1/models')
            self.assertEqual(error.exception.code,401)
            for fail in (False,True):
                mock.fail=fail;release.clear();finished.clear()
                body={'model':MODEL,'messages':[{'role':'user','content':'hi'}],'stream':True}
                req=urllib.request.Request(base+'/v1/chat/completions',json.dumps(body).encode(),headers={'Authorization':'Bearer synthetic'})
                with urllib.request.urlopen(req,timeout=5) as res:
                    first=res.readline().decode();self.assertIn('FIRST',first);self.assertFalse(finished.is_set())
                    release.set();rest=res.read().decode()
                if fail:
                    self.assertIn('official_event_stream_lost',rest);self.assertNotIn('[DONE]',rest);self.assertNotIn('"finish_reason": "stop"',rest)
                else:self.assertIn('[DONE]',rest)
                self.assertTrue(finished.wait(1))
        finally:
            release.set();server.shutdown();server.server_close();thread.join()

if __name__=='__main__':unittest.main()
