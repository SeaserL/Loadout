// SPDX-License-Identifier: Apache-2.0
// Offline compiler only. Native OmniRoute owns selection, cooldowns and execution.
import { createHash } from "node:crypto";
import { keytar } from "./runtime.mjs";
export const fingerprint = (value) =>
  createHash("sha256").update(JSON.stringify(value)).digest("hex");
export const nativeWait = () => ({
  requestQueue: {
    maxWaitMs: 30000,
    executionMaxWaitMs: 90000,
    maxQueueDepth: 4,
  },
  waitForCooldown: { enabled: false, maxRetries: 0 },
  comboCooldownWait: {
    enabled: true,
    maxWaitMs: 30000,
    maxAttempts: 2,
    budgetMs: 30000,
  },
});
export function compile(snapshot, bindings, endpointModels, waitPolicy, migration = {}) {
  if (waitPolicy !== undefined && waitPolicy !== "native-bounded-v1")
    throw Error("UNKNOWN_WAIT_POLICY");
  if (snapshot.version !== 1) throw Error("SNAPSHOT_VERSION");
  if (
    snapshot.accountSetCount ||
    (snapshot.clientPolicies.some((p) => Object.keys(p).length) && !migration.aliases)
  )
    throw Error("UNMAPPED_ACCESS_POLICY");
  const accounts = new Map(snapshot.accounts.map((a) => [a.ref, a]));
  const routes = [];
  function route(name, members, kind, waitSecs = 0, rotate = false) {
    const missing = [],
      targets = [];
    for (const member of members) {
      const account = accounts.get(member.account),
        binding = bindings[member.account];
      if (!account?.enabled || member.enabled === false) {
        missing.push({ account: member.account, reason: "DISABLED_MEMBER" });
        continue;
      }
      if (!binding?.connectionId || !binding.provider) {
        missing.push({ account: member.account, reason: "MISSING_CONNECTION" });
        continue;
      }
      if (account.kind === "codex-subscription" && binding.credentialProfile !== "loadout-codex-v1") {
        missing.push({ account: member.account, reason: "CONTROLLED_LOGIN_BINDING_REQUIRED" });
        continue;
      }
      if (!binding.models?.includes(member.model)) {
        missing.push({ account: member.account, reason: "MODEL_NOT_VERIFIED" });
        continue;
      }
      targets.push({
        kind: "model",
        provider: binding.provider,
        model: member.model,
        connectionId: binding.connectionId,
      });
    }
    const reasons = missing.length ? ["MEMBER_BINDINGS_INCOMPLETE"] : [];
    if (!members.length) reasons.push("EMPTY_MEMBERS");
    if (waitSecs > 0 && !waitPolicy)
      reasons.push(
        "ENDPOINT_WAIT_NOT_SCOPED_NATIVELY",
        "WALL_CLOCK_DEADLINE_NOT_PROVEN",
      );
    if (rotate) reasons.push("ROUND_ROBIN_COMBO_NOT_VALIDATED");
    const config = {
      failoverBeforeRetry: true,
      maxRetries: 0,
      maxSetRetries: 0,
      maxGlobalAttempts: Math.max(
        1,
        Math.min(200, targets.length * (waitPolicy ? 3 : 1)),
      ),
      disableSessionStickiness: true,
      stickyRoundRobinLimit: 1,
      hedging: false,
      comboTimeoutMs: 0,
      ...(waitPolicy ? { targetTimeoutMs: 90000 } : {}),
    };
    routes.push({
      name,
      kind,
      status: reasons.length ? "BLOCKED" : "READY",
      reasons,
      missing,
      sourceMembers: members,
      waitSecs,
      native: {
        name,
        strategy: rotate ? "round-robin" : "priority",
        models: targets,
        config,
      },
      allowedConnections: [...new Set(targets.map((t) => t.connectionId))],
      explicitCrossModel: new Set(members.map((m) => m.model)).size > 1,
    });
  }
  for (const pool of snapshot.pools) {
    if (pool.ordering !== "manual") throw Error("UNMAPPED_POOL_ORDERING");
    route("pool-" + pool.name, pool.members, "pool");
    if (migration.aliases) routes.at(-1).sourceAlias = "loadout/" + pool.name;
  }
  const aliases = migration.aliases ?? {};
  for (const [alias, members] of Object.entries(aliases)) {
    if (!/^[a-zA-Z0-9_./-]{1,200}$/.test(alias) || alias.includes("..") ||
        !Array.isArray(members) || !members.length || routes.some((r) => r.sourceAlias === alias))
      throw Error("INVALID_LEGACY_ALIAS");
    if (["loadout/default", "loadout/fast", "loadout/realtime"].includes(alias) &&
        members.some((m) => accounts.get(m.account)?.kind === "codex-subscription"))
      throw Error("SUBSCRIPTION_NOT_IN_AUTOMATIC_POOL");
    if (!alias.startsWith("loadout/") && (alias.split("/").length < 3 ||
        members.some((m) => m.model !== alias.split("/").slice(2).join("/") || m.account.split("/")[0] !== alias.split("/")[0])))
      throw Error("EXACT_ALIAS_SCOPE_MISMATCH");
    route("legacy-" + (routes.length + 1), members, "alias");
    routes.at(-1).sourceAlias = alias;
  }
  const clients = snapshot.clientPolicies.map((policy, index) => {
    if (!Object.keys(policy).length) return null;
    if (Object.keys(policy).some((k) => !["allowed_models", "requests_per_minute"].includes(k)) ||
        !Array.isArray(policy.allowed_models) || !policy.allowed_models.length ||
        (policy.requests_per_minute !== undefined && (!Number.isInteger(policy.requests_per_minute) || policy.requests_per_minute <= 0)))
      throw Error("UNMAPPED_ACCESS_POLICY");
    const selected = policy.allowed_models.map((alias) => routes.find((r) => r.sourceAlias === alias));
    if (selected.some((r) => !r)) throw Error("CLIENT_ALIAS_MAPPING_REQUIRED");
    return { name: "legacy-client-" + (index + 1), noLog: true, modelAccessMode: "restricted",
      allowedModels: [...policy.allowed_models], allowedCombos: selected.map((r) => r.name),
      allowedConnections: [...new Set(selected.flatMap((r) => r.allowedConnections))],
      ...(policy.requests_per_minute ? { rateLimits: [{ limit: policy.requests_per_minute, window: 60 }] } : {}) };
  }).filter(Boolean);
  for (const endpoint of snapshot.endpoints) {
    const members =
      endpoint.members ??
      snapshot.accounts
        .filter((a) => a.enabled && a.provider === endpoint.provider)
        .map((a) => a.ref);
    if (
      members.some((ref) => accounts.get(ref)?.provider !== endpoint.provider)
    )
      throw Error("ENDPOINT_PROVIDER_ESCAPE");
    const models = endpointModels[endpoint.name];
    if (!models?.length) throw Error("ENDPOINT_MODELS_REQUIRED");
    if (snapshot.accountModels) {
      const expected = [
        ...new Set(
          snapshot.accountModels
            .filter((m) => members.includes(m.account))
            .map((m) => m.model),
        ),
      ].sort();
      if (JSON.stringify([...models].sort()) !== JSON.stringify(expected))
        throw Error("ENDPOINT_MODEL_COVERAGE_INCOMPLETE");
    }
    if (endpoint.defaultModel)
      throw Error("DEFAULT_MODEL_FALLBACK_REQUIRES_EXPLICIT_MAPPING");
    if (
      !Number.isInteger(endpoint.waitSecs) ||
      endpoint.waitSecs < 0 ||
      endpoint.waitSecs > 30
    )
      throw Error("WAIT_OUT_OF_PROFILE");
    for (const [index, model] of models.entries()) {
      route(
        `endpoint-${endpoint.name}-${index + 1}`,
        members.map((account) => ({ account, model })),
        "endpoint",
        endpoint.waitSecs,
        endpoint.rotate,
      );
      Object.assign(routes.at(-1), {
        upstreamModel: model,
        sourceUrl: `/e/${endpoint.name}/v1`,
        newUrl: "/v1",
        membership:
          endpoint.members === null
            ? "RECOMPILE_ON_ACCOUNT_CHANGE"
            : "EXPLICIT",
      });
    }
  }
  return {
    version: 1,
    ...(waitPolicy ? { waitPolicy } : {}),
    endpointModels,
    ...(migration.aliases ? { migration, clients,
      accountInventory: snapshot.accounts.map((a) => ({ account: a.ref, kind: a.kind,
        status: !a.enabled ? "DISABLED" : bindings[a.ref] ? "BOUND" :
          ["codex", "claude-code", "gemini-cli", "kimi-code"].includes(a.kind) ? "READ_ONLY_SOURCE_RETAINED" : "UNBOUND_SOURCE_RETAINED" })),
      fullMigrationReady: snapshot.accounts.filter((a) => a.enabled).every((a) => !!bindings[a.ref]),
    } : {}),
    sourceFingerprint: fingerprint(snapshot),
    bindingsFingerprint: fingerprint(bindings),
    routes,
    activationReady: routes.every((r) => r.status === "READY"),
    resilience: waitPolicy
      ? nativeWait()
      : {
          waitForCooldown: { enabled: false, maxRetries: 0 },
          comboCooldownWait: { enabled: false, maxAttempts: 0 },
        },
    waitCandidate: {
      comboCooldownWait: {
        enabled: true,
        maxWaitMs: 30000,
        maxAttempts: 10,
        budgetMs: 30000,
      },
    },
    warnings: [
      "CLIENT_KEYS_REISSUED",
      "CONFIGURATION_SNAPSHOT_REQUIRES_RECOMPILE",
    ],
  };
}
export async function applyReady(
  instance,
  plan,
  { names, source, bindings } = {},
) {
  if (
    !source ||
    !bindings ||
    fingerprint(source) !== plan.sourceFingerprint ||
    fingerprint(bindings) !== plan.bindingsFingerprint
  )
    throw Error("STALE_MAPPING");
  if (
    fingerprint(
      compile(source, bindings, plan.endpointModels, plan.waitPolicy, plan.migration),
    ) !== fingerprint(plan)
  )
    throw Error("MODIFIED_MAPPING");
  const selected = plan.routes.filter((r) => !names || names.includes(r.name));
  if (plan.migration?.aliases && selected.length !== plan.routes.length)
    throw Error("COMPLETE_ALIAS_MAPPING_REQUIRED");
  if (
    !selected.length ||
    names?.some((n) => !selected.some((r) => r.name === n))
  )
    throw Error("UNKNOWN_MAPPING");
  if (selected.some((r) => r.status !== "READY"))
    throw Error("BLOCKED_MAPPING");
  const live = await instance.admin("/api/providers");
  for (const r of selected) for (const target of r.native.models) {
    const row = live.connections.find((c) => c.id === target.connectionId && c.provider === target.provider);
    if (!row?.isActive) throw Error("LIVE_BINDING_UNAVAILABLE");
    const member = r.sourceMembers.find((m) => bindings[m.account]?.connectionId === target.connectionId);
    if (source.accounts.find((a) => a.ref === member?.account)?.kind === "codex-subscription" &&
        (row.providerSpecificData?.credentialProfile !== "loadout-codex-v1" || row.providerSpecificData?.transportOptIn !== true || row.providerSpecificData?.loginState !== "ready"))
      throw Error("CONTROLLED_LOGIN_BINDING_REQUIRED");
  }
  if (selected.some((r) => r.sourceAlias)) {
    const old = await instance.admin("/api/model-combo-mappings");
    if (old.mappings.some((m) => /[*?]/.test(m.pattern) || selected.some((r) => r.sourceAlias === m.pattern)))
      throw Error("ALIAS_MAPPING_CONFLICT");
  }
  const existing = await instance.admin("/api/combos");
  if (existing.combos.some((c) => selected.some((r) => r.name === c.name)))
    throw Error("MAPPING_ALREADY_EXISTS");
  // Do not silently change a live instance's global retry/wait policy.
  const resilience = await instance.admin("/api/resilience");
  if (plan.waitPolicy) {
    for (const [section, fields] of Object.entries(nativeWait())) {
      if (
        Object.entries(fields).some(
          ([key, value]) => resilience[section]?.[key] !== value,
        )
      )
        throw Error("NATIVE_WAIT_PROFILE_MISMATCH");
    }
  } else if (
    resilience.comboCooldownWait?.enabled ||
    resilience.waitForCooldown?.enabled
  )
    throw Error("GLOBAL_WAIT_CONFLICT");
  const created = [],
    keys = [], aliases = [];
  try {
    for (const r of selected) {
      const combo = await instance.admin("/api/combos", "POST", r.native);
      created.push(combo.id);
      if (r.sourceAlias) {
        const mapped = await instance.admin("/api/model-combo-mappings", "POST", {
          pattern: r.sourceAlias, comboId: combo.id, priority: 100, enabled: true,
        });
        aliases.push(mapped.mapping.id);
      }
      const key = await instance.admin("/api/keys", "POST", {
        name: r.name,
        noLog: true,
        allowedConnections: r.allowedConnections,
        modelAccessMode: "restricted",
        allowedModels: r.sourceAlias ? [r.sourceAlias] : [],
        allowedCombos: [r.name],
      });
      keys.push(key);
      await keytar.setPassword(
        instance.meta.bootstrap,
        "route-" + r.name,
        key.key,
      );
    }
    for (const policy of plan.clients ?? []) {
      const key = await instance.admin("/api/keys", "POST", policy);
      keys.push(key);
      // Native create omits rateLimits; use its permissions update contract.
      if (policy.rateLimits) await instance.admin("/api/keys/" + key.id, "PATCH", { rateLimits: policy.rateLimits });
      await keytar.setPassword(instance.meta.bootstrap, "route-" + policy.name, key.key);
    }
    return { names: selected.map((r) => r.name), keys };
  } catch (e) {
    for (const id of aliases.reverse()) await instance.admin("/api/model-combo-mappings/" + id, "DELETE");
    for (const key of keys.reverse()) {
      await instance.admin("/api/keys/" + key.id, "DELETE");
      await keytar.deletePassword(instance.meta.bootstrap, "route-" + key.name);
    }
    for (const id of created.reverse())
      await instance.admin("/api/combos/" + id, "DELETE");
    throw e;
  }
}
