// SPDX-License-Identifier: Apache-2.0
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import http from "node:http";
import { once } from "node:events";
import { randomUUID } from "node:crypto";
import { DatabaseSync } from "node:sqlite";
import { Instance, sleep, baseline, backup, restore } from "./harness.mjs";

const root = path.resolve(process.env.FUSION_RELEASE_ROOT);
const run = path.join(root, "runs/loadout-fusion-isolated/quota-model-" + Date.now());
fs.mkdirSync(run, { recursive: true });
const result = { baseline: baseline(), cases: [] }, instances = [];
const secrets = ["synthetic-a-" + randomUUID(), "synthetic-b-" + randomUUID()];
const bodySentinel = "BODY_" + randomUUID();
const calls = [], discovery = [];
let models = { a: ["same-model", "a-only"], b: ["same-model", "b-only"] };
let failDiscovery = false, i, profile, limited, unknown, stage;
const check = async (name, fn) => {
  stage = name;
  await fn();
  result.cases.push({ name, pass: true });
  fs.writeFileSync(path.join(run, "result.json"), JSON.stringify(result, null, 2));
  console.log(name + " PASS");
};
const mock = http.createServer(async (req, res) => {
  const account = req.headers.authorization === "Bearer " + secrets[0] ? "a" :
    req.headers.authorization === "Bearer " + secrets[1] ? "b" : "invalid";
  res.setHeader("content-type", "application/json");
  if (req.method === "GET") {
    discovery.push(account);
    if (failDiscovery) { res.statusCode = 503; return res.end('{"error":{"message":"synthetic unavailable"}}'); }
    return res.end(JSON.stringify({ object: "list", data: (models[account] || []).map(id => ({ id, object: "model" })) }));
  }
  let raw = ""; for await (const c of req) raw += c;
  const body = JSON.parse(raw);
  calls.push({ account, model: body.model });
  if (!(models[account] || []).includes(body.model) && body.model !== "unpriced-model") {
    res.statusCode = 404; return res.end('{"error":{"message":"model not found"}}');
  }
  res.end(JSON.stringify({ id: "synthetic", object: "chat.completion", model: body.model,
    choices: [{ index: 0, message: { role: "assistant", content: "ok" }, finish_reason: "stop" }],
    usage: { prompt_tokens: 1000, completion_tokens: 500, total_tokens: 1500 } }));
});
mock.listen(0, "127.0.0.1"); await once(mock, "listening");
const request = (p, options = {}) => fetch(i.base + p, { redirect: "manual", ...options, signal: AbortSignal.timeout(30000) });
const ask = (model, key = profile.key, extra = {}) => request("/v1/chat/completions", {
  method: "POST", headers: { "content-type": "application/json", authorization: "Bearer " + key },
  body: JSON.stringify({ model, messages: [{ role: "user", content: bodySentinel }], ...extra }),
});
const sync = id => i.admin("/api/providers/" + id + "/sync-models", "POST", {});
const catalog = () => i.admin("/api/synced-available-models?provider=" + profile.provider);
const newKey = async name => {
  const key = await i.admin("/api/keys", "POST", { name, noLog: true, allowedConnections: [profile.connections[0]] });
  secrets.push(key.key); return key;
};
try {
  i = await Instance.create(path.join(run, "one")); instances.push(i); await i.start();
  profile = await i.addProvider("quota-model", "http://127.0.0.1:" + mock.address().port + "/v1",
    [{ alias: "a", key: secrets[0] }, { alias: "b", key: secrets[1] }]);
  secrets.push(profile.key);
  limited = await newKey("limited"); unknown = await newKey("unknown-price");
  await i.admin("/api/combos", "POST", { name: "account-a-pool", strategy: "priority",
    models: [{ kind: "model", provider: profile.provider, model: "same-model", connectionId: profile.connections[0] }] });
  await check("new-management-routes-session-only", async () => {
    for (const p of ["/api/usage/quota", "/api/usage/provider-limits", "/api/usage/budget?apiKeyId=" + limited.id,
      "/api/keys/" + limited.id + "/usage-limits", "/api/synced-available-models", "/api/providers/" + profile.connections[0] + "/models"]) {
      assert.notEqual((await request(p)).status, 200);
      assert.equal((await request(p, { headers: { authorization: "Bearer " + profile.key } })).status, 403);
    }
  });
  await check("quota-readonly-and-write-origin-boundaries", async () => {
    for (const p of ["/api/usage/quota", "/api/usage/provider-limits", "/api/keys/" + limited.id + "/usage-limits", "/api/synced-available-models"])
      assert.equal((await request(p, { method: "POST", headers: { cookie: i.cookie } })).status, 403);
    for (const p of ["/api/usage/budget", "/api/providers/" + profile.connections[0] + "/sync-models"])
      for (const headers of [{ authorization: "Bearer " + profile.key }, { cookie: i.cookie, origin: "http://foreign.invalid" }, { cookie: i.cookie, "sec-fetch-site": "cross-site" }])
        assert.equal((await request(p, { method: "POST", headers })).status, 403);
    assert.equal((await request("/api/quota/pools", { headers: { cookie: i.cookie } })).status, 404);
  });
  await check("quota-unknown-is-not-a-real-balance-and-no-refresh", async () => {
    const n = discovery.length, q = await i.admin("/api/usage/quota?connectionId=" + profile.connections[0]);
    assert.equal(q.providers.length, 1); assert.equal(q.providers[0].connectionId, profile.connections[0]);
    assert.equal(q.providers[0].quotaTotal, null);
    assert.deepEqual((await i.admin("/api/usage/provider-limits")).caches, {});
    assert.equal(discovery.length, n); result.emptyQuota = { total: q.providers[0].quotaTotal, nativePercent: q.providers[0].percentRemaining };
  });
  await check("native-budget-validation", async () => {
    await assert.rejects(i.admin("/api/usage/budget", "POST", { apiKeyId: limited.id, dailyLimitUsd: -1 }), /ADMIN_400/);
    await assert.rejects(i.admin("/api/keys/" + limited.id, "PATCH", { usageLimitEnabled: true, dailyUsageLimitUsd: -1 }), /ADMIN_400/);
    await i.admin("/api/keys/" + limited.id, "PATCH", { usageLimitEnabled: true, dailyUsageLimitUsd: 0 });
    // Zero clears this window's limit; it does not ban the key.
    assert.equal((await i.admin("/api/keys/" + limited.id + "/usage-limits")).key.dailyUsageLimitUsd, null);
    await i.admin("/api/keys/" + limited.id, "PATCH", { usageLimitEnabled: false });
  });
  // Public metadata fixtures only: no new pricing or quota writer in the product.
  await i.stop();
  const db = new DatabaseSync(path.join(i.dir, "data/storage.sqlite"));
  try {
    const put = db.prepare("INSERT OR REPLACE INTO key_value(namespace,key,value) VALUES(?,?,?)");
    put.run("pricing_synced", profile.provider, JSON.stringify({ "same-model": { input: 2, output: 4 } }));
    put.run("providerLimitsCache", profile.connections[0], JSON.stringify({ quotas: { daily: { used: 2, limit: 10, remaining: 8 } }, plan: "synthetic", message: null,
      source: "manual", fetchedAt: "2020-01-01T00:00:00Z" }));
  } finally { db.close(); }
  await i.start();
  await check("provider-cache-keeps-source-age-and-does-not-refresh", async () => {
    const n = discovery.length, c = (await i.admin("/api/usage/provider-limits")).caches[profile.connections[0]];
    assert.equal(c.fetchedAt, "2020-01-01T00:00:00Z"); assert.equal(c.source, "manual"); assert.equal(c.quotas.daily.remaining, 8);
    assert.equal(discovery.length, n); result.cacheFixture = { source: c.source, fetchedAt: c.fetchedAt, synthetic: true };
  });
  await check("discovery-resolves-vault-credential-for-selected-account", async () => {
    const n = discovery.length;
    const d = await i.admin("/api/providers/" + profile.connections[0] + "/models?refresh=true");
    assert(d.models.some(m => m.id === "a-only")); assert(!d.models.some(m => m.id === "b-only"));
    assert.deepEqual(discovery.slice(n), ["a"]);
  });
  await check("sync-merges-account-catalogs-without-changing-pool", async () => {
    const before = await i.admin("/api/combos");
    assert.equal((await sync(profile.connections[0])).ok, true); assert.equal((await sync(profile.connections[1])).ok, true);
    const c = await catalog(); assert(c.models.some(m => m.id === "a-only")); assert(c.models.some(m => m.id === "b-only"));
    assert.deepEqual(await i.admin("/api/combos"), before);
  });
  await check("sync-removes-only-selected-account-stale-models", async () => {
    models.a = ["same-model", "new-a"];
    const updated = await sync(profile.connections[0]); assert.equal(updated.ok, true);
    const c = await catalog(); assert(c.models.some(m => m.id === "new-a")); assert(c.models.some(m => m.id === "b-only")); assert(!c.models.some(m => m.id === "a-only"));
    assert.equal(updated.prunedComboRefs.length, 0);
  });
  await check("failed-sync-retains-last-good-catalog", async () => {
    const before = await catalog(); failDiscovery = true;
    const response = await request("/api/providers/" + profile.connections[0] + "/sync-models", { method: "POST", headers: { cookie: i.cookie, origin: i.base } });
    assert(response.status >= 400); failDiscovery = false;
    assert.deepEqual(await catalog(), before);
  });
  await check("sync-does-not-expand-key-account-scope", async () => {
    const n = calls.length;
    // Named account is preferred unless strict pinning is explicitly requested.
    const r = await ask(profile.model("b", "same-model"), limited.key); assert.equal(r.status, 200); await r.text();
    assert.deepEqual(calls.slice(n), [{ account: "a", model: "same-model" }]);
    const after = calls.length;
    const strict = await ask(profile.model("b", "same-model"), limited.key, { allow_account_failover: false });
    assert(strict.status >= 400); await strict.text(); assert.equal(calls.length, after);
  });
  await check("pool-and-exact-model-still-use-selected-account", async () => {
    for (const model of ["account-a-pool", profile.model("a", "same-model")]) {
      const n = calls.length, r = await ask(model, limited.key); assert.equal(r.status, 200); await r.text();
      assert.deepEqual(calls.slice(n), [{ account: "a", model: "same-model" }]);
    }
  });
  await check("removed-model-never-silently-substitutes", async () => {
    const n = calls.length, r = await ask(profile.model("a", "a-only"), limited.key); assert(r.status >= 400); await r.text();
    assert(calls.slice(n).every(c => c.account === "a" && c.model === "a-only"));
  });
  await sleep(2500);
  await check("native-key-spend-links-to-price-catalog", async () => {
    const s = (await i.admin("/api/keys/" + limited.id + "/usage-limits")).status;
    assert(Math.abs(s.dailySpentUsd - 0.012) < 1e-8); assert.equal(s.dailyHasUnpricedUsage, false);
    result.cost = s.dailySpentUsd;
  });
  await check("budget-blocks-before-upstream-and-can-be-raised", async () => {
    await i.admin("/api/usage/budget", "POST", { apiKeyId: limited.id, dailyLimitUsd: 0.001 });
    const started = Date.now(); let b;
    do {
      b = await i.admin("/api/usage/budget?apiKeyId=" + limited.id);
      if (!b.budgetCheck.allowed) break;
      await sleep(1000);
    } while (Date.now() - started < 75000);
    result.budget = { observedWaitMs: Date.now() - started, periodUsed: b.budgetCheck.periodUsed, allowed: b.budgetCheck.allowed };
    assert.equal(b.budgetCheck.allowed, false);
    const n = calls.length, r = await ask("account-a-pool", limited.key); assert.equal(r.status, 429);
    const error = await r.text(); result.budget.hasBudgetMessage = /budget/i.test(error);
    assert.equal(calls.length, n);
    await i.admin("/api/usage/budget", "POST", { apiKeyId: limited.id, dailyLimitUsd: 10 });
  });
  await check("daily-key-limit-blocks-before-upstream", async () => {
    await i.admin("/api/keys/" + limited.id, "PATCH", { usageLimitEnabled: true, dailyUsageLimitUsd: 0.001, weeklyUsageLimitUsd: 10 });
    assert.equal((await i.admin("/api/keys/" + limited.id + "/usage-limits")).status.dailyExceeded, true);
    const n = calls.length, r = await ask(profile.model("a", "same-model"), limited.key); assert.equal(r.status, 429); await r.text(); assert.equal(calls.length, n);
  });
  await check("weekly-key-limit-blocks-before-upstream", async () => {
    await i.admin("/api/keys/" + limited.id, "PATCH", { dailyUsageLimitUsd: 10, weeklyUsageLimitUsd: 0.001 });
    assert.equal((await i.admin("/api/keys/" + limited.id + "/usage-limits")).status.weeklyExceeded, true);
    const n = calls.length, r = await ask("account-a-pool", limited.key); assert.equal(r.status, 429); await r.text(); assert.equal(calls.length, n);
  });
  await check("native-unpriced-usage-fails-closed-on-next-request", async () => {
    await i.admin("/api/keys/" + unknown.id, "PATCH", { usageLimitEnabled: true, dailyUsageLimitUsd: 10 });
    const first = await ask(profile.model("a", "unpriced-model"), unknown.key); assert.equal(first.status, 200); await first.text(); await sleep(2500);
    const s = (await i.admin("/api/keys/" + unknown.id + "/usage-limits")).status;
    assert.equal(s.dailyHasUnpricedUsage, true); assert.equal(s.dailyExceeded, true);
    const n = calls.length, second = await ask(profile.model("a", "same-model"), unknown.key); assert.equal(second.status, 429); await second.text(); assert.equal(calls.length, n);
  });
  await check("restart-preserves-budget-and-models", async () => {
    await i.stop(); await i.start(); assert((await catalog()).models.some(m => m.id === "new-a"));
    assert.equal((await i.admin("/api/keys/" + limited.id + "/usage-limits")).status.weeklyExceeded, true);
    assert.equal((await i.admin("/api/usage/budget?apiKeyId=" + limited.id)).dailyLimitUsd, 10);
  });
  await check("encrypted-restore-preserves-limits-and-catalog", async () => {
    await i.stop(); const archive = path.join(run, "state.enc"), password = randomUUID(); await backup(i, archive, password);
    i = await restore(archive, path.join(run, "restored"), password); instances.push(i); await i.start();
    assert((await catalog()).models.some(m => m.id === "new-a"));
    const n = calls.length, r = await ask("account-a-pool", limited.key); assert.equal(r.status, 429); await r.text(); assert.equal(calls.length, n);
  });
  await check("credentials-bodies-and-client-profiles-not-written", async () => {
    for (const instance of instances) {
      await instance.stop();
      const walk = dir => { for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
        const p = path.join(dir, e.name);
        assert(![".codex", ".claude"].includes(e.name));
        if (e.isDirectory()) walk(p); else {
          const bytes = fs.readFileSync(p);
          for (const s of [...secrets, bodySentinel]) { assert(!bytes.includes(Buffer.from(s)), "SECRET_DISK"); assert(!bytes.includes(Buffer.from(s, "utf16le")), "SECRET_DISK_UTF16"); }
        }
      }}; walk(instance.dir);
    }
  });
} catch (e) {
  result.fatal = { stage, code: e.code || e.message.slice(0, 90), ...(typeof e.actual === "number" ? { actual: e.actual } : {}), ...(typeof e.expected === "number" ? { expected: e.expected } : {}) };
  console.log(JSON.stringify(result.fatal));
} finally {
  failDiscovery = false;
  for (const instance of instances) await check("cleanup-" + instance.meta.id, async () => assert.equal(await instance.cleanup(), 0));
  mock.closeAllConnections(); await new Promise(r => mock.close(r));
  fs.writeFileSync(path.join(run, "result.json"), JSON.stringify(result, null, 2));
  fs.writeFileSync(path.join(root, "evidence/quota-model-latest.json"), JSON.stringify({ run, ...result }, null, 2));
}
process.exitCode = result.fatal ? 1 : 0;
