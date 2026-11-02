// SPDX-License-Identifier: Apache-2.0
// Bounded acceptance only: reuse the unchanged release and existing instance.
import fs from "node:fs";
import path from "node:path";
import assert from "node:assert/strict";
import { fileURLToPath, pathToFileURL } from "node:url";
import { spawnSync } from "node:child_process";
import { DatabaseSync } from "node:sqlite";
import { diagnoseReply } from "./reply-diagnosis.mjs";
const phase = process.argv[2] ?? "060";
assert(["060","061","062","063"].includes(phase), "UNKNOWN_ACCEPTANCE_MODE");
const diagnosticMode = phase === "063", fixturePhase = diagnosticMode ? "062" : phase;
const conversationMode = phase !== "060", nativeToolMode = ["062","063"].includes(phase);
const routeNames = diagnosticMode ? ["legacy-2"] : nativeToolMode ? ["legacy-2","legacy-3"] : ["endpoint-sensenova-2","legacy-2","legacy-3"];
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const root = path.join(repo, "development/fusion/runs/060-live-acceptance");
const evidence = path.join(repo, "development/fusion/acceptance", phase);
const load = n => import(pathToFileURL(path.join(root, "extracted/tools", n)));
const { Instance, keytar, json, writeJson, sha, verifyPackage, validateConfig } = await load("runtime.mjs");
assert.equal(process.env.LOADOUT_LIVE_TESTS, "1", "LIVE_OPT_IN_REQUIRED");
assert(!fs.existsSync(path.join(evidence, "result.json")), "DO_NOT_REPEAT_LIVE_RUN");
fs.mkdirSync(evidence, { recursive: true });
const current = json(path.join(repo, "development/fusion/current.json"));
const migration = json(path.join(repo, "development/fusion/migrations/current.json"));
const bindings = json(path.join(repo, "development/fusion/migrations/059/bindings.json"));
const instance = new Instance(migration.instance);
const originalMeta = structuredClone(instance.meta);
const report = { started: new Date().toISOString(), releaseId: current.releaseId, requests: [], maxRequests: diagnosticMode ? 2 : nativeToolMode ? 8 : conversationMode ? 9 : 3,
  choices: migration.choices, runtimeChanges: 0, productionWrites: 0, checks: [], restored: false,
  deferred: ["COMMANDCODE_GPT_POOL_DISABLED", "OPENCODE_GO_GPT_POOL_DISABLED", "SUBSCRIPTION_DISABLED", "WORKBUDDY_NATURAL_ROTATION_NOT_TESTED"] };
let before, vaultBefore, bootstrapBefore, temporaryKey, started = false, stage = "preflight";
let healthProbeBaseline;
if(nativeToolMode)report.deferred.push("SENSENOVA_429_NO_RETEST");
if(diagnosticMode)report.replyDiagnosticPolicy={fixturePhase,strictGateUnchanged:true,maxConversations:1,rawReplyStored:false,historical062ReplyUnavailable:true};
const save = () => writeJson(path.join(evidence, "result.json"), report);
const check = name => { report.checks.push(name); save(); console.log(name); };
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
const vault = async service => (await keytar.findCredentials(service)).sort((a,b) => a.account.localeCompare(b.account));
const selectedKeys = keys => keys.map(k => Object.fromEntries(["id","name","isActive","allowedConnections","allowedModels","allowedCombos","modelAccessMode","rateLimits"].map(n=>[n,k[n]]))).sort((a,b)=>a.id.localeCompare(b.id));
async function configuration() {
  const providers = (await instance.admin("/api/providers")).connections;
  return {
    providers: providers.map(p => Object.fromEntries(["id","provider","isActive","rateLimitProtection","rateLimitOverrides","healthCheckInterval"].map(n=>[n,p[n]]))).sort((a,b)=>a.id.localeCompare(b.id)),
    combos: (await instance.admin("/api/combos")).combos.map(c => Object.fromEntries(["id","name","models","config","strategy"].map(n=>[n,c[n]]))).sort((a,b)=>a.id.localeCompare(b.id)),
    mappings: (await instance.admin("/api/model-combo-mappings")).mappings.map(m=>({pattern:m.pattern,comboId:m.comboId,enabled:m.enabled})).sort((a,b)=>a.pattern.localeCompare(b.pattern)),
    keys: selectedKeys((await instance.admin("/api/keys")).keys),
    resilience: await instance.admin("/api/resilience"),
  };
}
async function request(name, model, key, strict = false) {
  assert(report.requests.length < report.maxRequests);
  const row = { name, requestedModel: model, started: new Date().toISOString(), maxTokens: 256 };
  report.requests.push(row); save();
  const start = performance.now();
  try {
    const response = await fetch(instance.base + "/v1/chat/completions", {
      method: "POST", headers: { authorization: "Bearer " + key, "content-type": "application/json" },
      body: JSON.stringify({ model, stream: false, max_tokens: 256,
        ...(strict ? { allow_account_failover: false } : {}),
        messages: [{ role: "system", content: "You are a helpful assistant." }, { role: "user", content: "Reply with exactly: OK" }] }),
      signal: AbortSignal.timeout(100000), redirect: "error",
    });
    row.status = response.status;
    const raw = await response.text();
    let body; try { body = JSON.parse(raw); } catch { body = {}; }
    row.jsonResponse = Boolean(body && Object.keys(body).length);
    row.nonemptyText = typeof body.choices?.[0]?.message?.content === "string" && body.choices[0].message.content.trim().length > 0;
    row.exactReply = body.choices?.[0]?.message?.content?.trim() === "OK";
    row.finishReason = ["stop","length","tool_calls","content_filter"].includes(body.choices?.[0]?.finish_reason) ? body.choices[0].finish_reason : null;
    row.returnedModel = typeof body.model === "string" && /^[a-zA-Z0-9._/:-]{1,180}$/.test(body.model) ? body.model : null;
    row.usage = Object.fromEntries(["prompt_tokens","completion_tokens","total_tokens"].filter(n=>Number.isFinite(body.usage?.[n])).map(n=>[n,body.usage[n]]));
    // Classify only; never persist upstream error text or the response body.
    const error = JSON.stringify(body.error ?? "");
    row.errorClass = response.ok ? null : /429|rate.?limit/i.test(error) ? "RATE_LIMIT" : /401|unauthoriz|invalid.*key/i.test(error) ? "AUTH" : /403|channel|permission/i.test(error) ? "ACCESS_OR_CHANNEL" : /model.*not|not.*model/i.test(error) ? "MODEL_UNAVAILABLE" : "UNCLASSIFIED_UPSTREAM_OR_GATEWAY";
    row.pass = response.ok && row.jsonResponse && row.nonemptyText;
  } catch (e) { row.pass = false; row.errorClass = e.name === "TimeoutError" ? "TIMEOUT" : "TRANSPORT_ERROR"; }
  row.elapsedMs = Math.round(performance.now() - start); save(); console.log(JSON.stringify(row));
}
function dataSize() {
  const files = fs.readdirSync(path.join(instance.dir,"data"));
  return Object.fromEntries(files.filter(n=>/^storage\.sqlite(?:-wal|-shm)?$/.test(n)).map(n=>[n,fs.statSync(path.join(instance.dir,"data",n)).size]));
}
function healthProbeCount() {
  const db=new DatabaseSync(path.join(instance.dir,"data/storage.sqlite"),{readOnly:true});
  try{return db.prepare("SELECT count(*) AS n FROM call_logs WHERE model='connection-test'").get().n;}finally{db.close();}
}
async function noAutomaticProbes() {
  if(!nativeToolMode)return;
  const current=healthProbeCount();
  if(current!==healthProbeBaseline){
    report.unexpectedHealthProbes=current-healthProbeBaseline;save();
    await instance.stop();started=false;
    throw Error("UNEXPECTED_AUTOMATIC_HEALTH_PROBE");
  }
}
async function observe(label) {
  await noAutomaticProbes();
  const state = await instance.status(); assert.equal(state.status,"ready");
  const processInfo = spawnSync("powershell.exe",["-NoProfile","-NonInteractive","-Command",
    `Get-Process -Id ${Number(state.backendPid)} | Select-Object Id,WorkingSet64,PrivateMemorySize64,CPU | ConvertTo-Json -Compress`],
    {encoding:"utf8",windowsHide:true});
  let resource = null; try { resource = JSON.parse(processInfo.stdout); } catch {}
  (report.observations ??= []).push({label,at:new Date().toISOString(),state,resource,dataBytes:dataSize()}); save();
}
function history(name) {
  const marker = "AMBER-"+fixturePhase+"-"+name.toUpperCase();
  if(nativeToolMode)return {marker,messages:[{role:"system",content:"You are a helpful assistant."},
    {role:"user",content:`Remember original marker ${marker} and record_id CASE-${fixturePhase}. Use lookup_acceptance_record once to retrieve this record, with this original marker and note alpha/beta+gamma=1. The record is unavailable without the tool; never invent its result. After receiving the tool result, return exactly the original marker, the tool receipt and its numeric value, separated by |, with no other text.`}]};
  const messages = [{role:"system",content:"You are a helpful assistant."},
    {role:"user",content:`This is a synthetic gateway acceptance conversation. Remember the original marker ${marker} and record_id CASE-061. These remain unchanged throughout the conversation.`},
    {role:"assistant",content:"I will retain the original marker and record identifier."}];
  // A bounded long history, wholly synthetic, with the marker only near the beginning.
  for(let turn=1;turn<=12;turn++) {
    const rows = Array.from({length:12},(_,n)=>`Archive ${turn}-${n+1}: fictional garden measurements include stone paths, shaded benches, seed trays and rain gauges. This is background data only.`).join("\n");
    messages.push({role:"user",content:rows},{role:"assistant",content:`Background archive ${turn} acknowledged; the initial acceptance identifiers remain unchanged.`});
  }
  messages.push({role:"user",content:"Call lookup_acceptance_record once with the original record_id and marker, and note alpha/beta+gamma=1. Do not invent a tool result."});
  return {marker,messages};
}
const acceptanceTool = {type:"function",function:{name:"lookup_acceptance_record",description:"Read a fixed synthetic acceptance record; no external actions.",
  parameters:{type:"object",properties:{record_id:{type:"string"},marker:{type:"string"},note:{type:"string"}},required:["record_id","marker","note"],additionalProperties:false}}};
let lastSenseStart = 0;
async function conversationRequest(chain, step, extra={}) {
  assert(report.requests.length<report.maxRequests,"LIVE_BUDGET_EXHAUSTED");
  assert(report.requests.filter(r=>r.name===chain.name).length<(diagnosticMode?2:nativeToolMode?4:3),"PROVIDER_BUDGET_EXHAUSTED");
  await noAutomaticProbes();
  if(chain.name==="sensenova") await pause(Math.max(0,25000-(Date.now()-lastSenseStart)));
  const maxTokens=nativeToolMode?1024:512;
  const body={model:chain.model,stream:false,max_tokens:maxTokens,messages:chain.messages,...extra,
    ...(chain.name!=="sensenova"?{allow_account_failover:false}:{})};
  const serialized=JSON.stringify(body);
  assert(Buffer.byteLength(serialized)<64*1024,"ACCEPTANCE_BODY_LIMIT");
  const row={name:chain.name,step,requestedModel:chain.model,started:new Date().toISOString(),requestBytes:Buffer.byteLength(serialized),messages:chain.messages.length,maxTokens,
    inputAssistantMessages:chain.messages.filter(m=>m.role==="assistant").length,
    inputReasoningFields:chain.messages.filter(m=>m.role==="assistant"&&Object.hasOwn(m,"reasoning_content")).length};
  report.requests.push(row); save();
  if(chain.name==="sensenova")lastSenseStart=Date.now();
  const start=performance.now(); let message=null;
  try {
    const response=await fetch(instance.base+"/v1/chat/completions",{method:"POST",headers:{authorization:"Bearer "+chain.key,"content-type":"application/json"},body:serialized,redirect:"error",signal:AbortSignal.timeout(100000)});
    row.status=response.status;
    const raw=await response.text(); let reply;try{reply=JSON.parse(raw);}catch{reply={};}
    row.returnedModel=typeof reply.model==="string"&&/^[a-zA-Z0-9._/:-]{1,180}$/.test(reply.model)?reply.model:null;
    row.modelMatched=row.returnedModel===chain.upstreamModel;
    row.usage=Object.fromEntries(["prompt_tokens","completion_tokens","total_tokens"].filter(n=>Number.isFinite(reply.usage?.[n])).map(n=>[n,reply.usage[n]]));
    message=reply.choices?.[0]?.message;
    row.assistantReasoningFieldPresent=Boolean(message&&Object.hasOwn(message,"reasoning_content"));
    row.finishReason=["stop","length","tool_calls","content_filter"].includes(reply.choices?.[0]?.finish_reason)?reply.choices[0].finish_reason:null;
    if(!response.ok){
      const err=JSON.stringify(reply.error??"");
      row.errorClass=/429|rate.?limit/i.test(err)?"RATE_LIMIT":/401|unauthoriz|invalid.*key/i.test(err)?"AUTH":/403|channel|permission/i.test(err)?"ACCESS_OR_CHANNEL":"UPSTREAM_OR_GATEWAY_REJECTION";
    } else if(step==="tool-request") {
      row.toolCount=message?.tool_calls?.length??0;
      const tool=message?.tool_calls?.[0]; let args;
      try{args=JSON.parse(tool?.function?.arguments);}catch{}
      row.toolNameMatched=tool?.function?.name===acceptanceTool.function.name;
      try{assert.deepEqual(args,{record_id:"CASE-"+fixturePhase,marker:chain.marker,note:"alpha/beta+gamma=1"});row.toolArgumentsMatched=true;}catch{row.toolArgumentsMatched=false;}
      row.pass=row.modelMatched&&row.toolCount===1&&row.toolNameMatched&&row.toolArgumentsMatched&&typeof tool?.id==="string"&&tool.id.length>0;
    } else {
      row.historyAndToolResultMatched=typeof message?.content==="string"&&message.content.trim()===chain.marker+"|LOCAL-"+fixturePhase+"-OK|37";
      if(diagnosticMode){
        row.replyDiagnosis=diagnoseReply(message?.content,[chain.marker,"LOCAL-"+fixturePhase+"-OK","37"]);
        assert.equal(row.replyDiagnosis.strictPass,row.historyAndToolResultMatched,"STRICT_GATE_CHANGED");
        row.additionalToolCalls=message?.tool_calls?.length??0;
      }
      row.pass=row.modelMatched&&row.historyAndToolResultMatched&&!(message?.tool_calls?.length);
    }
    row.pass=Boolean(response.ok&&row.pass);
    if(response.ok&&!row.pass)row.errorClass="SEMANTIC_ACCEPTANCE_FAILED";
    if(diagnosticMode&&response.ok&&!row.pass&&row.modelMatched&&!row.additionalToolCalls&&row.replyDiagnosis?.classification==="FORMAT_ONLY")row.errorClass="STRICT_FORMAT_MISMATCH";
  } catch(e){row.pass=false;row.errorClass=e.name==="TimeoutError"?"TIMEOUT":"TRANSPORT_ERROR";}
  row.elapsedMs=Math.round(performance.now()-start);save();console.log(JSON.stringify(row));
  await noAutomaticProbes();
  return row.pass?message:null;
}
async function conversations(legacyKey) {
  const chains=[...(nativeToolMode?[]:[{name:"sensenova",model:"endpoint-sensenova-2",upstreamModel:"deepseek-v4-flash",key:temporaryKey.key}]),
    {name:"deepseek",model:"deepseek/main/deepseek-flash",upstreamModel:"deepseek-flash",key:legacyKey},
    ...(diagnosticMode?[]:[{name:"workbuddy",model:"workbuddy/main/deepseek-v4.1-flash",upstreamModel:"deepseek-v4.1-flash",key:legacyKey}])].map(c=>({...c,...history(c.name)}));
  const liveReady=Date.now();
  report.dataBytesBefore=dataSize();
  await observe("before-conversations");
  for(const chain of chains) {
    const call=await conversationRequest(chain,"tool-request",{tools:[acceptanceTool],tool_choice:nativeToolMode?"auto":{type:"function",function:{name:acceptanceTool.function.name}}});
    if(!call)continue;
    // Keep the complete returned message, including upstream reasoning_content.
    chain.messages.push(call,{role:"tool",tool_call_id:call.tool_calls[0].id,content:JSON.stringify({receipt:"LOCAL-"+fixturePhase+"-OK",value:37})});
    if(!nativeToolMode)chain.messages.push({role:"user",content:"Return exactly the original marker, the tool receipt and its numeric value, separated by |, with no other text."});
    const answer=await conversationRequest(chain,"tool-result",{tools:[acceptanceTool],tool_choice:nativeToolMode?"auto":"none"});
    if(!answer)continue;
    chain.messages.push(answer);
    if(diagnosticMode){chain.ready=true;continue;}
    if(nativeToolMode){
      const background=Array.from({length:144},(_,n)=>`Archive ${n+1}: fictional garden measurements include stone paths, shaded benches, seed trays and rain gauges. This is background data only.`).join("\n");
      chain.messages.push({role:"user",content:background+"\nContinue the original task. Without another tool call, recall the original marker and the earlier tool receipt and value. Return exactly those three values separated by |, with no other text."});
      const continued=await conversationRequest(chain,"long-context",{tools:[acceptanceTool],tool_choice:"auto"});
      if(!continued)continue;
      chain.messages.push(continued);
    }
    chain.ready=true;
  }
  if(diagnosticMode){await observe("after-reply-diagnosis");report.observationMs=Date.now()-liveReady;return;}
  // Observe the same backend for at least three minutes without extra inference.
  while(Date.now()-liveReady<180000){await pause(Math.min(30000,180000-(Date.now()-liveReady)));await observe("idle-before-restart");console.log("local observation; no extra inference");}
  stage="normal-stop-restart";
  const preState=await instance.status(),preConfig=await configuration();
  await instance.stop();started=false;
  assert.equal((await instance.status()).status,"stopped");
  await pause(5000);
  await instance.start();started=true;
  const postState=await instance.status();
  assert.equal(postState.instance,preState.instance);assert.equal(postState.port,preState.port);
  assert.notEqual(postState.backendPid,preState.backendPid);assert.notEqual(postState.run,preState.run);
  assert.deepEqual(await configuration(),preConfig,"RESTART_CONFIGURATION_MISMATCH");
  assert.deepEqual(await vault(instance.meta.service),vaultBefore);assert.deepEqual(await vault(instance.meta.bootstrap),bootstrapBefore);
  assert.equal(await keytar.getPassword(instance.meta.bootstrap,"route-legacy-client-2"),legacyKey);
  report.restart={pass:true,sameInstance:true,samePort:true,newBackend:true,configurationPreserved:true,vaultPreserved:true,clientKeyPreserved:true};
  check("normal-stop-restart-restored-native-state");
  for(const chain of chains.filter(c=>c.ready)) {
    chain.messages.push({role:"user",content:"Continue our previous conversation. Recall the original marker and the previous tool receipt and value. Return those same three values separated by |, with no other text."});
    await conversationRequest(chain,"after-restart",{tools:[acceptanceTool],tool_choice:nativeToolMode?"auto":"none"});
  }
  await pause(30000);await observe("after-restart-conversations");
  report.observationMs=Date.now()-liveReady;
}
save();
try {
  verifyPackage(conversationMode);
  assert.equal(sha(fs.readFileSync(current.archive)), current.archiveSha256);
  assert.equal((await instance.status()).status, "stopped");
  assert.equal(instance.meta.config.network, "loopback-only");
  if(nativeToolMode){healthProbeBaseline=healthProbeCount();report.healthProbesBefore=healthProbeBaseline;}
  assert.equal(migration.choices.gptPool, "WHOLE_POOL_DISABLED_BY_USER");
  assert(!Object.keys(bindings).some(k=>k.startsWith("codex-sub/")));
  vaultBefore = await vault(instance.meta.service); bootstrapBefore = await vault(instance.meta.bootstrap);
  assert.equal(vaultBefore.length, 16); assert.equal(bootstrapBefore.length, 10);
  await instance.start(); started = true;
  before = await configuration();
  assert.equal(before.providers.length,16); assert.equal(before.combos.length,8);
  // 061 exposed a separate startup health sweep; do not reopen real egress
  // unless the native per-connection opt-out is already configured.
  if(conversationMode) {
    const connections=(await instance.admin("/api/providers")).connections;
    assert(connections.every(p=>p.healthCheckInterval===0),"DISABLE_NATIVE_ACCOUNT_PROBES_BEFORE_LIVE_ACCEPTANCE");
  }
  assert(!before.combos.some(c=>/gpt|default/i.test(c.name)));
  assert(!before.mappings.some(m=>m.enabled && /loadout\/(default|fast|realtime|gpt)/.test(m.pattern)));
  const senseIds = Object.entries(bindings).filter(([k])=>k.startsWith("sensenova/")).map(([,v])=>v.connectionId);
  assert.equal(senseIds.length,12);
  for (const p of before.providers.filter(p=>senseIds.includes(p.id))) {
    assert.equal(p.rateLimitProtection,true);
    assert(p.rateLimitOverrides.minTime>=20000 && p.rateLimitOverrides.maxConcurrent===1 && p.rateLimitOverrides.rpm<=3);
  }
  writeJson(path.join(evidence,"restore-config.json"), { meta: originalMeta, configuration: before });
  check("original-scope-and-pacing-verified");
  if(nativeToolMode){await pause(20000);await noAutomaticProbes();await pause(15000);await noAutomaticProbes();check("offline-startup-window-no-health-probes");}
  stage = "restrict-attempts";
  for (const name of routeNames) {
    const c = before.combos.find(c=>c.name===name); assert(c);
    assert.equal(c.config.failoverBeforeRetry,true); // Disables BaseExecutor intra-URL retries.
    await instance.admin("/api/combos/"+c.id,"PUT",{config:{...c.config,maxGlobalAttempts:1,maxRetries:0,maxSetRetries:0}});
  }
  await instance.admin("/api/resilience","PATCH",{comboCooldownWait:{enabled:false},waitForCooldown:{enabled:false,maxRetries:0}});
  const scope = bindings["sensenova/account-1"].connectionId;
  if(!nativeToolMode){
    temporaryKey = await instance.admin("/api/keys","POST", { name:`acceptance-${phase}-sense-single`, noLog:true, modelAccessMode:"restricted", allowedConnections:[scope], allowedCombos:["endpoint-sensenova-2"], allowedModels:["endpoint-sensenova-2"] });
    assert(temporaryKey.id && temporaryKey.key);
    writeJson(path.join(evidence,"temporary-key.json"),{id:temporaryKey.id});
  }
  const narrowed = await configuration();
  for(const c of narrowed.combos.filter(c=>routeNames.includes(c.name))) assert.equal(c.config.maxGlobalAttempts,1);
  if(temporaryKey)assert.deepEqual(narrowed.keys.find(k=>k.id===temporaryKey.id).allowedConnections,[scope]);
  assert.equal(narrowed.resilience.comboCooldownWait.enabled,false);
  await instance.stop(); started=false;
  instance.meta.config = validateConfig({...originalMeta.config, network:"allowlist", hosts:diagnosticMode?["api.deepseek.com"]:nativeToolMode?["api.deepseek.com","www.workbuddy.ai"]:["token.sensenova.cn","api.deepseek.com","www.workbuddy.ai"]});
  writeJson(path.join(instance.dir,"instance.json"),instance.meta);
  await instance.start(); started=true;
  check("single-attempt-and-host-allowlist-ready");
  stage="bounded-live-requests";
  const legacyKey = await keytar.getPassword(instance.meta.bootstrap,"route-legacy-client-2"); assert(legacyKey);
  if(conversationMode)await conversations(legacyKey);
  else {
    await request("sensenova", "endpoint-sensenova-2", temporaryKey.key);
    await request("deepseek", "deepseek/main/deepseek-flash", legacyKey, true);
    await request("workbuddy", "workbuddy/main/deepseek-v4.1-flash", legacyKey, true);
  }
  report.status = "BOUNDED_LIVE_COMPLETED";
} catch (e) {
  report.status="ACCEPTANCE_INTERRUPTED"; report.failureStage=stage;
  report.failureCode=/^[A-Z_0-9]+$/.test(e.message)?e.message:e.code??e.name;
} finally {
  try {
    if (before) {
      // If the live startup failed, restore through the same offline control plane.
      if((await instance.status()).status!=="ready"){instance.meta.config=structuredClone(originalMeta.config);writeJson(path.join(instance.dir,"instance.json"),instance.meta);await instance.start();started=true;}
      if (temporaryKey) await instance.admin("/api/keys/"+temporaryKey.id,"DELETE");
      for (const c of before.combos.filter(c=>routeNames.includes(c.name))) await instance.admin("/api/combos/"+c.id,"PUT",{config:c.config});
      await instance.admin("/api/resilience","PATCH",{comboCooldownWait:before.resilience.comboCooldownWait,waitForCooldown:before.resilience.waitForCooldown});
      assert.deepEqual(await configuration(),before,"CONFIG_RESTORE_MISMATCH");
      check("native-configuration-and-key-scopes-restored");
    }
  } catch (e) { report.restoreError=/^[A-Z_0-9]+$/.test(e.message)?e.message:e.code??e.name; }
  try {
    await instance.stop();
    instance.meta=structuredClone(originalMeta); writeJson(path.join(instance.dir,"instance.json"),instance.meta);
    assert.equal((await instance.status()).status,"stopped");
    assert.deepEqual(await vault(instance.meta.service),vaultBefore);
    assert.deepEqual(await vault(instance.meta.bootstrap),bootstrapBefore);
    assert.equal(sha(fs.readFileSync(current.archive)),current.archiveSha256);
    // Check only files owned by this instance; secrets remain in OS vault.
    const secrets=[...vaultBefore,...bootstrapBefore].map(v=>v.password).filter(v=>v.length>12);
    function scan(dir) { for(const e of fs.readdirSync(dir,{withFileTypes:true})) {
      const p=path.join(dir,e.name); assert(!e.isSymbolicLink());
      if(e.isDirectory())scan(p);else {const b=fs.readFileSync(p);for(const s of secrets)assert(!b.includes(Buffer.from(s))&&!b.includes(Buffer.from(s,"utf16le")),"SECRET_ON_DISK");}
    }}
    scan(instance.dir);
    if(conversationMode){
      const db=new DatabaseSync(path.join(instance.dir,"data/storage.sqlite"),{readOnly:true});
      try {
        const accountNames=new Map(Object.entries(bindings).map(([n,b])=>[b.connectionId,n]));
        const attribute=({connection_id,...row})=>({...row,aggregate:!connection_id,account:connection_id?(accountNames.get(connection_id)??"UNKNOWN"):null});
        const rows=db.prepare("SELECT timestamp,model,connection_id,status,success,tokens_input,tokens_output FROM usage_history WHERE timestamp>=? ORDER BY timestamp").all(report.started).map(attribute);
        const logs=db.prepare("SELECT timestamp,model,connection_id,status,has_request_body,has_response_body FROM call_logs WHERE timestamp>=? ORDER BY timestamp").all(report.started).map(attribute);
        const details=db.prepare("SELECT count(*) AS n FROM request_detail_logs WHERE timestamp>=?").get(report.started).n;
        writeJson(path.join(evidence,"usage-audit.json"),{usage:rows,callLogs:logs,detailRows:details});
        const executionRows=rows.filter(r=>!r.aggregate), executionLogs=logs.filter(r=>!r.aggregate&&r.model!=="connection-test");
        assert(!logs.some(r=>r.model==="connection-test"),"UNEXPECTED_AUTOMATIC_HEALTH_PROBE");
        assert(executionRows.length<=report.requests.length&&executionLogs.length<=report.requests.length,"UNEXPECTED_UPSTREAM_ATTEMPTS");
        const allowed={"sensenova/account-1":"deepseek-v4-flash","deepseek/account-1":"deepseek-flash","workbuddy/account-1":"deepseek-v4.1-flash"};
        if(nativeToolMode)delete allowed["sensenova/account-1"];
        if(diagnosticMode)delete allowed["workbuddy/account-1"];
        assert([...executionRows,...executionLogs].every(r=>allowed[r.account]===r.model),"ACCOUNT_OR_MODEL_SCOPE");
        assert(logs.every(r=>!r.has_request_body&&!r.has_response_body)&&details===0,"BODY_LOGGING");
        report.nativeAudit={usageRows:executionRows.length,callLogRows:executionLogs.length,aggregateUsageRows:rows.length-executionRows.length,aggregateCallLogRows:logs.length-executionLogs.length,detailRows:details,scopeMatched:true};
        report.dataBytesAfter=dataSize();
        if(nativeToolMode){report.healthProbesAfter=healthProbeCount();assert.equal(report.healthProbesAfter,healthProbeBaseline,"UNEXPECTED_AUTOMATIC_HEALTH_PROBE");}
      } finally {db.close();}
    }
    assert.equal(fs.readdirSync(path.join(repo,"dist/fusion")).filter(n=>n.endsWith(".zip")).length,1,"CANDIDATE_COUNT");
    report.restored=!report.restoreError && Boolean(before);
    report.finalState={status:"stopped",network:"loopback-only",providerCredentials:16,bootstrapEntries:10,candidateSha256:current.archiveSha256};
    check("stopped-vault-unchanged-no-secret-on-disk-single-candidate");
  } catch(e) {report.finalAuditError=/^[A-Z_0-9]+$/.test(e.message)?e.message:e.code??e.name;}
  report.finished=new Date().toISOString();save();
}
console.log(JSON.stringify({status:report.status,requests:report.requests.length,passed:report.requests.filter(r=>r.pass).length,restored:report.restored}));
if(!report.restored || report.status!=="BOUNDED_LIVE_COMPLETED")process.exitCode=1;
