// SPDX-License-Identifier: Apache-2.0
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import http from "node:http";
import { once } from "node:events";
import { generateKeyPairSync, sign, randomUUID } from "node:crypto";
import { spawn } from "node:child_process";
import { DatabaseSync } from "node:sqlite";
import { Instance, sleep, baseline, backup, restore, pkg, load } from "./harness.mjs";

const root = path.resolve(process.env.FUSION_RELEASE_ROOT);
const run = path.join(root, "runs/loadout-fusion-isolated/radar-" + Date.now());
fs.mkdirSync(run, { recursive: true });
const result = { baseline: baseline(), cases: [] }, instances = [], requests = [];
const { publicKey, privateKey } = generateKeyPairSync("ed25519");
const publicDer = publicKey.export({ type: "spki", format: "der" }).toString("base64");
const fixture = JSON.parse(fs.readFileSync(path.join(process.env.FUSION_UPSTREAM,"tests/fixtures/radar-feed-canonical.json")));
fixture.generatedAt = "2026-10-01T12:00:00Z";
fixture.tier = "live";
fixture.models[0].displayName = "Synthetic Radar Catalog";
let revision = 1, mode = "valid", i, profile, catalog, status, scope;
const version = () => "2026.10.01." + revision;
const pulls = () => requests.filter(r => r.path === "/v1/catalog/latest").length;
const check = async (name, action) => {
  try { await action(); result.cases.push({ name, pass: true }); }
  catch (error) { result.cases.push({ name, pass: false, code: error.code || error.message.slice(0, 65), ...(typeof error.actual === "number" ? {actual:error.actual, expected:error.expected} : {}) }); }
  fs.writeFileSync(path.join(run, "result.json"), JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result.cases.at(-1)));
};
const server = http.createServer(async (req, res) => {
  let body = ""; for await (const c of req) body += c;
  if (req.url.startsWith("/provider")) { res.writeHead(200, {"content-type":"application/json"}); res.end(JSON.stringify({data:[]})); return; }
  requests.push({ path: req.url, method: req.method, bodyLength: body.length,
    authorization: Boolean(req.headers.authorization), cookie: Boolean(req.headers.cookie), schema: req.headers["x-omniroute-radar-schema"] });
  if (req.url !== "/v1/catalog/latest") { res.writeHead(503); res.end(); return; }
  if (mode === "http-error") { res.writeHead(503); res.end(); return; }
  if (mode === "oversized") { res.writeHead(200, { "content-length": 10 * 1024 * 1024 + 1 }); res.end(); return; }
  const bytes = Buffer.from(JSON.stringify(mode === "schema" ? { invalid: true } : { ...fixture, version: version() }));
  const signature = sign(null, bytes, privateKey).toString("base64");
  res.writeHead(200, { "content-type": "application/json", "x-omniroute-feed-signature": signature, "x-omniroute-feed-tier": "community" });
  res.end(mode === "tamper" ? Buffer.concat([bytes, Buffer.from(" ")]) : bytes);
});
server.listen(0, "127.0.0.1"); await once(server, "listening");
const config = { network: "loopback-only", port: 0, radar: { feedUrl: "http://127.0.0.1:" + server.address().port, publicKey: publicDer } };
const raw = (url, options = {}) => fetch(i.base + url, { redirect: "manual", ...options, signal: AbortSignal.timeout(35000) });
const sync = () => i.admin("/api/radar/sync", "POST", {});
const readScope = async () => ({ providers: await i.admin("/api/providers"), combos: await i.admin("/api/combos"), keys: await i.admin("/api/keys") });
const waitVersion = async expected => {
  for (let n=0;n<80;n++) { const s=await i.admin("/api/radar/status"); if(s.feeds.catalog.version===expected)return s; await sleep(250); }
  throw Error("RADAR_UPDATE_TIMEOUT");
};
const ageCache = () => {
  const db = new DatabaseSync(path.join(i.dir, "data/storage.sqlite"));
  try { db.prepare("UPDATE radar_feed_cache SET fetched_at=? WHERE id=1").run(new Date(Date.now()-25*60*60*1000).toISOString()); }
  finally { db.close(); }
};
try {
  await check("feed-config-rejects-credentials-private-key-and-unknown-fields", async () => {
    const { validateConfig } = await load("runtime.mjs");
    for (const radar of [{feedUrl:"https://user:secret@example.invalid"},{feedUrl:"file:///x"},{feedUrl:config.radar.feedUrl,publicKey:privateKey.export({type:"pkcs8",format:"der"}).toString("base64")},{...config.radar,unknown:true}])
      assert.throws(() => validateConfig({...config,radar}), /INVALID_RADAR_CONFIG/);
  });
  i = await Instance.create(path.join(run, "one"), undefined, config); instances.push(i); await i.start();
  profile = await i.addProvider("radar-scope", config.radar.feedUrl + "/provider", [{ alias: "one", key: "synthetic-" + randomUUID() }]);
  scope = await readScope();
  await check("default-opt-out-static-baseline-no-network", async () => {
    const free = await i.admin("/api/free-models"); assert(free.models.length>0); assert(!("fetchedAt" in free));
    catalog = await i.admin("/api/radar/catalog"); assert.equal(catalog.meta,null); assert(catalog.entries.every(m=>m.origin==="baseline"));
    status = await i.admin("/api/radar/status"); assert.equal(status.settings.optIn,false); assert.equal(status.feeds.catalog.available,false);
    assert.equal((await sync()).status,"opt_out"); await sleep(500); assert.equal(requests.length,0);
    result.baselineModels=free.models.length;
  });
  await check("admin-session-method-and-adjacent-route-boundaries", async () => {
    for (const p of ["/api/free-models","/api/radar/catalog","/api/radar/status","/api/radar/settings"]) {
      assert.notEqual((await raw(p)).status,200);
      assert.equal((await raw(p,{headers:{authorization:"Bearer "+profile.key}})).status,403);
      assert.equal((await raw(p,{method:"DELETE",headers:{cookie:i.cookie}})).status,403);
    }
    for(const p of ["/api/radar/local-model-state","/api/radar/sync-all","/api/radar/offers","/api/radar/intel","/api/radar/referrals"])
      assert.equal((await raw(p,{headers:{cookie:i.cookie}})).status,404);
  });
  await check("native-cross-site-and-foreign-origin-rejected-before-mutation", async () => {
    for(const p of ["/api/radar/settings","/api/radar/sync"]){
      const body=JSON.stringify(p.endsWith("settings")?{optIn:true}:{});
      assert.equal((await raw(p,{method:"POST",headers:{cookie:i.cookie,"sec-fetch-site":"cross-site","content-type":"application/json"},body})).status,403);
      assert.equal((await raw(p,{method:"POST",headers:{cookie:i.cookie,origin:"https://foreign.invalid","x-omniroute-csrf":i.csrf,"content-type":"application/json"},body})).status,403);
    }
    assert.equal(requests.length,0);
  });
  await check("supporter-key-and-arbitrary-settings-refused-before-side-effects", async () => {
    for(const body of [{optIn:true,supporterKey:"omr_"+"a".repeat(40)},{optIn:true,supporterKey:null},{optIn:true,unknown:true}])
      await assert.rejects(i.admin("/api/radar/settings","POST",body),/ADMIN_400/);
    assert.equal((await i.admin("/api/radar/status")).settings.optIn,false); assert.equal(requests.length,0);
  });
  await check("opt-in-arms-native-scheduler-and-verifies-signed-feed", async () => {
    await i.admin("/api/radar/settings","POST",{optIn:true}); status=await waitVersion(version());
    assert.equal(status.feeds.catalog.generatedAt,fixture.generatedAt);
    assert(Date.parse(status.feeds.catalog.fetchedAt)>Date.parse(fixture.generatedAt));
    assert.equal(status.feeds.catalog.tier,"community");
    catalog=await i.admin("/api/radar/catalog"); assert.equal(catalog.meta.generatedAt,fixture.generatedAt);
    assert(catalog.entries.some(m=>m.displayName==="Synthetic Radar Catalog"));
    result.firstSync={...status.feeds.catalog};
  });
  await check("reads-do-not-sync-or-change-accounts-pools-and-client-keys", async () => {
    const n=pulls(); for(const p of ["/api/free-models","/api/radar/catalog","/api/radar/status","/api/radar/settings"])await i.admin(p);
    assert.equal(pulls(),n); assert.deepEqual(await readScope(),scope);
  });
  await check("same-version-is-stale-without-refreshing-fetched-date", async () => {
    assert.equal((await sync()).status,"stale"); assert.deepEqual((await i.admin("/api/radar/status")).feeds.catalog,status.feeds.catalog);
  });
  for(const [caseMode,expected] of [["older","stale"],["tamper","invalid_signature"],["schema","invalid_schema"],["oversized","too_large"],["http-error","error"]]){
    await check(caseMode+"-keeps-last-good-cache",async()=>{
      mode=caseMode;revision=caseMode==="older"?0:2;
      assert.equal((await sync()).status,expected);
      assert.deepEqual((await i.admin("/api/radar/status")).feeds.catalog,status.feeds.catalog);
    });
  }
  mode="valid";revision=2;
  await check("new-version-updates-native-cache-only",async()=>{
    assert.equal((await sync()).status,"updated");status=await waitVersion(version());
    assert.equal(status.feeds.catalog.generatedAt,fixture.generatedAt);
    assert.deepEqual(await readScope(),scope);
  });
  await check("sync-body-rejects-parameters-and-oversized-input",async()=>{
    const n=pulls();await assert.rejects(i.admin("/api/radar/sync","POST",{url:"https://invalid.example"}),/ADMIN_400/);
    await assert.rejects(i.admin("/api/radar/sync","POST",{x:"x".repeat(1100)}),/ADMIN_413/);assert.equal(pulls(),n);
  });
  await check("opt-out-and-old-cache-remain-readable-without-sync",async()=>{
    await i.admin("/api/radar/settings","POST",{optIn:false});assert.equal((await sync()).status,"opt_out");await i.stop();ageCache();
    const n=pulls();await i.start();await sleep(600);
    assert.equal(pulls(),n);catalog=await i.admin("/api/radar/catalog");assert.equal(catalog.meta.version,version());
    assert(Date.now()-Date.parse(catalog.meta.fetchedAt)>24*60*60*1000);result.oldCacheStillServed=true;
  });
  await check("re-opt-in-and-restart-resume-native-due-sync",async()=>{
    revision=3;await i.admin("/api/radar/settings","POST",{optIn:true});await waitVersion(version());
    await i.stop();ageCache();revision=4;await i.start();status=await waitVersion(version());
    assert.equal(status.settings.optIn,true);assert.equal(status.feeds.catalog.generatedAt,fixture.generatedAt);
  });
  await check("fresh-restart-skips-catalog-download",async()=>{
    await i.stop();const n=pulls();await i.start();await sleep(600);assert.equal(pulls(),n);
  });
  await check("cli-reads-native-status-and-triggers-parameterless-sync",async()=>{
    for(const [method,p,input] of [["GET","/api/radar/status",""],["POST","/api/radar/sync","{}"]]){
      const r=await new Promise((resolve,reject)=>{
        const child=spawn(path.join(pkg,"node.exe"),[path.join(pkg,"tools/cli.mjs"),"admin",i.dir,method,p],{windowsHide:true,timeout:40000,stdio:["pipe","pipe","pipe"]});
        let stdout="";child.stdout.on("data",c=>stdout+=c);child.stderr.resume();child.on("error",reject);
        child.on("close",status=>resolve({status,stdout}));child.stdin.end(input);
      });
      assert.equal(r.status,0);const data=JSON.parse(r.stdout);if(method==="GET")assert.equal(data.feeds.catalog.version,version());else assert.equal(data.status,"stale");
    }
  });
  await check("feed-downloads-send-no-provider-or-client-secrets",async()=>{
    assert(requests.length>0);assert(requests.every(r=>r.method==="GET"&&r.bodyLength===0&&!r.authorization&&!r.cookie));
    assert(requests.filter(r=>r.path==="/v1/catalog/latest").every(r=>r.schema==="2"));
    assert(requests.every(r=>["/v1/catalog/latest","/v1/referrals/latest"].includes(r.path)));
  });
  await check("encrypted-restore-retains-native-cache-dates-and-opt-out",async()=>{
    await i.admin("/api/radar/settings","POST",{optIn:false});status=await i.admin("/api/radar/status");await i.stop();
    const password=randomUUID(),archive=path.join(run,"state.enc");await backup(i,archive,password);
    i=await restore(archive,path.join(run,"restored"),password);instances.push(i);await i.start();
    assert.deepEqual(await i.admin("/api/radar/status"),status);
    const restoredScope=await readScope();
    for(const group of Object.keys(scope)) {
      try { assert.deepEqual(restoredScope[group],scope[group]); }
      catch { throw Error("RESTORE_SCOPE_"+group.toUpperCase()); }
    }
  });
}catch(error){result.fatal=error.code||error.message.slice(0,70);}
finally{
  for(const instance of instances)await check("cleanup-"+instance.meta.id,async()=>assert.equal(await instance.cleanup(),0));
  server.closeAllConnections();await new Promise(r=>server.close(r));
  result.downloads={catalog:pulls(),other:requests.length-pulls(),realProviderCalls:0};
  fs.writeFileSync(path.join(run,"result.json"),JSON.stringify(result,null,2));
  fs.writeFileSync(path.join(root,"evidence/radar-latest.json"),JSON.stringify({run,...result},null,2));
}
process.exitCode=result.fatal||result.cases.some(c=>!c.pass)?1:0;
