// SPDX-License-Identifier: Apache-2.0
import fs from "node:fs";
import path from "node:path";
import http from "node:http";
import { once } from "node:events";
import { randomUUID } from "node:crypto";
import {
  Instance,
  runs,
  backup,
  restore,
  baseline,
  sha,
  keytar,
} from "./harness.mjs";
import { compile, applyReady, nativeWait } from "./harness.mjs";
import { configure, conservativeTraffic } from "./harness.mjs";
const dir = path.join(runs, "final-pacing-" + Date.now());
fs.mkdirSync(dir, { recursive: true });
const result = {
  started: new Date().toISOString(),
  baseline: baseline(),
  cases: [],
  observations: [],
};
const tokens = new Map(),
  calls = [];
let counter = 0,
  instance,
  recovered,
  pacedId;
const mock = http.createServer(async (req, res) => {
  if (req.method === "GET") {
    res.writeHead(200, { "content-type": "application/json" });
    res.end(
      JSON.stringify({
        data: [
          "model-a",
          "model-b",
          "same-model",
          "gpt-6-astra",
          "gpt-6-luna",
          "deepseek-v4.1-flash",
        ].map((id) => ({ id, object: "model" })),
      }),
    );
    return;
  }
  const parts = [];
  for await (const c of req) parts.push(c);
  const body = JSON.parse(Buffer.concat(parts));
  const account = tokens.get(
    (req.headers.authorization || "").replace("Bearer ", ""),
  );
  if (!account) {
    res.writeHead(401);
    res.end();
    return;
  }
  account.count++;
  calls.push({
    account: account.name,
    model: body.model,
    at: Date.now(),
    path: req.url,
  });
  const code =
    account.code ?? (account.count <= (account.failCount ?? 0) ? 429 : 200);
  if (code !== 200) {
    if (account.delay) await new Promise((r) => setTimeout(r, account.delay));
    res.writeHead(code, {
      "content-type": "application/json",
      "retry-after": String(account.retryAfter ?? 2),
    });
    res.end(
      JSON.stringify({
        error: {
          message: code === 401 ? "invalid credential" : "temporary rate limit",
          type: code === 401 ? "authentication_error" : "rate_limit_error",
        },
      }),
    );
    return;
  }
  const reply = {
    id: "chat-test",
    object: "chat.completion",
    model: body.model,
    choices: [
      {
        index: 0,
        message: { role: "assistant", content: account.name },
        finish_reason: "stop",
      },
    ],
    usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 },
  };
  if (!body.stream) {
    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify(reply));
    return;
  }
  res.writeHead(200, { "content-type": "text/event-stream" });
  res.end(
    "data: " +
      JSON.stringify({
        id: "chat-test",
        object: "chat.completion.chunk",
        model: body.model,
        choices: [
          {
            index: 0,
            delta: { role: "assistant", content: account.name },
            finish_reason: null,
          },
        ],
      }) +
      "\n\ndata: " +
      JSON.stringify({
        id: "chat-test",
        object: "chat.completion.chunk",
        model: body.model,
        choices: [{ index: 0, delta: {}, finish_reason: "stop" }],
      }) +
      "\n\ndata: [DONE]\n\n",
  );
});
mock.listen(0, "127.0.0.1");
await once(mock, "listening");
const save = () =>
  fs.writeFileSync(
    path.join(dir, "result.json"),
    JSON.stringify(result, null, 2),
  );
async function check(name, fn) {
  if (
    process.argv[2] &&
    !name.includes(process.argv[2]) &&
    !name.startsWith("cleanup-")
  )
    return;
  try {
    result.cases.push({ name, ...(await fn()) });
  } catch (e) {
    result.cases.push({ name, pass: false, error: e.message });
  }
  save();
  console.log(JSON.stringify(result.cases.at(-1)));
}
async function waitConfig(enabled, maxWaitMs = 30000, budgetMs = 30000) {
  return instance.admin("/api/resilience", "PATCH", {
    requestQueue: { ...nativeWait().requestQueue, minTimeBetweenRequestsMs: 0 },
    waitForCooldown: { enabled: false, maxRetries: 0 },
    comboCooldownWait: { enabled, maxWaitMs, maxAttempts: 2, budgetMs },
    connectionCooldown: {
      apikey: {
        baseCooldownMs: 1000,
        useUpstreamRetryHints: true,
        maxBackoffSteps: 1,
      },
    },
  });
}
async function make(
  states,
  { endpoint = false, waitSecs = 0, models, rawWait = false } = {},
) {
  const name = "case-" + ++counter,
    entries = states.map((s, i) => {
      const key = "fake-" + randomUUID();
      tokens.set(key, { name: name + "-" + i, count: 0, ...s });
      return { alias: "a" + i, key };
    });
  const profile = await instance.addProvider(
    name,
    `http://127.0.0.1:${mock.address().port}/v1`,
    entries,
  );
  const accounts = entries.map((a, i) => ({
    ref: name + "/" + a.alias,
    provider: name,
    enabled: true,
  }));
  const bindings = Object.fromEntries(
    accounts.map((a, i) => [
      a.ref,
      {
        provider: profile.provider,
        connectionId: profile.connections[i],
        models: ["model-a", "model-b", "same-model"],
      },
    ]),
  );
  const source = {
    version: 1,
    accounts,
    accountSetCount: 0,
    clientPolicies: [{}],
    pools: endpoint
      ? []
      : [
          {
            name,
            ordering: "manual",
            members: accounts.map((a, i) => ({
              account: a.ref,
              model: models?.[i] ?? "same-model",
            })),
          },
        ],
    endpoints: endpoint
      ? [
          {
            name,
            provider: name,
            members: null,
            waitSecs,
            rotate: false,
            defaultModel: null,
          },
        ]
      : [],
  };
  const plan = compile(
    source,
    bindings,
    endpoint ? { [name]: ["same-model"] } : {},
    "native-bounded-v1",
  );
  const route = plan.routes[0];
  let key;
  if (rawWait) {
    const combo = structuredClone(route.native);
    combo.config.maxGlobalAttempts = 100;
    combo.config.comboTimeoutMs = waitSecs * 1000;
    await instance.admin("/api/combos", "POST", combo);
    key = await instance.admin("/api/keys", "POST", {
      name: route.name,
      noLog: true,
      allowedConnections: profile.connections,
      modelAccessMode: "restricted",
      allowedCombos: [route.name],
    });
  } else {
    key = (await applyReady(instance, plan, { source, bindings })).keys[0];
  }
  return { plan, source, bindings, route, key, profile, entries };
}
async function request(
  c,
  {
    protocol = "chat/completions",
    model = c.route.name,
    key = c.key.key,
    signal,
    stream = false,
  } = {},
) {
  const offset = calls.length,
    start = performance.now();
  const body =
    protocol === "responses"
      ? { model, input: "TEST", store: false, stream }
      : {
          model,
          messages: [{ role: "user", content: "TEST" }],
          max_tokens: 30,
          stream,
        };
  const r = await fetch(instance.base + "/v1/" + protocol, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      authorization: "Bearer " + key,
      "anthropic-version": "2023-06-01",
    },
    body: JSON.stringify(body),
    signal: signal ?? AbortSignal.timeout(45000),
  });
  const text = await r.text();
  return {
    status: r.status,
    ms: performance.now() - start,
    calls: calls.slice(offset),
    error: r.status >= 400 ? text.slice(0, 240) : undefined,
  };
}
try {
  instance = await Instance.create(path.join(dir, "instance"));
  await instance.start();
  await instance.settings();
  await waitConfig(true);
  for (const protocol of ["chat/completions", "responses", "messages"]) {
    await check("bounded-recovery-" + protocol, async () => {
      const c = await make([{ failCount: 1, retryAfter: 2 }], {
        endpoint: true,
        waitSecs: 30,
      });
      const r = await request(c, { protocol });
      return {
        ...r,
        pass:
          r.status === 200 &&
          r.calls.length === 2 &&
          r.calls.every((x) => x.model === "same-model") &&
          r.ms < 8000,
      };
    });
  }
  await check("two-waits-three-dispatches-maximum", async () => {
    const c = await make([{ code: 429, retryAfter: 2 }], {
      endpoint: true,
      waitSecs: 30,
    });
    const r = await request(c);
    return {
      ...r,
      pass:
        r.status !== 200 &&
        r.calls.length === 3 &&
        r.ms >= 4000 &&
        r.ms < 10000,
    };
  });
  await check("cumulative-wait-budget-thirty-seconds", async () => {
    const c = await make([{ code: 429, retryAfter: 16 }], {
      endpoint: true,
      waitSecs: 30,
    });
    const r = await request(c);
    return {
      ...r,
      pass:
        r.status !== 200 &&
        r.calls.length === 2 &&
        r.ms >= 16000 &&
        r.ms < 23000,
    };
  });
  for (const state of [{ code: 429, retryAfter: 60 }, { code: 401 }]) {
    await check("no-wait-" + state.code, async () => {
      const c = await make([state], { endpoint: true, waitSecs: 30 });
      const r = await request(c);
      return {
        ...r,
        pass: r.status !== 200 && r.calls.length === 1 && r.ms < 2000,
      };
    });
  }
  await check("wait-retry-scope-does-not-escape", async () => {
    const c = await make([{ code: 429, retryAfter: 2 }, {}], {
      endpoint: true,
      waitSecs: 30,
    });
    const scoped = await instance.admin("/api/keys", "POST", {
      name: "restricted-wait",
      noLog: true,
      allowedConnections: [c.profile.connections[0]],
      modelAccessMode: "restricted",
      allowedModels: [],
      allowedCombos: [c.route.name],
    });
    const r = await request(c, { key: scoped.key });
    const bypass = await request(c, {
      key: scoped.key,
      model: c.profile.model("a1", "same-model"),
    });
    return {
      ...r,
      pass:
        r.status !== 200 &&
        r.calls.length <= 3 &&
        r.calls.length > 0 &&
        r.calls.every(
          (x) => x.account.endsWith("-0") && x.model === "same-model",
        ) &&
        bypass.status !== 200 &&
        bypass.calls.length === 0,
    };
  });
  await check("cancel-wait-no-later-dispatch", async () => {
    const c = await make([{ failCount: 1, retryAfter: 3 }], {
      endpoint: true,
      waitSecs: 30,
    });
    const offset = calls.length,
      abort = new AbortController();
    const pending = request(c, { signal: abort.signal }).catch(() => null);
    while (calls.length === offset) await new Promise((r) => setTimeout(r, 20));
    abort.abort();
    await pending;
    await new Promise((r) => setTimeout(r, 3600));
    return { pass: calls.length === offset + 1 };
  });
  const retained = await make([{}], { endpoint: true, waitSecs: 30 });
  await check("altered-plan-denied", async () => {
    const p = structuredClone(retained.plan);
    p.resilience.comboCooldownWait.maxAttempts = 10;
    try {
      await applyReady(instance, p, {
        source: retained.source,
        bindings: retained.bindings,
      });
      return { pass: false };
    } catch (e) {
      return { pass: e.message === "MODIFIED_MAPPING" };
    }
  });
  await check("incompatible-global-policy-denied", async () => {
    await instance.admin("/api/resilience", "PATCH", {
      comboCooldownWait: { maxAttempts: 10 },
    });
    try {
      await make([{}]);
      return { pass: false };
    } catch (e) {
      return { pass: e.message === "NATIVE_WAIT_PROFILE_MISMATCH" };
    } finally {
      await waitConfig(true);
    }
  });
  await check("no-positive-retry-hint-does-not-force-retry", async () => {
    await instance.admin("/api/resilience", "PATCH", {
      connectionCooldown: {
        apikey: {
          baseCooldownMs: 20000,
          useUpstreamRetryHints: true,
          maxBackoffSteps: 1,
        },
      },
    });
    const c = await make([{ failCount: 1, retryAfter: 0 }], {
      endpoint: true,
      waitSecs: 30,
    });
    const r = await request(c);
    return {
      ...r,
      pass: r.status !== 200 && r.calls.length === 1 && r.ms < 2000,
      legacyTwentySecondRecoveryProven: false,
    };
  });
  await waitConfig(true);
  await check("native-per-account-pacing-twenty-seconds", async () => {
    const c = await make([{}], { endpoint: true, waitSecs: 30 });
    pacedId = c.profile.connections[0];
    await configure(instance, [pacedId]);
    const first = await request(c),
      second = await request(c);
    const gap = second.calls[0]?.at - first.calls[0]?.at;
    return {
      pass:
        first.status === 200 &&
        second.status === 200 &&
        gap >= 19500 &&
        gap < 26000,
      gapMs: gap,
      firstCalls: first.calls.length,
      secondCalls: second.calls.length,
    };
  });
  await check("paced-retry-honors-account-floor", async () => {
    const c = await make([{ failCount: 1, retryAfter: 2 }], {
      endpoint: true,
      waitSecs: 30,
    });
    await configure(instance, c.profile.connections);
    const r = await request(c);
    const gap = r.calls[1]?.at - r.calls[0]?.at;
    return {
      ...r,
      pass:
        r.status === 200 && r.calls.length === 2 && gap >= 19500 && gap < 26000,
      gapMs: gap,
    };
  });
  await check("normal-request-timing", async () => {
    const times = [];
    for (let i = 0; i < 12; i++) {
      const r = await request(retained);
      if (r.status !== 200 || r.calls.length !== 1)
        throw Error("ROUTE_CHANGED");
      times.push(r.ms);
    }
    times.sort((a, b) => a - b);
    return {
      pass: true,
      samples: 12,
      medianMs: times[6],
      p95Ms: times[11],
      queueMinTimeMs: 0,
    };
  });
  for (const protocol of ["responses", "messages"]) {
    await check("paced-retry-full-package-" + protocol, async () => {
      const c = await make([{ failCount: 1, retryAfter: 2 }], {
        endpoint: true,
        waitSecs: 30,
      });
      await configure(instance, c.profile.connections);
      const r = await request(c, { protocol });
      const gap = r.calls[1]?.at - r.calls[0]?.at;
      return {
        ...r,
        gapMs: gap,
        pass:
          r.status === 200 &&
          r.calls.length === 2 &&
          gap >= 19500 &&
          gap < 26000 &&
          r.calls.every((x) => x.model === "same-model"),
      };
    });
  }
  await check("paced-long-hint-not-shortened", async () => {
    const c = await make([{ failCount: 1, retryAfter: 25 }], {
      endpoint: true,
      waitSecs: 30,
    });
    await configure(instance, c.profile.connections);
    const r = await request(c);
    const gap = r.calls[1]?.at - r.calls[0]?.at;
    return {
      ...r,
      gapMs: gap,
      pass:
        r.status === 200 && r.calls.length === 2 && gap >= 24500 && gap < 32000,
    };
  });
  await check("paced-failover-other-account-not-delayed", async () => {
    const c = await make([{ code: 429, retryAfter: 2 }, {}], {
      endpoint: true,
      waitSecs: 30,
    });
    await configure(instance, c.profile.connections);
    const r = await request(c);
    return {
      ...r,
      pass:
        r.status === 200 &&
        r.ms < 3000 &&
        r.calls.length === 2 &&
        r.calls[0].account.endsWith("-0") &&
        r.calls[1].account.endsWith("-1") &&
        r.calls.every((x) => x.model === "same-model"),
    };
  });
  await check(
    "paced-scoped-retry-never-uses-healthy-excluded-account",
    async () => {
      const c = await make([{ failCount: 1, retryAfter: 2 }, {}], {
        endpoint: true,
        waitSecs: 30,
      });
      await configure(instance, c.profile.connections);
      const key = await instance.admin("/api/keys", "POST", {
        name: "paced-restricted",
        noLog: true,
        allowedConnections: [c.profile.connections[0]],
        modelAccessMode: "restricted",
        allowedModels: [],
        allowedCombos: [c.route.name],
      });
      const r = await request(c, { key: key.key });
      const gap = r.calls[1]?.at - r.calls[0]?.at;
      return {
        ...r,
        gapMs: gap,
        pass:
          r.status === 200 &&
          r.calls.length === 2 &&
          gap >= 19500 &&
          r.calls.every(
            (x) => x.account.endsWith("-0") && x.model === "same-model",
          ),
      };
    },
  );
  const password = randomUUID(),
    archive = path.join(dir, "backup.json");
  await instance.stop();
  await backup(instance, archive, password);
  recovered = await restore(archive, path.join(dir, "restored"), password);
  await recovered.start();
  await check("restored-native-policy-and-scoped-route", async () => {
    const original = instance;
    instance = recovered;
    try {
      const policy = await instance.admin("/api/resilience");
      const paced = (await instance.admin("/api/providers")).connections.find(
        (p) => p.id === pacedId,
      );
      const key = await keytar.getPassword(
        instance.meta.bootstrap,
        "route-" + retained.route.name,
      );
      const r = await request(retained, { key });
      return {
        ...r,
        pass:
          r.status === 200 &&
          r.calls.length === 1 &&
          r.calls[0].model === "same-model" &&
          paced?.rateLimitProtection === true &&
          Object.entries(conservativeTraffic()).every(
            ([k, v]) => paced.rateLimitOverrides?.[k] === v,
          ) &&
          Object.entries(nativeWait()).every(([s, v]) =>
            Object.entries(v).every(([k, x]) => policy[s][k] === x),
          ),
      };
    } finally {
      instance = original;
    }
  });
  result.baselineUnchanged =
    JSON.stringify(result.baseline) === JSON.stringify(baseline());
} catch (e) {
  result.fatal = e.message;
} finally {
  for (const i of [instance, recovered])
    if (i)
      await check("cleanup-" + i.meta.id, async () => ({
        pass: (await i.cleanup()) === 0,
      }));
  mock.closeAllConnections();
  await new Promise((r) => mock.close(r));
  save();
  console.log("RESULT " + path.join(dir, "result.json"));
  if (result.fatal || result.cases.some((c) => !c.pass)) process.exitCode = 1;
}
