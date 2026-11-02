// SPDX-License-Identifier: Apache-2.0
// Read-only metadata assessment. A proposed mapping is never proof of live credentials.
import { snapshot } from "./migrate.mjs";
import { inventory } from "./inventory.mjs";
import { compile, fingerprint } from "./mapping.mjs";

export function assess(state, proposal = {}) {
  const bindings = proposal.bindings ?? {}, aliases = proposal.aliases ?? {};
  const catalog = inventory(state);
  const endpointModels = proposal.endpointModels ?? Object.fromEntries(
    catalog.endpoints.map(e => [e.name, e.models.map(m => m.model)]),
  );
  const required = new Set([
    ...state.pools.flatMap(p => p.members.map(m => m.account)),
    ...catalog.endpoints.flatMap(e => e.models.flatMap(m => m.members.map(a => a.account))),
    ...Object.values(aliases).flatMap(members => Array.isArray(members) ? members.map(m => m.account) : []),
  ]);
  const accounts = state.accounts.map(a => ({
    account: a.ref, kind: a.kind, requiredByRoutes: required.has(a.ref),
    status: !a.enabled ? "DISABLED" : bindings[a.ref] ? "PROPOSED_NOT_VERIFIED" :
      ["codex", "claude-code", "gemini-cli", "kimi-code"].includes(a.kind)
        ? "READ_ONLY_SOURCE_RETAINED" : "UNBOUND_SOURCE_RETAINED",
    credentialMethod: catalog.accounts.find(x => x.account === a.ref)?.target?.credential ?? "UNSUPPORTED_TRANSPORT",
  }));
  const poolAliases = new Set(state.pools.map(p => "loadout/" + p.name));
  const aliasNames = new Set(["loadout/default", "loadout/fast", "loadout/realtime",
    ...state.clientPolicies.flatMap(p => p.allowed_models ?? [])]);
  const aliasChecks = [...aliasNames].map(alias => ({ alias,
    status: poolAliases.has(alias) ? "POOL_MAPPING" : Object.hasOwn(aliases, alias)
      ? "PROPOSED_NOT_VERIFIED" : "EXPLICIT_MAPPING_REQUIRED",
  }));
  const issues = [];
  if (proposal.sourceFingerprint && proposal.sourceFingerprint !== fingerprint(state)) issues.push("STALE_SOURCE");
  if (Object.keys(bindings).some(ref => !state.accounts.some(a => a.ref === ref && a.enabled))) issues.push("UNKNOWN_OR_DISABLED_BINDING");
  let plan;
  try { plan = compile(state, bindings, endpointModels, "native-bounded-v1", { aliases }); }
  catch (e) { issues.push(/^[A-Z_]+$/.test(e.message) ? e.message : "INVALID_MAPPING_INPUT"); }
  for (const r of plan?.routes ?? []) if (r.status !== "READY") issues.push(...r.reasons);
  if (accounts.some(a => a.requiredByRoutes && a.status !== "PROPOSED_NOT_VERIFIED")) issues.push("REQUIRED_ACCOUNT_BINDINGS_INCOMPLETE");
  if (aliasChecks.some(a => a.status === "EXPLICIT_MAPPING_REQUIRED")) issues.push("DEFAULT_OR_CLIENT_ALIAS_MAPPING_REQUIRED");
  return {
    version: 1, mode: "READ_ONLY_PREFLIGHT", sourceFingerprint: fingerprint(state),
    sourceIdentityFingerprint: state.identityFingerprint,
    sourceCounts: { accounts: state.accounts.length, enabledAccounts: state.accounts.filter(a => a.enabled).length,
      pools: state.pools.length, endpoints: state.endpoints.length, clientPolicies: state.clientPolicies.length, accountSets: state.accountSetCount },
    pools: state.pools, endpoints: catalog.endpoints, accounts, aliases: aliasChecks,
    mapping: plan ? { structurallyReady: plan.activationReady, routes: plan.routes.map(r => ({ name: r.name, status: r.status, reasons: r.reasons, missing: r.missing })) } : null,
    issues: [...new Set(issues)], activationReady: false,
    reason: "PREFLIGHT_DOES_NOT_VERIFY_CREDENTIALS_OR_APPLY_MIGRATION",
    externalIssues: ["SENSENOVA_RISK_429", "COMMANDCODE_401", "WORKBUDDY_NATURAL_CREDENTIAL_ROTATION"].map(code => ({ code, status: "DEFERRED_NOT_TESTED" })),
  };
}
export function preflight(source, proposal = {}) {
  const state = snapshot(source), report = assess(state, proposal);
  report.metadataStableDuringRead = fingerprint(snapshot(source)) === report.sourceFingerprint;
  if (!report.metadataStableDuringRead) report.issues.push("SOURCE_CHANGED");
  return report;
}
