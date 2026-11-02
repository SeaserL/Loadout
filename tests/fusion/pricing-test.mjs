// SPDX-License-Identifier: Apache-2.0
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import http from "node:http";
import cp from "node:child_process";
import { syncBuiltinESMExports } from "node:module";
import { pathToFileURL } from "node:url";
import { once } from "node:events";
import { randomUUID } from "node:crypto";
import { DatabaseSync } from "node:sqlite";
import { Instance, sleep, baseline, backup, restore, pkg, load } from "./harness.mjs";

const root=path.resolve(process.env.FUSION_RELEASE_ROOT);
const run=path.join(root,"runs/loadout-fusion-isolated/pricing-"+Date.now());fs.mkdirSync(run,{recursive:true});
const result={baseline:baseline(),cases:[]},instances=[],downloads=[],calls=[];
const sentinels=["BODY_"+randomUUID(),"REPLY_"+randomUUID(),"synthetic-"+randomUUID()];
let i,profile,mode="valid",rate=2,last,scope;
const check=async(name,fn)=>{try{await fn();result.cases.push({name,pass:true});}catch(e){result.cases.push({name,pass:false,code:e.code||e.message.slice(0,80),location:e.stack?.match(/pricing-test\.mjs:\d+/)?.[0],...(typeof e.actual==="number"?{actual:e.actual,expected:e.expected}:{})});}fs.writeFileSync(path.join(run,"result.json"),JSON.stringify(result,null,2));console.log(JSON.stringify(result.cases.at(-1)));};
const feed=()=>({
  "openai/gpt-4o":{litellm_provider:"openai",input_cost_per_token:rate/1e6,output_cost_per_token:rate*2/1e6},
  "test/priced-model":{litellm_provider:profile?.provider||"synthetic",input_cost_per_token:rate/1e6,output_cost_per_token:rate*2/1e6},
});
const server=http.createServer(async(req,res)=>{
  let text="";for await(const c of req)text+=c;
  res.setHeader("content-type","application/json");
  if(req.url==="/prices"){
    downloads.push({method:req.method,bytes:text.length,auth:Boolean(req.headers.authorization),cookie:Boolean(req.headers.cookie)});
    if(mode==="http-error"){res.statusCode=503;res.end("unavailable");return;}
    res.end(mode==="invalid-json"?"invalid synthetic json":JSON.stringify(mode==="empty"?{}:feed()));return;
  }
  if(req.method!=="POST"){res.end(JSON.stringify({data:[]}));return;}
  const body=JSON.parse(text);calls.push({model:body.model,accountAllowed:req.headers.authorization==="Bearer "+sentinels[2]});
  res.end(JSON.stringify({id:"mock",object:"chat.completion",model:body.model,choices:[{index:0,message:{role:"assistant",content:sentinels[1]},finish_reason:"stop"}],usage:{prompt_tokens:1000,completion_tokens:500,total_tokens:1500}}));
});
server.listen(0,"127.0.0.1");await once(server,"listening");
const local="http://127.0.0.1:"+server.address().port;
// Test-only transport instrumentation. No packaged file or native pricing code is changed.
const preload=path.join(run,"pricing-network.mjs"),preloadUrl=pathToFileURL(preload).href;
fs.writeFileSync(preload,`import cp from "node:child_process";
import {syncBuiltinESMExports} from "node:module";
const originalSpawn=cp.spawn;
cp.spawn=function(exe,args,options){
 if(args?.[0]==="server.js")args=["--import",import.meta.url,...args];
 return originalSpawn.call(this,exe,args,options);
};syncBuiltinESMExports();
const originalFetch=globalThis.fetch;
globalThis.fetch=(input,init)=>{
 const url=input instanceof Request?input.url:String(input);
 if(url==="https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json")
  return originalFetch(${JSON.stringify(local+"/prices")},init);
 return originalFetch(input,init);
};
`);
const realSpawn=cp.spawn;
cp.spawn=function(exe,args,options){
 if(args?.[0]===path.join(pkg,"tools/supervisor.mjs"))args=["--import",preloadUrl,...args];
 return realSpawn.call(this,exe,args,options);
};syncBuiltinESMExports();
const status=()=>i.admin("/api/pricing/sync");
const catalog=()=>i.admin("/api/pricing?includeSources=1");
const sync=(body={})=>i.admin("/api/pricing/sync","POST",body);
const raw=(p,options={})=>fetch(i.base+p,{redirect:"manual",...options});
const near=(a,b)=>assert(Math.abs(a-b)<1e-8,"COST_MISMATCH");
const ask=async(model="priced-model")=>{const r=await raw("/v1/chat/completions",{method:"POST",headers:{"content-type":"application/json",authorization:"Bearer "+profile.key},body:JSON.stringify({model:profile.model("one",model),messages:[{role:"user",content:sentinels[0]}]})});assert.equal(r.status,200);await r.json();await sleep(700);};
const restart=async()=>{await i.stop();await i.start();};
const dbEdit=fn=>{const db=new DatabaseSync(path.join(i.dir,"data/storage.sqlite"));try{fn(db);}finally{db.close();}};
const stats=()=>i.admin("/api/usage/history");
try{
 await check("pricing-config-is-boolean-only",async()=>{const {validateConfig}=await load("runtime.mjs");assert.throws(()=>validateConfig({network:"loopback-only",port:0,pricingSync:"true"}),/INVALID_PRICING_SYNC_CONFIG/);});
 i=await Instance.create(path.join(run,"one"));instances.push(i);await i.start();
 profile=await i.addProvider("pricing",local+"/v1",[{alias:"one",key:sentinels[2]}]);sentinels.push(profile.key);
 scope={connections:(await i.admin("/api/providers")).connections?.map(x=>x.id),combos:await i.admin("/api/combos")};
 await check("native-read-defaults-sources-and-catalog-without-download",async()=>{
   const p=await catalog(),defaults=await i.admin("/api/pricing/defaults");assert(Object.keys(p.pricing).length>0);assert.deepEqual(p.pricing,defaults);assert.equal(p.sourceMap.openai["gpt-4o"],"default");assert(Object.keys(await i.admin("/api/pricing/models")).length>0);
   const s=await status();assert.equal(s.enabled,false);assert.equal(s.lastSync,null);assert.equal(s.nextSync,null);assert.equal(s.intervalMs,86400000);assert.deepEqual(s.sources,["litellm"]);assert.equal(downloads.length,0);
 });
 await check("admin-only-read-and-sync-method-boundaries",async()=>{
   for(const p of ["/api/pricing","/api/pricing/defaults","/api/pricing/models","/api/pricing/sync"]){assert.notEqual((await raw(p)).status,200);assert.equal((await raw(p,{headers:{authorization:"Bearer "+profile.key}})).status,403);for(const method of ["PATCH","DELETE"])assert.equal((await raw(p,{method,headers:{cookie:i.cookie}})).status,403);}
   assert.equal((await raw("/api/pricing",{method:"POST",headers:{cookie:i.cookie}})).status,403);
 });
 await check("origin-and-cross-site-writes-rejected",async()=>{
   for(const headers of [{origin:"https://foreign.invalid"},{"sec-fetch-site":"cross-site"}])assert.equal((await raw("/api/pricing/sync",{method:"POST",headers:{cookie:i.cookie,"content-type":"application/json",...headers},body:"{}"})).status,403);assert.equal(downloads.length,0);
 });
 await check("native-sync-schema-rejects-unknown-source-and-url",async()=>{
   for(const b of [{sources:["unknown"]},{url:local},{sources:[]},{dryRun:"yes"}])await assert.rejects(sync(b),/ADMIN_400/);assert.equal(downloads.length,0);await assert.rejects(i.admin("/api/pricing?includeSources=1","POST",{}),/ADMIN_PATH/);
 });
 await check("dry-run-converts-units-without-persisting",async()=>{
   const r=await sync({sources:["litellm"],dryRun:true});assert(r.success);assert.equal(r.data[profile.provider]["priced-model"].input,2);assert.equal(r.data[profile.provider]["priced-model"].output,4);assert.equal((await status()).lastSync,null);assert(!(await catalog()).pricing[profile.provider]);
 });
 await check("manual-sync-disabled-auto-persists-native-source-and-time",async()=>{
   const begin=Date.now();assert((await sync()).success);last=await status();assert.equal(last.enabled,false);assert(Date.parse(last.lastSync)>=begin);assert(!("generatedAt" in last));const p=await catalog();assert.equal(p.sourceMap[profile.provider]["priced-model"],"litellm");assert.equal(p.pricing[profile.provider]["priced-model"].input,2);
 });
 await check("real-mock-usage-uses-synced-price",async()=>{await ask();const s=await stats();assert.equal(s.totalRequests,1);assert.equal(s.totalPromptTokens,1000);assert.equal(s.totalCompletionTokens,500);near(s.totalCost,0.004);result.firstCost=s.totalCost;});
 for(const failure of ["http-error","invalid-json"])await check(failure+"-retains-prices-and-last-success",async()=>{mode=failure;await assert.rejects(sync(),/ADMIN_502/);assert.equal((await status()).lastSync,last.lastSync);assert.equal((await catalog()).pricing[profile.provider]["priced-model"].input,2);});
 mode="valid";
 await check("same-data-sync-refreshes-local-time",async()=>{await sleep(20);await sync();const s=await status();assert(Date.parse(s.lastSync)>Date.parse(last.lastSync));last=s;});
 await check("price-change-invalidates-read-cache-and-reprices-history",async()=>{
   rate=3;await sync();assert.equal((await catalog()).pricing[profile.provider]["priced-model"].input,3);
   // Restart clears the native analytics TTL; no product cache override.
   await restart();near((await stats()).totalCost,0.006);await ask();await restart();near((await stats()).totalCost,0.012);result.repricedCost=(await stats()).totalCost;
 });
 await check("unknown-price-zero-is-not-free-proof",async()=>{await ask("unknown-price-model");await restart();const s=await stats();assert.equal(s.totalRequests,3);near(s.totalCost,0.012);assert(!(await catalog()).pricing[profile.provider]["unknown-price-model"]);});
 await check("stored-provider-cost-wins-over-current-pricing",async()=>{
   await i.stop();dbEdit(db=>db.prepare("UPDATE usage_history SET provider_cost_usd=? WHERE rowid=(SELECT min(rowid) FROM usage_history)").run(0.125));await i.start();near((await stats()).totalCost,0.131);result.storedCostFixture=true;
 });
 await check("native-layer-priority-and-model-level-source-map",async()=>{
   await i.stop();dbEdit(db=>{const put=db.prepare("INSERT OR REPLACE INTO key_value(namespace,key,value) VALUES(?,?,?)");put.run("models_dev_pricing",profile.provider,JSON.stringify({"priced-model":{input:8,output:9}}));put.run("pricing",profile.provider,JSON.stringify({"priced-model":{input:10}}));});await i.start();
   const p=await catalog();assert.equal(p.sourceMap[profile.provider]["priced-model"],"user");assert.equal(p.pricing[profile.provider]["priced-model"].input,10);assert.equal(p.pricing[profile.provider]["priced-model"].output,9);await sync();assert.equal((await catalog()).pricing[profile.provider]["priced-model"].input,10);
 });
 await check("empty-valid-feed-clears-synced-layer-native-semantics",async()=>{
   mode="empty";const r=await sync();assert.equal(r.success,true);assert.equal(r.modelCount,0);assert.equal((await status()).lastSyncModelCount,0);const p=await catalog();assert.equal(p.sourceMap.openai["gpt-4o"],"default");assert.equal(p.sourceMap[profile.provider]["priced-model"],"user");result.emptyFeedAccepted=true;
 });
 mode="valid";await sync();last=await status();
 await check("automatic-opt-in-initializes-native-timer-and-status",async()=>{
   await i.stop();i.meta.config.pricingSync=true;fs.writeFileSync(path.join(i.dir,"instance.json"),JSON.stringify(i.meta));const n=downloads.length;await i.start();
   for(let t=0;t<100;t++){if(downloads.length>n&&(await status()).lastSync!==last.lastSync)break;await sleep(100);}
   const s=await status();assert(s.enabled);assert(downloads.length>n);assert.equal(Date.parse(s.nextSync)-Date.parse(s.lastSync),86400000);last=s;
 });
 await check("disable-auto-restart-keeps-status-without-fetch",async()=>{
   await i.stop();i.meta.config.pricingSync=false;fs.writeFileSync(path.join(i.dir,"instance.json"),JSON.stringify(i.meta));const n=downloads.length;await i.start();await sleep(400);assert.equal(downloads.length,n);const s=await status();assert.equal(s.lastSync,last.lastSync);assert.equal(s.enabled,false);assert.equal(s.nextSync,null);
 });
 await check("packaged-cli-reads-native-source-map",async()=>{
   const r=await new Promise((resolve,reject)=>{const c=realSpawn(path.join(pkg,"node.exe"),[path.join(pkg,"tools/cli.mjs"),"admin",i.dir,"GET","/api/pricing?includeSources=1"],{stdio:["ignore","pipe","pipe"],windowsHide:true});let output="";c.stdout.on("data",x=>output+=x);c.stderr.resume();c.on("error",reject);c.on("close",status=>resolve({status,output}));});assert.equal(r.status,0);assert.equal(JSON.parse(r.output).sourceMap[profile.provider]["priced-model"],"user");
 });
 await check("sync-does-not-change-accounts-routes-or-send-secrets",async()=>{
   assert.deepEqual({connections:(await i.admin("/api/providers")).connections?.map(x=>x.id),combos:await i.admin("/api/combos")},scope);
   assert(downloads.length>0);assert(downloads.every(r=>r.method==="GET"&&!r.auth&&!r.cookie&&r.bytes===0));assert(calls.every(c=>c.accountAllowed&&["priced-model","unknown-price-model"].includes(c.model)));
 });
 await check("encrypted-backup-restores-prices-origin-time-and-cost",async()=>{
   const before=await catalog(),beforeStatus=await status(),beforeCost=(await stats()).totalCost;await i.stop();const password=randomUUID(),archive=path.join(run,"state.enc");await backup(i,archive,password);i=await restore(archive,path.join(run,"restored"),password);instances.push(i);await i.start();assert.deepEqual(await catalog(),before);assert.deepEqual(await status(),beforeStatus);near((await stats()).totalCost,beforeCost);
 });
 await check("normal-launcher-egress-denial-preserves-price-state",async()=>{
   await i.stop();cp.spawn=realSpawn;syncBuiltinESMExports();const n=downloads.length,before=await Promise.resolve(last);await i.start();const s=await status();await assert.rejects(sync(),/ADMIN_502/);assert.equal((await status()).lastSync,s.lastSync);assert.equal(downloads.length,n);assert((await catalog()).pricing[profile.provider]);
 });
 await check("no-credentials-or-request-body-in-instance-files",async()=>{
   for(const instance of instances){await instance.stop();const walk=d=>{for(const e of fs.readdirSync(d,{withFileTypes:true})){const p=path.join(d,e.name);if(e.isDirectory())walk(p);else{const b=fs.readFileSync(p);for(const value of sentinels){assert(!b.includes(Buffer.from(value)),"SENSITIVE_DISK");assert(!b.includes(Buffer.from(value,"utf16le")),"SENSITIVE_DISK_UTF16");}}}};walk(instance.dir);}
 });
}catch(e){result.fatal=e.code||e.message.slice(0,80);}
finally{
 cp.spawn=realSpawn;syncBuiltinESMExports();
 for(const instance of instances)await check("cleanup-"+instance.meta.id,async()=>assert.equal(await instance.cleanup(),0));
 server.closeAllConnections();await new Promise(r=>server.close(r));result.downloads=downloads.length;result.mockInferenceCalls=calls.length;result.realExternalCalls=0;result.networkInstrumentation="test-process-only fixed LiteLLM URL to loopback; package unchanged";
 fs.writeFileSync(path.join(run,"result.json"),JSON.stringify(result,null,2));fs.writeFileSync(path.join(root,"evidence/pricing-latest.json"),JSON.stringify({run,...result},null,2));
}
process.exitCode=result.fatal||result.cases.some(c=>!c.pass)?1:0;
