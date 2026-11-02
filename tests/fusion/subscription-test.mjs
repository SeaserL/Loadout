// SPDX-License-Identifier: Apache-2.0
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import http from "node:http";
import { once } from "node:events";
import { randomUUID } from "node:crypto";
import { DatabaseSync } from "node:sqlite";
import { Instance, keytar, sleep, backup, restore, load, baseline, compile, applyReady, nativeWait } from "./harness.mjs";
const root = path.resolve(process.env.FUSION_RELEASE_ROOT);
const run = path.join(root, "runs/loadout-fusion-isolated/subscription-" + Date.now());
fs.mkdirSync(run, { recursive: true });
const report = { baseline: baseline(), cases: [] }, instances = [], secrets = [], calls = [];
let generation = 0, refreshes = 0, pollCalls = 0, rejectRefresh = false, issueNearExpiry = false, failSubscription = false;
const accountId = "mock-account-" + randomUUID();
const jwt = () => "e30." + Buffer.from(JSON.stringify({ exp: Math.floor(Date.now()/1000)+3600,
  "https://api.openai.com/auth": { chatgpt_account_id: accountId } })).toString("base64url") + "." + randomUUID();
const tokenSet = () => { const access = jwt(), refresh = "mock-refresh-" + (++generation) + randomUUID();
  secrets.push(access, refresh); return { access_token: access, refresh_token: refresh, expires_in: issueNearExpiry ? 3600 : 172800, id_token: "DISCARD_ID_TOKEN_" + randomUUID() }; };
const mock = http.createServer(async (req, res) => {
  let raw = ""; for await (const c of req) raw += c;
  res.setHeader("content-type", "application/json");
  if (req.url.endsWith("/usercode")) return res.end(JSON.stringify({ device_auth_id: "device", user_code: "SYNTHETIC", interval: 1 }));
  if (req.url.endsWith("/deviceauth/token")) { pollCalls++;
    if (pollCalls === 1) { res.statusCode = 403; return res.end("{}"); }
    return res.end(JSON.stringify({ authorization_code: "code", code_verifier: "verifier" })); }
  if (req.url === "/oauth/token") {
    const form = new URLSearchParams(raw);
    if (form.get("grant_type") === "refresh_token") {
      refreshes++;
      if (rejectRefresh) { res.statusCode = 400; return res.end('{"error":"invalid_grant"}'); }
    }
    return res.end(JSON.stringify(tokenSet()));
  }
  const body = JSON.parse(raw || "{}");
  calls.push({ body, account: req.headers["chatgpt-account-id"], ua: req.headers["user-agent"],
    originator: req.headers.originator, token: req.headers.authorization, path: req.url });
  if (failSubscription && req.headers["chatgpt-account-id"]) { res.statusCode=429; return res.end('{"error":{"message":"temporary limit"}}'); }
  if (req.url.endsWith("/responses")) {
    res.setHeader("content-type", "text/event-stream");
    const response = { id: "resp_mock", object: "response", status: "completed", model: body.model,
      output: [{ id: "msg_mock", type: "message", role: "assistant", status: "completed", content: [{ type: "output_text", text: "mock-ok", annotations: [] }] }],
      usage: { input_tokens: 5, output_tokens: 2, total_tokens: 7 } };
    for (const event of [{ type: "response.created", response: { ...response, status: "in_progress", output: [] } },
      { type: "response.output_text.delta", item_id: "msg_mock", output_index: 0, content_index: 0, delta: "mock-ok" },
      { type: "response.completed", response }]) res.write(`event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`);
    return res.end();
  }
  res.end(JSON.stringify({ id: "mock", object: "chat.completion", model: body.model,
    choices: [{ index: 0, message: { role: "assistant", content: "mock-ok" }, finish_reason: "stop" }], usage: { prompt_tokens: 1, completion_tokens: 1 } }));
});
mock.listen(0, "127.0.0.1"); await once(mock, "listening");
const base = "http://127.0.0.1:" + mock.address().port;
const check = async (name, fn) => { try { await fn(); report.cases.push({ name, pass: true }); }
  catch (e) { report.cases.push({ name, pass: false, error: /^[A-Z_0-9]+$/.test(e.message) ? e.message : e.code || "ASSERTION_FAILED",
    ...(typeof e.actual === "number" ? {actual:e.actual} : {}), ...(typeof e.expected === "number" ? {expected:e.expected} : {}) }); }
  fs.writeFileSync(path.join(run, "result.json"), JSON.stringify(report, null, 2)); console.log(JSON.stringify(report.cases.at(-1))); };
let i, id, provider, key, plan, bindings, source;
const command = (body) => i.admin("/api/loadout/subscription", "POST", body);
try {
  await check("normal-device-login-pending-and-own-vault", async () => {
    i = await Instance.create(path.join(run, "one")); instances.push(i); await i.start();
    const result = await i.admin("/api/provider-nodes", "POST", { type: "openai-compatible", name: "subscription", prefix: "sub", apiType: "responses", baseUrl: base + "/codex" });
    provider = result.node.id;
    issueNearExpiry = true;
    const start = await command({ operation: "start", provider, alias: "main", authBase: base });
    assert.equal((await command({ operation: "status", id: start.id })).state, "pending");
    for (let n = 0; n < 80; n++) { const s = await command({ operation: "status", id: start.id });
      if (s.state === "complete") { id = s.connectionId; break; } if (s.state === "failed") throw Error(s.failure || "DEVICE_LOGIN_FAILED"); await sleep(200); }
    assert.ok(id); const rows = await command({ operation: "list" }); assert.equal(rows[0].transportOptIn, false);
    const vault = await keytar.findCredentials(i.meta.service); assert.equal(vault.length, 2);
    assert.ok(vault.every((x) => !x.password.startsWith("DISCARD_ID_TOKEN")));
  });
  if (!id) throw Error("LOGIN_SETUP_FAILED");
  await check("default-off-and-two-confirmations", async () => {
    await assert.rejects(command({ operation: "transport", id, enabled: true }));
    await assert.rejects(command({ operation: "transport", id, enabled: true, confirmation: { risk: "I understand", alias: "wrong" } }));
    await command({ operation: "transport", id, enabled: true, confirmation: { risk: "I understand", alias: "main" } });
    key = (await i.admin("/api/keys", "POST", { name: "subscription-test", noLog: true, allowedConnections: [id] })).key;
  });
  await check("generic-admin-cannot-bypass-credential-lifecycle", async () => {
    await assert.rejects(i.admin("/api/providers/" + id, "PUT", { apiKey: "must-not-replace" }));
    await assert.rejects(i.admin("/api/providers/" + id, "PUT", { providerSpecificData: { credentialProfile: null } }));
  });
  await check("coalesced-refresh-prunes-old-vault", async () => {
    issueNearExpiry = false;
    await Promise.all([command({ operation: "refresh", id }), command({ operation: "refresh", id })]);
    assert.equal(refreshes, 1); assert.equal((await keytar.findCredentials(i.meta.service)).length, 2);
    assert.ok(!(await keytar.findCredentials(i.meta.service)).some((x) => x.password === secrets[0] || x.password === secrets[1]));
  });
  for (const protocol of ["responses", "chat/completions", "messages"]) await check("subscription-native-" + protocol, async () => {
    const tools = protocol === "responses" ? [{ type: "function", name: "lookup", description: "keep", parameters: { type: "object", properties: { value: { type: "string" } } } }] : undefined;
    const body = { model: provider + "/main/gpt-mock", stream: false,
      ...(protocol === "responses" ? { input: "keep-user", instructions: "keep-instruction", tools } : { messages: [{ role: "user", content: "keep-user" }], max_tokens: 100 }) };
    const response = await fetch(i.base + "/v1/" + protocol, { method: "POST", headers: { "content-type": "application/json", authorization: "Bearer " + key }, body: JSON.stringify(body), signal: AbortSignal.timeout(20000) });
    const text = await response.text(); assert.equal(response.status, 200); assert.ok(text.includes("mock-ok"));
    const last = calls.at(-1); assert.equal(last.body.stream, true); assert.equal(last.body.store, false);
    assert.equal(last.account, accountId); assert.equal(last.ua, "loadout/0.4.0"); assert.equal(last.originator, undefined);
    if (protocol === "responses") { assert.equal(last.body.instructions, "keep-instruction"); assert.deepEqual(last.body.tools, tools); }
  });
  await check("native-old-pool-alias-and-restricted-client", async () => {
    const api = await i.addProvider("api", base + "/v1", [{ alias: "a", key: "synthetic-api" }, { alias: "b", key: "synthetic-api-b" }]);
    source = { version: 1, accountSetCount: 0, accounts: [
      { ref: "codex-sub/account-1", kind: "codex-subscription", enabled: true },
      { ref: "api/account-1", kind: "openai-compatible", enabled: true },
      { ref: "api/account-2", kind: "openai-compatible", enabled: true }],
      pools: [{ name: "gpt", ordering: "manual", members: [{ account: "codex-sub/account-1", model: "gpt-mock" }, { account: "api/account-1", model: "fallback-model" }] }],
      endpoints: [], clientPolicies: [{ allowed_models: ["loadout/default", "api/main/same-model"], requests_per_minute: 8 }] };
    bindings = { "codex-sub/account-1": { provider, connectionId: id, models: ["gpt-mock"], credentialProfile: "loadout-codex-v1" },
      "api/account-1": { provider: api.provider, connectionId: api.connections[0], models: ["same-model", "fallback-model"] },
      "api/account-2": { provider: api.provider, connectionId: api.connections[1], models: ["same-model"] } };
    const migration = { aliases: { "loadout/default": [{ account: "api/account-1", model: "same-model" }], "api/main/same-model": [{ account: "api/account-1", model: "same-model" }] } };
    plan = compile(source, bindings, {}, "native-bounded-v1", migration); assert.equal(plan.activationReady, true);
    await i.admin("/api/resilience", "PATCH", nativeWait());
    await applyReady(i, plan, { source, bindings });
    const client = await keytar.getPassword(i.meta.bootstrap, "route-legacy-client-1");
    for (const model of ["loadout/default", "api/main/same-model"]) {
      const response = await fetch(i.base + "/v1/chat/completions", { method: "POST", headers: { "content-type": "application/json", authorization: "Bearer " + client },
        body: JSON.stringify({ model, messages: [{ role: "user", content: "hello" }] }) });
      assert.equal(response.status, 200); await response.text(); assert.equal(calls.at(-1).body.model, "same-model");
    }
    const before = calls.length;
    const denied = await fetch(i.base + "/v1/chat/completions", { method: "POST", headers: { "content-type": "application/json", authorization: "Bearer " + client },
      body: JSON.stringify({ model: "loadout/gpt", messages: [{ role: "user", content: "denied" }] }) });
    assert.equal(denied.status, 403); assert.equal(calls.length, before);
  });
  await check("unknown-alias-and-uncontrolled-binding-refused", async () => {
    assert.throws(() => compile(source, bindings, {}, "native-bounded-v1", { aliases: {} }), /CLIENT_ALIAS_MAPPING_REQUIRED/);
    const changed = structuredClone(bindings); delete changed["codex-sub/account-1"].credentialProfile;
    assert.equal(compile(source, changed, {}, "native-bounded-v1", plan.migration).activationReady, false);
  });
  await check("actual-old-pools-complete-metadata-plan", async () => {
    const actual=JSON.parse(fs.readFileSync(new URL("./fixtures/legacy-pools.json",import.meta.url),"utf8"));
    const mapped={};
    for(const a of actual.accounts){
      const models=actual.accountModels.filter(m=>m.account===a.ref).map(m=>m.model);
      if(!models.length)continue;
      mapped[a.ref]={provider:"synthetic-"+a.provider,connectionId:"synthetic-"+a.ref,models,
        ...(a.kind==="codex-subscription"?{credentialProfile:"loadout-codex-v1"}:{})};
    }
    const deep=actual.accounts.find(a=>a.provider==="deepseek").ref;
    const wb=actual.accounts.find(a=>a.kind==="workbuddy-bridge").ref;
    const aliases={"loadout/default":[{account:deep,model:"deepseek-flash"}],
      "deepseek/main/deepseek-flash":[{account:deep,model:"deepseek-flash"}],
      "workbuddy/main/deepseek-v4.1-flash":[{account:wb,model:"deepseek-v4.1-flash"}]};
    const models=Object.fromEntries(actual.endpoints.map(e=>[e.name,[...new Set(actual.accountModels.filter(m=>m.account.startsWith(e.provider+"/")).map(m=>m.model))]]));
    const full=compile(actual,mapped,models,"native-bounded-v1",{aliases});
    assert.equal(full.activationReady,true);assert.equal(full.routes.find(r=>r.name==="pool-gpt").native.models.length,3);
    assert.equal(full.routes.filter(r=>r.kind==="endpoint").length,5);
    assert.ok(full.routes.filter(r=>r.kind==="endpoint").every(r=>r.native.models.length===12));
    assert.equal(full.clients[0].rateLimits[0].limit,8);
    assert.equal(full.accountInventory.length,actual.accounts.length);
    // Synthetic bindings and explicit default input prove compiler coverage, not a live default decision.
  });
  await check("explicit-gpt-pool-failover-only-to-listed-model", async () => {
    failSubscription=true;
    const poolKey=await keytar.getPassword(i.meta.bootstrap,"route-pool-gpt"), before=calls.length;
    const response=await fetch(i.base+"/v1/chat/completions",{method:"POST",headers:{"content-type":"application/json",authorization:"Bearer "+poolKey},
      body:JSON.stringify({model:"loadout/gpt",messages:[{role:"user",content:"pool"}] }),signal:AbortSignal.timeout(20000)});
    await response.text(); failSubscription=false;
    report.poolProbe={status:response.status,attempts:calls.slice(before).map(c=>({model:c.body.model,subscription:!!c.account}))};
    assert.equal(response.status,200);
    assert.equal(calls.at(-1).body.model,"fallback-model");
    assert.ok(calls.slice(before).every(c=>["gpt-mock","fallback-model"].includes(c.body.model)));
  });
  await check("legacy-alias-strict-pin-and-native-eight-rpm",async()=>{
    const policy=plan.clients[0];
    const stored=(await i.admin("/api/keys")).keys.find(k=>k.name===policy.name);
    assert.deepEqual(stored.rateLimits,policy.rateLimits);
    const fresh=await i.admin("/api/keys","POST",{...policy,name:"alias-rpm"});
    await i.admin("/api/keys/"+fresh.id,"PATCH",{rateLimits:policy.rateLimits});
    for(let n=0;n<9;n++){
      const before=calls.length;
      const response=await fetch(i.base+"/v1/chat/completions",{method:"POST",headers:{"content-type":"application/json",authorization:"Bearer "+fresh.key},
        body:JSON.stringify({model:"api/main/same-model",allow_account_failover:false,messages:[{role:"user",content:"bounded"}]})});
      await response.text(); (report.strictStatuses??=[]).push(response.status); assert.equal(response.status,n===8?429:200);
      if(n===8)assert.equal(calls.length,before);else assert.equal(calls.at(-1).body.model,"same-model");
    }
  });
  await check("encrypted-restart-restore-keeps-login-and-aliases", async () => {
    await i.stop(); const archive = path.join(run, "backup.enc"); const password = randomUUID();
    await backup(i, archive, password);
    const restored = await restore(archive, path.join(run, "restored"), password); instances.push(restored);
    await restored.start(); const rows = await restored.admin("/api/loadout/subscription", "POST", { operation: "list" });
    assert.equal(rows[0].id, id); assert.equal(rows[0].transportOptIn, true);
    assert.equal((await restored.admin("/api/model-combo-mappings")).mappings.length, 3);
    await restored.stop(); await i.start();
  });
  await check("invalid-grant-stops-retry-and-disables-routing", async () => {
    await i.stop(); const db = new DatabaseSync(path.join(i.dir, "data/storage.sqlite"));
    db.prepare("UPDATE provider_connections SET expires_at=? WHERE id=?").run(new Date(Date.now()+3600000).toISOString(), id); db.close();
    rejectRefresh = true; await i.start();
    for (let n = 0; n < 40; n++) { if ((await command({ operation: "list" }))[0].loginState === "needs_login") break; await sleep(100); }
    assert.equal((await command({ operation: "list" }))[0].loginState, "needs_login"); const before = refreshes;
    await command({ operation: "refresh", id }); assert.equal(refreshes, before);
  });
  await check("native-delete-cleans-access-and-refresh", async () => {
    const before = (await keytar.findCredentials(i.meta.service)).length;
    await i.admin("/api/providers/" + id, "DELETE");
    assert.equal((await keytar.findCredentials(i.meta.service)).length, before-2);
  });
  await check("instance-files-contain-no-tokens-or-request-body",async()=>{
    for(const instance of instances)await instance.stop();
    const forbidden=[...secrets,"keep-instruction","keep-user"];
    function walk(dir){for(const e of fs.readdirSync(dir,{withFileTypes:true})){
      const p=path.join(dir,e.name);if(e.isDirectory())walk(p);else if(e.isFile()){
        const bytes=fs.readFileSync(p);assert.ok(forbidden.every(value=>!bytes.includes(Buffer.from(value))));
      }
    }}
    for(const instance of instances)walk(instance.dir);
  });
} catch (e) { report.setupFailure = e.message === "LOGIN_SETUP_FAILED" ? e.message : "SETUP_FAILED"; }
finally {
  for (const instance of instances) await check("cleanup-" + instance.meta.id, async () => assert.equal(await instance.cleanup(), 0));
  mock.closeAllConnections(); await new Promise((r) => mock.close(r));
  // Never persist captured Authorization headers, raw auth responses or request bodies.
  fs.mkdirSync(path.join(root, "evidence"), { recursive: true });
  fs.writeFileSync(path.join(root, "evidence/subscription-latest.json"), JSON.stringify({ run, ...report }, null, 2));
}
process.exitCode = !report.setupFailure && report.cases.every((c) => c.pass) ? 0 : 1;
