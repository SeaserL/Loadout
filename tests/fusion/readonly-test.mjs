// SPDX-License-Identifier: Apache-2.0
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import http from "node:http";
import { once } from "node:events";
import { randomUUID } from "node:crypto";
import { spawnSync } from "node:child_process";
import { DatabaseSync } from "node:sqlite";
import { Instance, sleep, baseline, backup, restore, pkg } from "./harness.mjs";
const root=path.resolve(process.env.FUSION_RELEASE_ROOT);
const run=path.join(root,"runs/loadout-fusion-isolated/readonly-"+Date.now());fs.mkdirSync(run,{recursive:true});
const result={baseline:baseline(),cases:[]}, instances=[], sentinels=["BODY_"+randomUUID(),"REPLY_"+randomUUID()];
let i, profile, traceId, logId, mode="normal", calls=0, stats, routes;
const check=async(name,fn)=>{try{await fn();result.cases.push({name,pass:true});}catch(e){result.cases.push({name,pass:false,code:e.code||e.message.slice(0,70),...(typeof e.actual==="number"?{actual:e.actual}:{}),...(typeof e.expected==="number"?{expected:e.expected}:{})});}
fs.writeFileSync(path.join(run,"result.json"),JSON.stringify(result,null,2));console.log(JSON.stringify(result.cases.at(-1)));};
const server=http.createServer(async(req,res)=>{let raw="";for await(const c of req)raw+=c;calls++;
res.setHeader("content-type","application/json");if(req.method!=="POST")return res.end(JSON.stringify({data:[{id:"fixture-model"}]}));
// Exercise native error sanitization with a synthetic labeled credential echo.
const body=JSON.parse(raw);if(mode==="fail-first"&&req.headers.authorization==="Bearer "+sentinels[2]){res.statusCode=429;return res.end(JSON.stringify({error:{message:"temporary rate limit api_key="+sentinels[2]}}));}
res.end(JSON.stringify({id:"mock",object:"chat.completion",model:body.model,choices:[{index:0,message:{role:"assistant",content:sentinels[1]},finish_reason:"stop"}],usage:{prompt_tokens:11,completion_tokens:7,total_tokens:18}}));});
server.listen(0,"127.0.0.1");await once(server,"listening");
const request=async(p,options={})=>fetch(i.base+p,{redirect:"manual",...options,signal:AbortSignal.timeout(20000)});
const auth=()=>({cookie:i.cookie});
const read=async(p)=>{const r=await request(p,{headers:auth()});assert.equal(r.status,200,p);const data=await r.json();const serialized=JSON.stringify(data);for(const s of sentinels)assert(!serialized.includes(s),"SENSITIVE_READ");return data;};
const ask=async(model,key=profile.key)=>{const r=await request("/v1/chat/completions",{method:"POST",headers:{"content-type":"application/json",authorization:"Bearer "+key},body:JSON.stringify({model,messages:[{role:"user",content:sentinels[0]}]})});const body=await r.text();assert.equal(r.status,200);assert(body.includes(sentinels[1]));return r;};
try{
 i=await Instance.create(path.join(run,"one"));instances.push(i);await i.start();
 sentinels.push("synthetic-a-"+randomUUID(),"synthetic-b-"+randomUUID());
 profile=await i.addProvider("readonly","http://127.0.0.1:"+server.address().port+"/v1",[{alias:"a",key:sentinels[2]},{alias:"b",key:sentinels[3]}]);sentinels.push(profile.key);
 await i.admin("/api/combos","POST",{name:"readonly-pool",strategy:"priority",models:profile.connections.map(id=>({kind:"model",provider:profile.provider,model:"fixture-model",connectionId:id}))});
 // Seed a native stored model fixture offline; no new catalog mutation API.
 await i.stop();const db=new DatabaseSync(path.join(i.dir,"data/storage.sqlite"));db.prepare("INSERT OR REPLACE INTO key_value(namespace,key,value) VALUES('customModels',?,?)").run(profile.provider,JSON.stringify([{id:"fixture-model",name:"Synthetic model",source:"manual"}]));db.close();await i.start();
 routes=["/api/models?all=true","/api/provider-models?provider="+profile.provider,"/api/usage/history","/api/usage/model-latency-stats?minSamples=1","/api/usage/call-logs?limit=20","/api/usage/route-explain/missing","/api/usage/combo-trace/combo-missing"];
 await check("all-read-routes-require-admin-session",async()=>{for(const p of routes){assert.notEqual((await request(p)).status,200);assert.equal((await request(p,{headers:{authorization:"Bearer "+profile.key}})).status,403);}});
 await check("readonly-methods-and-neighbor-routes-denied",async()=>{for(const p of routes)for(const method of ["POST","PUT","PATCH","DELETE"]){assert.equal((await request(p,{method,headers:{...auth(),"x-csrf-token":i.csrf}})).status,403);}for(const p of ["/api/provider-models/validate-and-add","/api/usage/call-logs/unknown","/api/usage/analytics","/api/usage/request-logs"])assert.equal((await request(p,{headers:auth()})).status,404);});
 await check("native-head-and-invalid-session",async()=>{const h=await request("/api/provider-models",{method:"HEAD",headers:auth()});assert.equal(h.status,200);assert.equal(await h.text(),"");assert.notEqual((await request("/api/usage/history",{headers:{cookie:"auth_token=invalid"}})).status,200);});
 await check("readonly-origin-and-host-protection",async()=>{
  assert.equal((await request("/api/usage/history",{headers:{...auth(),origin:"http://foreign.invalid"}})).status,403);
  // Node fetch replaces Host with the URL authority; raw HTTP preserves it.
  const status=await new Promise((resolve,reject)=>{
   const req=http.get(i.base+"/api/usage/history",{headers:{...auth(),host:"foreign.invalid"}},res=>{res.resume();res.on("end",()=>resolve(res.statusCode));});
   req.setTimeout(20000,()=>req.destroy(new Error("HOST_TEST_TIMEOUT")));req.on("error",reject);
  });
  assert.equal(status,403);
 });
 await check("native-static-and-custom-catalog-no-upstream-call",async()=>{const n=calls;const catalog=await read(routes[0]);assert(catalog.models.length>0);const custom=await read(routes[1]);assert(custom.models.some(m=>m.id==="fixture-model"));assert.equal(calls,n);});
 await check("empty-stats-are-empty-not-fabricated",async()=>{const h=await read("/api/usage/history");assert.equal(h.totalRequests,0);assert.equal((await read(routes[3])).entries.length,0);});
 await check("real-mock-request-records-native-usage-and-trace",async()=>{const r=await ask("readonly-pool");traceId=r.headers.get("x-omniroute-combo-trace");assert(traceId?.startsWith("combo-"));await sleep(2500);for(let n=0;n<70;n++){stats=await read("/api/usage/history");if(stats.totalRequests===1)break;await sleep(1000);}assert.equal(stats.totalRequests,1);assert.equal(stats.totalPromptTokens,11);assert.equal(stats.totalCompletionTokens,7);assert(Number.isFinite(stats.totalCost));result.usage={requests:stats.totalRequests,input:stats.totalPromptTokens,output:stats.totalCompletionTokens,cost:stats.totalCost};});
 await check("native-latency-and-call-index",async()=>{const l=await read(routes[3]);assert(l.entries.some(e=>JSON.stringify(e).includes("fixture-model")));const logs=await read(routes[4]);const row=logs.find(r=>r.status===200&&r.model==="fixture-model"&&!r.active&&!r.completed);assert(row);logId=row.id;assert.equal(row.tokens.in,11);assert.equal(row.tokens.out,7);result.callMetadata={id:row.id,connectionId:row.connectionId};});
 await check("route-explanation-retains-recompute-limitations",async()=>{const e=await read("/api/usage/route-explain/"+logId);assert.equal(e.modelUsed,"fixture-model");assert.equal(e.selectedTarget.connectionId,profile.connections[0]);assert(Array.isArray(e.limitations));assert(e.decisionReplay);result.explanation={confidence:e.confidence,limitations:e.limitations};});
 await check("actual-combo-trace-and-missing-id-semantics",async()=>{const t=await read("/api/usage/combo-trace/"+traceId);assert(t.decisions.some(d=>d.decision==="dispatched"));assert.equal((await request("/api/usage/combo-trace/combo-missing",{headers:auth()})).status,404);assert.equal((await request("/api/usage/combo-trace/not-combo",{headers:auth()})).status,400);assert.equal((await request("/api/usage/route-explain/missing",{headers:auth()})).status,404);});
 await check("failed-first-route-diagnostics-keep-account-boundary",async()=>{mode="fail-first";const r=await ask("readonly-pool");const t=await read("/api/usage/combo-trace/"+r.headers.get("x-omniroute-combo-trace"));assert(t.decisions.filter(d=>d.decision==="dispatched").length===2);await sleep(500);const logs=await read(routes[4]);assert(logs.filter(x=>x.model==="fixture-model").every(x=>profile.connections.includes(x.connectionId)));mode="normal";});
 await check("native-client-catalog-restricts-combos",async()=>{const key=await i.admin("/api/keys","POST",{name:"restricted",noLog:true,allowedConnections:[profile.connections[1]],modelAccessMode:"restricted",allowedModels:[],allowedCombos:["readonly-pool"]});sentinels.push(key.key);const r=await request("/v1/models",{headers:{authorization:"Bearer "+key.key}});assert.equal(r.status,200);const data=await r.json();assert(data.data.some(m=>m.id==="readonly-pool"));assert(data.data.every(m=>m.id==="readonly-pool"));});
 await check("cli-reuses-native-get",async()=>{
  for(const p of [routes[1],routes[3],routes[4],"/api/usage/history"]){
   const r=spawnSync(path.join(pkg,"node.exe"),[path.join(pkg,"tools/cli.mjs"),"admin",i.dir,"GET",p],{encoding:"utf8",windowsHide:true,timeout:30000});
   assert.equal(r.status,0);const data=JSON.parse(r.stdout);if(p===routes[1])assert(data.models.some(m=>m.id==="fixture-model"));
   if(p==="/api/usage/history")assert(data.totalRequests>=1);
  }
  await assert.rejects(i.admin("/api/providers?unexpected=true"),/ADMIN_PATH/);
  await assert.rejects(i.admin("/api/provider-models?provider=fixture","POST",{}),/ADMIN_PATH/);
 });
 await check("read-query-does-not-dispatch-and-bad-filter-rejected",async()=>{const n=calls;await read(routes[0]);await read(routes[1]);await read(routes[2]);await read(routes[3]);await read(routes[4]);await read("/api/usage/route-explain/"+logId);await read("/api/usage/combo-trace/"+traceId);assert.equal((await request("/api/usage/model-latency-stats?windowHours=-1",{headers:auth()})).status,400);assert.equal(calls,n);});
 await check("restart-persists-metadata-but-expires-memory-trace",async()=>{await i.stop();await i.start();const h=await read(routes[2]);assert(h.totalRequests>=2);await read("/api/usage/route-explain/"+logId);assert.equal((await request("/api/usage/combo-trace/"+traceId,{headers:auth()})).status,404);});
 await check("native-stored-cost-fixture-aggregates-without-new-pricing-layer",async()=>{await i.stop();const db=new DatabaseSync(path.join(i.dir,"data/storage.sqlite"));try{db.prepare("UPDATE usage_history SET provider_cost_usd=0 WHERE model=?").run("fixture-model");db.prepare("UPDATE usage_history SET provider_cost_usd=0.125 WHERE rowid=(SELECT rowid FROM usage_history WHERE model=? LIMIT 1)").run("fixture-model");}finally{db.close();}await i.start();const h=await read(routes[2]);assert.equal(h.totalCost,0.125);result.costFixture={storedCost:0.125,observedTotal:h.totalCost,synthetic:true};});
 await check("encrypted-backup-restore-preserves-read-model",async()=>{await i.stop();const archive=path.join(run,"state.enc"),password=randomUUID();await backup(i,archive,password);i=await restore(archive,path.join(run,"restored"),password);instances.push(i);await i.start();assert((await read(routes[2])).totalRequests>=2);await read("/api/usage/route-explain/"+logId);assert((await read(routes[1])).models.some(m=>m.id==="fixture-model"));});
 await check("no-credentials-or-bodies-in-instance-files",async()=>{for(const instance of instances){await instance.stop();const walk=d=>{for(const e of fs.readdirSync(d,{withFileTypes:true})){const p=path.join(d,e.name);if(e.isDirectory())walk(p);else{const b=fs.readFileSync(p);for(const s of sentinels){assert(!b.includes(Buffer.from(s)),"SENSITIVE_DISK");assert(!b.includes(Buffer.from(s,"utf16le")),"SENSITIVE_DISK_UTF16");}}}};walk(instance.dir);}});
}catch(e){result.fatal=e.code||e.message.slice(0,80);}
finally{for(const instance of instances)await check("cleanup-"+instance.meta.id,async()=>assert.equal(await instance.cleanup(),0));server.closeAllConnections();await new Promise(r=>server.close(r));fs.writeFileSync(path.join(run,"result.json"),JSON.stringify(result,null,2));fs.writeFileSync(path.join(root,"evidence/readonly-latest.json"),JSON.stringify({run,...result},null,2));}
process.exitCode=result.fatal||result.cases.some(c=>!c.pass)?1:0;
