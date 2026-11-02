// SPDX-License-Identifier: Apache-2.0
import fs from "node:fs";
import path from "node:path";
import http from "node:http";
import assert from "node:assert/strict";
import { once } from "node:events";
import { pathToFileURL } from "node:url";
import { randomUUID } from "node:crypto";
import { DatabaseSync } from "node:sqlite";
const root=path.resolve(process.env.FUSION_RELEASE_ROOT), read=p=>JSON.parse(fs.readFileSync(p,"utf8"));
const release=read(path.join(root,"runs/release-latest.json"));
const old=read(path.join(root,"runs/old-baseline.json"));
const load=(base,name)=>import(pathToFileURL(path.join(base,"tools",name)));
const prior=await load(old.directory,"runtime.mjs"), previous=await load(old.directory,"backup.mjs");
const current=await load(release.extracted,"runtime.mjs"), upgraded=await load(release.extracted,"backup.mjs");
const run=path.join(root,"runs/loadout-fusion-isolated/upgrade-"+Date.now());fs.mkdirSync(run,{recursive:true});
const cases=[],instances=[];let before,after,key,keyId,connectionId,stage,provider;
const check=async(name,fn)=>{stage=name;await fn();cases.push({name,pass:true});console.log(name+" PASS");};
const mock=http.createServer(async(req,res)=>{let raw="";for await(const c of req)raw+=c;
if(req.method!=="POST"||!raw){res.writeHead(200,{"content-type":"application/json"});res.end(JSON.stringify({object:"list",data:[]}));return;}
const body=JSON.parse(raw);res.setHeader("content-type","application/json");res.end(JSON.stringify({id:"mock",object:"chat.completion",model:body.model,
choices:[{index:0,message:{role:"assistant",content:"upgrade-ok"},finish_reason:"stop"}],usage:{prompt_tokens:11,completion_tokens:7,total_tokens:18}}));});
mock.listen(0,"127.0.0.1");await once(mock,"listening");
const ask=async(i)=>{await i.attach();const r=await fetch(i.base+"/v1/chat/completions",{method:"POST",headers:{"content-type":"application/json",authorization:"Bearer "+key},
body:JSON.stringify({model:"upgrade-pool",messages:[{role:"user",content:"synthetic"}]})});assert.equal(r.status,200);assert.ok((await r.text()).includes("upgrade-ok"));await new Promise(r=>setTimeout(r,1000));};
let failure;
try{
 await check("previous-package-state-and-encrypted-backup",async()=>{
 before=await prior.Instance.create(path.join(run,"old"));instances.push(before);await before.start();
 const profile=await before.addProvider("upgrade","http://127.0.0.1:"+mock.address().port+"/v1",[{alias:"a",key:"synthetic-"+randomUUID()}]);
 provider=profile.provider;connectionId=profile.connections[0];
 await before.admin("/api/combos","POST",{name:"upgrade-pool",strategy:"priority",models:[{kind:"model",provider:profile.provider,model:"same-model",connectionId:profile.connections[0]}]});
 const client=await before.admin("/api/keys","POST",{name:"upgrade",noLog:true,allowedConnections:profile.connections,modelAccessMode:"restricted",allowedModels:[],allowedCombos:["upgrade-pool"]});key=client.key;keyId=client.id;
 await before.admin("/api/keys/"+keyId,"PATCH",{usageLimitEnabled:true,dailyUsageLimitUsd:10,weeklyUsageLimitUsd:50});
 await ask(before);await before.stop();
 const db=new DatabaseSync(path.join(before.dir,"data/storage.sqlite"),{readOnly:true});
 try{
  assert.equal(db.prepare("SELECT count(*) AS n FROM usage_history").get().n,1);
  // Offline public metadata fixture: tests existing native table preservation, not feed acceptance.
 }finally{db.close();}
 const fixture=JSON.parse(fs.readFileSync(path.join(process.env.FUSION_UPSTREAM,"tests/fixtures/radar-feed-canonical.json")));
 const seed=new DatabaseSync(path.join(before.dir,"data/storage.sqlite"));
 try{seed.prepare("INSERT INTO radar_feed_cache(id,version,generated_at,tier,payload,signature,fetched_at) VALUES(1,?,?,?,?,?,?)")
  .run(fixture.version,fixture.generatedAt,fixture.tier,JSON.stringify(fixture),"synthetic-migration-fixture","2026-10-30T00:00:00Z");
  const put=seed.prepare("INSERT OR REPLACE INTO key_value(namespace,key,value) VALUES(?,?,?)");
  put.run("pricing_synced",provider,JSON.stringify({"same-model":{input:2,output:4}}));
  put.run("pricing_sync_status","last_sync",JSON.stringify({lastSyncTime:"2026-10-30T00:00:00Z",lastSyncModelCount:1}));
  put.run("syncedAvailableModels",provider+":"+connectionId,JSON.stringify([{id:"same-model",name:"same-model",source:"api"}]));
  put.run("providerLimitsCache",connectionId,JSON.stringify({quotas:{daily:{remaining:8}},plan:"synthetic",message:null,source:"manual",fetchedAt:"2020-01-01T00:00:00Z"}));
  seed.prepare("INSERT OR REPLACE INTO domain_budgets(api_key_id,daily_limit_usd,weekly_limit_usd,monthly_limit_usd,warning_threshold,reset_interval,reset_time) VALUES(?,?,?,?,?,?,?)").run(keyId,10,50,200,0.8,"daily","00:00");
 }finally{seed.close();}
 });
 const password=randomUUID(),archive=path.join(run,"before.enc");await previous.backup(before,archive,password);
 await check("current-upgrade-keeps-keys-native-state-and-routing",async()=>{
 after=await upgraded.upgrade(archive,path.join(run,"new"),password,prior.manifest());instances.push(after);await after.start();await ask(after);
 assert.deepEqual(await after.admin("/api/loadout/subscription","POST",{operation:"list"}),[]);
 const stats=await after.admin("/api/usage/history");assert.equal(stats.totalRequests,2);assert.equal(stats.totalPromptTokens,22);assert.equal(stats.totalCompletionTokens,14);
 const logs=await after.admin("/api/usage/call-logs?limit=10");assert(logs.length>=2);
 const radar=await after.admin("/api/radar/status");assert.equal(radar.settings.optIn,false);assert.equal(radar.feeds.catalog.fetchedAt,"2026-10-30T00:00:00Z");
 assert.equal((await after.admin("/api/radar/catalog")).meta.generatedAt,radar.feeds.catalog.generatedAt);
 const prices=await after.admin("/api/pricing?includeSources=1");assert.equal(prices.sourceMap[provider]["same-model"],"litellm");assert.equal(prices.pricing[provider]["same-model"].output,4);
 assert.equal((await after.admin("/api/pricing/sync")).lastSync,"2026-10-30T00:00:00Z");assert(Math.abs(stats.totalCost-0.0001)<1e-8);
 const limits=await after.admin("/api/keys/"+keyId+"/usage-limits");assert.equal(limits.key.usageLimitEnabled,true);assert.equal(limits.key.weeklyUsageLimitUsd,50);
 assert.equal((await after.admin("/api/usage/budget?apiKeyId="+keyId)).dailyLimitUsd,10);
 assert((await after.admin("/api/synced-available-models?provider="+provider)).models.some(m=>m.id==="same-model"));
 assert.equal((await after.admin("/api/usage/provider-limits")).caches[connectionId].fetchedAt,"2020-01-01T00:00:00Z");
 await after.stop();});
 await check("rollback-reopens-previous-package-without-downgrading-database",async()=>{await before.start();await ask(before);await before.stop();});
 await check("unknown-upgrade-pair-refused",async()=>{await assert.rejects(upgraded.upgrade(archive,path.join(run,"denied"),password,{...prior.manifest(),adapterProfile:"unknown"}),/UPGRADE_PAIR_UNSUPPORTED/);});
}catch(e){failure=/^[A-Z_0-9]+$/.test(e.message)?e.message:e.code||"UPGRADE_FAILED";}
finally{
 for(const i of instances)assert.equal(await i.cleanup(),0);
 mock.closeAllConnections();await new Promise(r=>mock.close(r));
 const result={from:prior.manifest().releaseId,to:current.manifest().releaseId,run,cases,failure,...(failure?{stage}:{})};
 fs.writeFileSync(path.join(root,"evidence/upgrade-result.json"),JSON.stringify(result,null,2));
 console.log(JSON.stringify({passed:cases.length,failure}));
}
process.exitCode=failure?1:0;
