// SPDX-License-Identifier: Apache-2.0
// 059 execution receipt: use the unchanged candidate's native APIs and migration tools.
import fs from "node:fs";
import path from "node:path";
import net from "node:net";
import assert from "node:assert/strict";
import { once } from "node:events";
import { spawnSync } from "node:child_process";
import { pathToFileURL, fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";
import { DatabaseSync } from "node:sqlite";
const repo=path.resolve(path.dirname(fileURLToPath(import.meta.url)),"../..");
const run=path.join(repo,"development/fusion/runs/059-real-binding");
const pkg=path.join(run,"extracted");
const destination=path.join(repo,"development/fusion/isolated/loadout-fusion-isolated/059-current");
const evidence=path.join(repo,"development/fusion/migrations/059");
const sourceFile=path.join(process.env.LOCALAPPDATA,"Loadout/data/loadout.db");
if(process.env.LOADOUT_LIVE_TESTS!=="1")throw Error("PRIVATE_BINDING_OPT_IN_REQUIRED");
const load=n=>import(pathToFileURL(path.join(pkg,"tools",n)));
const {Instance,keytar,sha,manifest,verifyPackage,writeJson}=await load("runtime.mjs");
const {snapshot}=await load("migrate.mjs");
const {compile,applyReady,nativeWait,fingerprint}=await load("mapping.mjs");
const {configure}=await load("policy.mjs");
const {backup,restore}=await load("backup.mjs");
const {desktopReader}=await load("credential-reader.mjs");
const {accountDigest}=await load("workbuddy-connect.mjs");
const legacyWb=await import(pathToFileURL(path.join(repo,"development/experiments/omniroute-phase7/workbuddy-connect.mjs")));
fs.mkdirSync(evidence,{recursive:true});
const report={releaseId:manifest().releaseId,cases:[],externalIssues:["SENSENOVA_RISK_429","COMMANDCODE_401","WORKBUDDY_NATURAL_ROTATION"].map(code=>({code,status:"DEFERRED_NOT_TESTED"})),
  choices:{defaultAlias:"DISABLED_BY_USER",subscriptionTransport:"DISABLED_BY_USER",gptPool:"WHOLE_POOL_DISABLED_BY_USER"},
  realInferenceRequests:0,productionWrites:0};
let original, recovered,stage,privateRows=[],wbState,full,bindings={};
const sensitive=[];
const save=()=>writeJson(path.join(evidence,"result.json"),report);
const check=async(name,fn)=>{stage=name;await fn();report.cases.push({name,pass:true});save();console.log(name+" PASS");};
function sourceInput(operation,refs=[]){
  const r=spawnSync("python",[path.join(repo,"scripts/fusion/source-binding-input.py")],{
    input:JSON.stringify({source:sourceFile,operation,refs}),encoding:"utf8",windowsHide:true,maxBuffer:1024*1024,
    env:{...process.env,PYTHONDONTWRITEBYTECODE:"1",LOADOUT_LIVE_TESTS:"1"},
  });
  if(r.status!==0)throw Error("SOURCE_BINDING_INPUT_FAILED");
  return JSON.parse(r.stdout);
}
async function freePort(){const server=net.createServer();server.listen(0,"127.0.0.1");await once(server,"listening");const port=server.address().port;await new Promise(r=>server.close(r));return port;}
function existingBaseUrl(row){
  if(row.baseUrl!==null)return row.baseUrl;
  const files={sensenova:"presets.rs",commandcode:"commandcode.rs",deepseek:"deepseek.rs","opencode-go":"opencode_go.rs"};
  const sourcePath="crates/providers/src/"+files[row.kind],text=fs.readFileSync(path.join(repo,sourcePath),"utf8");
  const match=row.kind==="sensenova"?text.match(/descriptor\(\s*"sensenova",\s*"[^"]+",\s*"([^"]+)"/):text.match(/default_base_url:\s*Some\("([^"]+)"\)/);
  if(!match)throw Error("LEGACY_DEFAULT_ENDPOINT_UNRESOLVED");
  report.endpointSources??={};report.endpointSources[row.provider]={sourcePath,sourceSha256:sha(text),baseUrl:match[1]};
  return match[1];
}
async function nativeProvider(slug,baseUrl,accounts,apiType){
  // Native control APIs do not need inference egress. Keep the process loopback-only.
  const url=new URL(baseUrl);
  assert(!url.username&&!url.password&&!url.search&&!url.hash);
  assert(url.protocol==="https:"||(url.protocol==="http:"&&url.hostname==="127.0.0.1"));
  const {node}=await original.admin("/api/provider-nodes","POST",{type:"openai-compatible",name:slug,prefix:"fusion-"+slug,apiType,baseUrl});
  for(const [index,a] of accounts.entries()){
    const {connection}=await original.admin("/api/providers","POST",{provider:node.id,name:a.alias,apiKey:a.key,priority:index+1,
      providerSpecificData:{baseUrl,apiType}});
    await original.admin("/api/providers/"+connection.id,"PUT",{isActive:true});
    bindings[a.ref]={provider:node.id,connectionId:connection.id,models:full.accountModels.filter(m=>m.account===a.ref).map(m=>m.model)};
  }
}
async function configuration(instance){
  const providers=(await instance.admin("/api/providers")).connections;
  const combos=(await instance.admin("/api/combos")).combos;
  const mappings=(await instance.admin("/api/model-combo-mappings")).mappings;
  const keys=(await instance.admin("/api/keys")).keys;
  return {providers:providers.map(p=>({id:p.id,provider:p.provider,isActive:p.isActive,rateLimitOverrides:p.rateLimitOverrides,rateLimitProtection:p.rateLimitProtection})).sort((a,b)=>a.id.localeCompare(b.id)),
    combos:combos.map(c=>({id:c.id,name:c.name,models:c.models,config:c.config,strategy:c.strategy})).sort((a,b)=>a.id.localeCompare(b.id)),
    mappings:mappings.map(m=>({pattern:m.pattern,comboId:m.comboId,enabled:m.enabled})).sort((a,b)=>a.pattern.localeCompare(b.pattern)),
    keys:keys.map(k=>({id:k.id,name:k.name,isActive:k.isActive,allowedConnections:k.allowedConnections,allowedModels:k.allowedModels,allowedCombos:k.allowedCombos,modelAccessMode:k.modelAccessMode,rateLimits:k.rateLimits})).sort((a,b)=>a.id.localeCompare(b.id)),
    resilience:await instance.admin("/api/resilience")};
}
async function vault(instance){return (await keytar.findCredentials(instance.meta.service)).sort((a,b)=>a.account.localeCompare(b.account));}
async function assertBindings(instance){
  const db=new DatabaseSync(path.join(instance.dir,"data/storage.sqlite"),{readOnly:true});
  try{for(const row of privateRows){
    const stored=db.prepare("SELECT api_key FROM provider_connections WHERE id=?").get(bindings[row.ref].connectionId);
    assert(stored?.api_key?.startsWith("loadout-ref:v1:"));
    assert.equal(await keytar.getPassword(instance.meta.service,stored.api_key),row.key);
  }}finally{db.close();}
}
function scanOwnedFiles(dir){for(const e of fs.readdirSync(dir,{withFileTypes:true})){
  const file=path.join(dir,e.name);assert(!e.isSymbolicLink());
  if(e.isDirectory())scanOwnedFiles(file);
  else {const bytes=fs.readFileSync(file);for(const secret of sensitive){assert(!bytes.includes(Buffer.from(secret)),"SECRET_ON_DISK");assert(!bytes.includes(Buffer.from(secret,"utf16le")),"SECRET_ON_DISK_UTF16");}}
}}
try{
  await check("candidate-and-source-fixed",async()=>{
    verifyPackage(false);assert.equal(manifest().releaseId,JSON.parse(fs.readFileSync(path.join(repo,"development/fusion/current.json"))).releaseId);
    full=snapshot(sourceFile);report.sourceFingerprint=fingerprint(full);
    const meta=sourceInput("metadata");
    assert.equal(meta.find(a=>a.kind==="codex-subscription").transportOptIn,false);
    const selected=meta.filter(a=>a.enabled&&["sensenova","commandcode","deepseek","opencode-go"].includes(a.kind));
    assert.equal(selected.length,15);assert(selected.every(a=>a.apiKeyAvailable));
    privateRows=sourceInput("private-copy",selected.map(a=>a.ref));
    assert(privateRows.every(a=>typeof a.key==="string"&&a.key.length>0));sensitive.push(...privateRows.map(a=>a.key));
    report.sourceAccounts=meta.map(a=>({account:a.ref,kind:a.kind,enabled:a.enabled,transportOptIn:a.transportOptIn}));
    report.exactAliases={};
    for(const alias of full.clientPolicies.flatMap(p=>p.allowed_models??[]).filter(a=>!a.startsWith("loadout/"))){
      const [provider,account,...modelParts]=alias.split("/"),model=modelParts.join("/");
      const rows=meta.filter(a=>a.provider===provider&&a.alias===account&&a.enabled);assert.equal(rows.length,1);
      assert(full.accountModels.some(m=>m.account===rows[0].ref&&m.model===model));
      report.exactAliases[alias]=[{account:rows[0].ref,model}];
    }
    assert.equal(Object.keys(report.exactAliases).length,2);
  });
  await check("bound-desktop-session-read-only",async()=>{
    const bindingFile=path.join(process.env.LOCALAPPDATA,"LoadoutWorkBuddyBridge/binding.json");
    const bound=JSON.parse(fs.readFileSync(bindingFile));const doc=JSON.parse(fs.readFileSync(legacyWb.authPath));
    assert.equal(accountDigest(doc.account?.uid),bound.account_hash);
    wbState={authPath:legacyWb.authPath,exe:legacyWb.exe,accountHash:bound.account_hash,...legacyWb.identity(),port:await freePort()};
    report.workbuddySourceBefore=legacyWb.fileReceipt();
    const r=await desktopReader(wbState).current();sensitive.push(r.value.accessToken);
    report.workbuddySession={accountMatchesOldBinding:true,accessReadable:true,upstreamVerified:false};
  });
  await check("real-api-keys-stored-in-isolated-vault",async()=>{
    if(process.argv.includes("--resume-empty")){
      original=new Instance(destination);assert.equal((await original.status()).status,"stopped");
      assert.equal(original.meta.config.network,"loopback-only");assert.equal(original.meta.config.workbuddy.accountHash,wbState.accountHash);
      wbState=original.meta.config.workbuddy;
    }else original=await Instance.create(destination,undefined,{network:"loopback-only",port:0,workbuddy:wbState});
    report.instance=destination;report.instanceId=original.meta.id;save();await original.start();
    assert.equal((await original.admin("/api/providers")).connections.length,0);
    assert.equal((await original.admin("/api/combos")).combos.length,0);
    for(const slug of ["sensenova","commandcode","deepseek","opencode-go"]){
      const rows=privateRows.filter(a=>a.provider===slug);assert(rows.every(a=>a.baseUrl===rows[0].baseUrl));
      await nativeProvider(slug,existingBaseUrl(rows[0]),rows,slug==="opencode-go"?"responses":"chat");
    }
    const secrets=await original.secrets();
    await nativeProvider("workbuddy","http://127.0.0.1:"+wbState.port+"/v1",[{ref:"workbuddy/account-1",alias:"main",key:secrets.bridge}],"chat");
    await configure(original,privateRows.filter(a=>a.kind==="sensenova").map(a=>bindings[a.ref].connectionId));
    await assertBindings(original);report.boundAccounts=Object.keys(bindings).length;report.copiedApiKeys=privateRows.length;
    writeJson(path.join(evidence,"bindings.json"),bindings);
  });
  await check("complete-pools-and-exact-aliases-with-explicit-disabled-scope",async()=>{
    const projection=structuredClone(full);
    report.pendingPools=projection.pools.filter(p=>p.name==="gpt").map(p=>({...p,status:"DISABLED_BY_USER",reason:"SUBSCRIPTION_TRANSPORT_REMAINS_OFF"}));
    assert.equal(report.pendingPools.length,1);assert.equal(report.pendingPools[0].members.length,3);
    projection.pools=projection.pools.filter(p=>p.name!=="gpt");
    for(const p of projection.clientPolicies) if(p.allowed_models)p.allowed_models=p.allowed_models.filter(a=>a!=="loadout/default");
    const endpointModels=Object.fromEntries(full.endpoints.map(e=>[e.name,[...new Set(full.accountModels.filter(m=>full.accounts.find(a=>a.ref===m.account)?.provider===e.provider).map(m=>m.model))].sort()]));
    const plan=compile(projection,bindings,endpointModels,"native-bounded-v1",{aliases:report.exactAliases});
    assert(plan.activationReady);assert.equal(plan.routes.length,8);
    await original.admin("/api/resilience","PATCH",nativeWait());
    await applyReady(original,plan,{source:projection,bindings});
    writeJson(path.join(original.dir,"migration.json"),{fullSource:full,projection,bindings,plan,
      disabledAliases:["loadout/default","loadout/fast","loadout/realtime"],pendingPools:report.pendingPools,choices:report.choices,fullMigrationReady:false});
    writeJson(path.join(evidence,"mapping.json"),{sourceFingerprint:fingerprint(full),plan,pendingPools:report.pendingPools,choices:report.choices});
    const read=await configuration(original);
    assert.equal(read.combos.length,8);assert(!read.combos.some(c=>c.name==="pool-gpt"));
    assert.equal(read.mappings.length,3);assert(!read.mappings.some(m=>m.pattern==="loadout/default"));
    const client=read.keys.find(k=>k.name==="legacy-client-2");assert(client);
    assert.deepEqual([...client.allowedModels].sort(),Object.keys(report.exactAliases).sort());
    assert.deepEqual(client.rateLimits,[{limit:8,window:60}]);
    assert.deepEqual([...client.allowedConnections].sort(),Object.values(report.exactAliases).flat().map(m=>bindings[m.account].connectionId).sort());
    report.appliedRoutes=plan.routes.map(r=>({name:r.name,alias:r.sourceAlias??null,members:r.sourceMembers.length,model:r.upstreamModel??null}));
  });
  await check("encrypted-restore-keeps-real-bindings-and-policy",async()=>{
    const configBefore=await configuration(original);await original.stop();
    const beforeVault=await vault(original),beforeBootstrap=(await keytar.findCredentials(original.meta.bootstrap)).sort((a,b)=>a.account.localeCompare(b.account));
    const archive=path.join(run,"restore-check.enc.json"),password=randomUUID()+randomUUID();
    await backup(original,archive,password);
    recovered=await restore(archive,path.join(run,"loadout-fusion-isolated/restored"),password);
    assert.deepEqual(await vault(recovered),beforeVault);
    assert.deepEqual((await keytar.findCredentials(recovered.meta.bootstrap)).sort((a,b)=>a.account.localeCompare(b.account)),beforeBootstrap);
    await recovered.start();await assertBindings(recovered);assert.deepEqual(await configuration(recovered),configBefore);
    assert.deepEqual(JSON.parse(fs.readFileSync(path.join(recovered.dir,"migration.json"))),JSON.parse(fs.readFileSync(path.join(original.dir,"migration.json"))));
    report.restore={vaultEqual:true,bootstrapEqual:true,nativeConfigurationEqual:true,realApiKeysEqual:true,migrationRecordEqual:true};
  });
  await check("disabled-and-out-of-scope-models-refused",async()=>{
    const key=await keytar.getPassword(recovered.meta.bootstrap,"route-legacy-client-2");assert(key);
    const outcomes=[];
    for(const model of ["loadout/default","loadout/gpt","deepseek/main/not-authorized"]){
      const response=await fetch(recovered.base+"/v1/chat/completions",{method:"POST",headers:{authorization:"Bearer "+key,"content-type":"application/json"},
        body:JSON.stringify({model,messages:[{role:"user",content:"ISOLATED_SCOPE_DENIAL_ONLY"}]}),signal:AbortSignal.timeout(10000)});
      await response.arrayBuffer();assert([400,403,404].includes(response.status));outcomes.push({model,status:response.status});
    }
    report.denials=outcomes;
  });
  await check("source-unchanged-and-no-plaintext-secrets",async()=>{
    await recovered.stop();await original.stop();
    assert.equal(fingerprint(snapshot(sourceFile)),report.sourceFingerprint);
    const again=sourceInput("private-copy",privateRows.map(a=>a.ref));
    assert(again.every(a=>privateRows.find(b=>b.ref===a.ref)?.key===a.key));
    report.workbuddySourceAfter=legacyWb.fileReceipt();assert.deepEqual(report.workbuddySourceAfter,report.workbuddySourceBefore);
    for(const instance of [original,recovered])scanOwnedFiles(instance.dir);
    scanOwnedFiles(evidence);
    report.sourceMetadataUnchanged=true;report.sourceApiKeysUnchanged=true;report.workbuddyFilesUnchanged=true;
  });
  report.status="PARTIAL_SCOPE_MIGRATED_AND_RESTORE_VERIFIED";
}catch(e){report.status="INCOMPLETE";report.failure={stage,code:/^[A-Z_0-9]+$/.test(e.message)?e.message:"CHECK_FAILED"};process.exitCode=1;}
finally{
  if(original){await original.stop();report.retainedInstance={status:(await original.status()).status,providerVaultEntries:(await vault(original)).length,
    bootstrapEntries:(await keytar.findCredentials(original.meta.bootstrap)).length,purpose:"STOPPED_ISOLATED_MIGRATION_STATE"};}
  if(recovered){report.restoredCleanup={directory:recovered.dir,remaining:await recovered.cleanup(),status:(await recovered.status()).status};}
  report.finished=new Date().toISOString();save();console.log(JSON.stringify({status:report.status,boundAccounts:report.boundAccounts,checks:report.cases.length,failure:report.failure}));
}
