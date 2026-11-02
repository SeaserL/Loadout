// SPDX-License-Identifier: Apache-2.0
import fs from "node:fs";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";
import { Instance, sha, writeJson } from "./runtime.mjs";
import { compile, applyReady, nativeWait, fingerprint } from "./mapping.mjs";
import { configure } from "./policy.mjs";
import { inventory } from "./inventory.mjs";

// Read metadata only. Credential values never come from the SQLite source.
export function snapshot(source) {
  const db = new DatabaseSync(path.resolve(source), { readOnly: true });
  try {
    db.exec("PRAGMA query_only=ON");
    db.exec("BEGIN");
    const refs = new Map(),
      count = {},
      identities = [];
    const accounts = db
      .prepare(
        "SELECT a.id,p.slug,p.kind,a.enabled,a.position FROM accounts a JOIN providers p ON a.provider_id=p.id ORDER BY p.slug,a.position,a.id",
      )
      .all()
      .map((row) => {
        const ref = `${row.slug}/account-${(count[row.slug] = (count[row.slug] || 0) + 1)}`;
        refs.set(row.id, ref);
        identities.push([row.id, row.slug, row.position, ref]);
        return {
          ref,
          provider: row.slug,
          kind: row.kind,
          enabled: !!row.enabled,
        };
      });
    const pools = db
      .prepare("SELECT id,name,ordering FROM pools ORDER BY name")
      .all()
      .map((p) => ({
        name: p.name,
        ordering: p.ordering,
        members: db
          .prepare(
            "SELECT am.account_id,am.upstream_model_id,am.enabled FROM pool_members pm JOIN account_models am ON am.id=pm.account_model_id WHERE pm.pool_id=? ORDER BY pm.position",
          )
          .all(p.id)
          .map((m) => ({
            account: refs.get(m.account_id),
            model: m.upstream_model_id,
            enabled: !!m.enabled,
          })),
      }));
    const columns = new Set(
      db
        .prepare("PRAGMA table_info(endpoints)")
        .all()
        .map((c) => c.name),
    );
    const endpoints = db
      .prepare("SELECT * FROM endpoints WHERE enabled=1 ORDER BY name")
      .all()
      .map((e) => ({
        name: e.name,
        provider: e.provider_slug,
        members: e.members_json
          ? JSON.parse(e.members_json).map((id) => refs.get(id) ?? "UNKNOWN")
          : null,
        waitSecs: e.wait_secs,
        rotate: columns.has("rotate") ? !!e.rotate : false,
        rotateColumnPresent: columns.has("rotate"),
        defaultModel: e.default_model,
      }));
    const clientPolicies = db
      .prepare(
        "SELECT policy_json FROM clients WHERE enabled=1 AND revoked_at IS NULL",
      )
      .all()
      .filter((r) => r.policy_json)
      .map((r) => JSON.parse(r.policy_json));
    const accountSetCount = Number(
      db.prepare("SELECT count(*) AS n FROM account_sets").get().n,
    );
    const modelColumns = new Set(
      db
        .prepare("PRAGMA table_info(account_models)")
        .all()
        .map((c) => c.name),
    );
    const accountModels = db
      .prepare(
        "SELECT account_id,upstream_model_id" +
          (modelColumns.has("protocols_json") ? ",protocols_json" : "") +
          " FROM account_models WHERE enabled=1 ORDER BY account_id,upstream_model_id",
      )
      .all()
      .map((m) => ({
        account: refs.get(m.account_id),
        model: m.upstream_model_id,
        protocols: m.protocols_json ? JSON.parse(m.protocols_json) : [],
      }));
    return {
      version: 1,
      identityFingerprint: sha(JSON.stringify(identities)),
      accounts,
      pools,
      endpoints,
      accountSetCount,
      clientPolicies,
      accountModels,
    };
  } finally {
    db.close();
  }
}
export function preview(source, bindings = {}, models = {}) {
  const state = snapshot(source);
  try {
    return {
      source: state,
      inventory: inventory(state),
      plan: compile(state, bindings, models, "native-bounded-v1"),
    };
  } catch (error) {
    return {
      source: state,
      inventory: inventory(state),
      activationReady: false,
      blocker: /^[A-Z_]+$/.test(error.message)
        ? error.message
        : "SOURCE_UNSUPPORTED",
    };
  }
}

// Secrets supplied through a private stdin stream or by a caller, never a JSON file option.
// All source accounts must be represented; unsupported members are an explicit blocker.
export async function migrate(sourceFile, destination, input) {
  const state = snapshot(sourceFile);
  if (fingerprint(state) !== input.sourceFingerprint)
    throw Error("STALE_SOURCE");
  const groups = input.providers;
  if (state.accounts.some((a) => a.enabled && a.kind === "codex-subscription"))
    throw Error("USE_CONTROLLED_LOGIN_AND_MAP");
  if (!Array.isArray(groups) || !groups.length)
    throw Error("PROVIDERS_REQUIRED");
  if (groups.some((g) => ![undefined, "chat", "responses"].includes(g.apiType)))
    throw Error("UPSTREAM_PROTOCOL_UNSUPPORTED");
  if (
    groups.some(
      (g) => g.traffic !== "conservative-v1" && g.traffic !== "native-default",
    )
  )
    throw Error("EXPLICIT_TRAFFIC_POLICY_REQUIRED");
  const bindings = {},
    refs = new Set();
  for (const group of groups)
    for (const account of group.accounts) {
      if (
        refs.has(account.ref) ||
        !account.key ||
        !Array.isArray(account.models)
      )
        throw Error("ACCOUNT_INPUT_INVALID");
      refs.add(account.ref);
      bindings[account.ref] = {
        provider: "planned-" + group.slug,
        connectionId: "planned-" + account.ref,
        models: account.models,
      };
    }
  if (
    state.accounts.filter((a) => a.enabled).some((a) => !refs.has(a.ref)) ||
    [...refs].some(
      (ref) => !state.accounts.some((a) => a.ref === ref && a.enabled),
    )
  )
    throw Error("ACCOUNT_BINDINGS_INCOMPLETE");
  const preflight = compile(
    state,
    bindings,
    input.endpointModels ?? {},
    "native-bounded-v1",
    input.aliases ? { aliases: input.aliases } : {},
  );
  if (!preflight.activationReady) throw Error("BLOCKED_MAPPING");
  if (fingerprint(snapshot(sourceFile)) !== fingerprint(state))
    throw Error("SOURCE_CHANGED");
  const instance = await Instance.create(destination, undefined, input.config);
  try {
    await instance.start();
    await instance.admin("/api/resilience", "PATCH", nativeWait());
    for (const group of groups) {
      const created = await instance.addProvider(
        group.slug,
        group.baseUrl,
        group.accounts,
        group.apiType,
      );
      if (group.traffic === "conservative-v1")
        await configure(instance, created.connections);
      group.accounts.forEach((account, index) => {
        bindings[account.ref] = {
          provider: created.provider,
          connectionId: created.connections[index],
          models: account.models,
        };
      });
    }
    const plan = compile(
      state,
      bindings,
      input.endpointModels ?? {},
      "native-bounded-v1",
      input.aliases ? { aliases: input.aliases } : {},
    );
    if (plan.routes.length)
      await applyReady(instance, plan, { source: state, bindings });
    if (fingerprint(snapshot(sourceFile)) !== fingerprint(state))
      throw Error("SOURCE_CHANGED");
    writeJson(path.join(instance.dir, "migration.json"), {
      sourceFingerprint: fingerprint(state),
      bindings,
      plan,
      clientKeys: "REISSUED",
      traffic: groups.map((g) => ({ provider: g.slug, profile: g.traffic })),
      oldSourceUntouched: true,
    });
    await instance.stop();
    return { instance, routes: plan.routes.map((r) => r.name) };
  } catch (error) {
    await instance.cleanup();
    fs.writeFileSync(path.join(instance.dir, "FAILED"), "MIGRATION_FAILED");
    throw error;
  }
}
