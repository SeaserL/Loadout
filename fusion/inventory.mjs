// SPDX-License-Identifier: Apache-2.0
// Declarative migration inventory. Never grants permissions or invents credentials.
export function inventory(state) {
  const accounts = new Map(state.accounts.map((a) => [a.ref, a]));
  const transport = {
    sensenova: { apiType: "chat", credential: "API_KEY_PRIVATE_INPUT" },
    commandcode: { apiType: "chat", credential: "API_KEY_PRIVATE_INPUT" },
    "opencode-go": {
      apiType: "responses",
      credential: "API_KEY_PRIVATE_INPUT",
      scope: "GPT_POOL_MEMBER_ONLY",
    },
    "workbuddy-bridge": {
      apiType: "chat",
      credential: "BOUND_DESKTOP_SESSION",
    },
    deepseek: { apiType: "chat", credential: "API_KEY_PRIVATE_INPUT" },
    "codex-subscription": { apiType: "responses", credential: "LOADOUT_CONTROLLED_LOGIN", scope: "EXPLICIT_OPT_IN_POOL_ONLY" },
  };
  const describe = (ref, model) => {
    const account = accounts.get(ref);
    return {
      account: ref,
      model,
      target: transport[account?.kind] ?? null,
      blocker: !account?.enabled
        ? "ACCOUNT_DISABLED_OR_MISSING"
        : !transport[account.kind]
          ? "CREDENTIAL_PROFILE_UNSUPPORTED"
          : account.kind === "codex-subscription" ? "CONTROLLED_LOGIN_BINDING_REQUIRED" : null,
    };
  };
  const pools = state.pools.map((p) => ({
    source: "loadout/" + p.name,
    target: "pool-" + p.name,
    ordering: p.ordering,
    members: p.members.map((m) => describe(m.account, m.model)),
  }));
  const endpoints = state.endpoints.map((e) => {
    const members =
      e.members ??
      state.accounts
        .filter((a) => a.enabled && a.provider === e.provider)
        .map((a) => a.ref);
    const models = [
      ...new Set(
        (state.accountModels ?? [])
          .filter((m) => members.includes(m.account))
          .map((m) => m.model),
      ),
    ].sort();
    return {
      source: `/e/${e.name}/v1`,
      target: "/v1",
      name: e.name,
      models: models.map((model, i) => ({
        model,
        target: `endpoint-${e.name}-${i + 1}`,
        members: members.map((ref) => describe(ref, model)),
      })),
      waitPolicy: "native-bounded-v1",
      rotate: e.rotate,
      membership:
        e.members === null ? "RECOMPILE_ON_ACCOUNT_CHANGE" : "EXPLICIT",
      blocker: !models.length ? "MODEL_CATALOG_REQUIRED" : null,
    };
  });
  return {
    version: 1,
    mode: "INVENTORY_ONLY",
    pools,
    endpoints,
    accounts: state.accounts.map((a) => describe(a.ref, null)),
    clients: state.clientPolicies.map((policy, i) => ({
      ordinal: i + 1,
      source: policy,
      targetRequirements: {
        modelAccessMode: Object.keys(policy).length
          ? "restricted"
          : "EXPLICIT_REISSUE",
        ...(policy.requests_per_minute
          ? { rateLimits: [{ limit: policy.requests_per_minute, window: 60 }] }
          : {}),
        allowedModels: policy.allowed_models ?? [],
      },
      blocker: Object.keys(policy).length
        ? "CLIENT_POLICY_REQUIRES_EXPLICIT_ALIAS_MAPPING"
        : null,
    })),
    activationReady: false,
    reason: "INVENTORY_DOES_NOT_BIND_CREDENTIALS_OR_APPLY_POLICY",
  };
}
