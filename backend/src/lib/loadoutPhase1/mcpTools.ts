// SPDX-License-Identifier: Apache-2.0
// The MCP tools Loadout announces (ExecPlan 071). Read-only tools come with MCP itself; tools that
// change settings or spend tokens need the separate "actions" switch. Everything else OmniRoute
// ships (plugins, files, shell, notes, skills, web search, pool and combo edits) stays off: pools
// are Loadout's, and an edit from outside would leave its own record of them behind.
import { kvGet } from "./control/shared";

/** Tool name and the scopes its OmniRoute definition requires. */
const READ: Record<string, string[]> = {
  omniroute_get_health: ["read:health"],
  omniroute_list_combos: ["read:combos"],
  omniroute_get_combo_metrics: ["read:combos"],
  omniroute_check_quota: ["read:quota"],
  omniroute_cost_report: ["read:usage"],
  omniroute_list_models_catalog: [],
  omniroute_simulate_route: ["read:health", "read:combos"],
  omniroute_explain_route: ["read:health", "read:usage"],
  omniroute_get_provider_metrics: ["read:health"],
  omniroute_best_combo_for_task: ["read:combos", "read:health"],
  omniroute_pick_fastest_model: [],
  omniroute_get_session_snapshot: ["read:usage"],
  omniroute_cache_stats: ["read:cache"],
  omniroute_compression_status: ["read:compression"],
  omniroute_memory_search: ["read:memory"],
};
const ACTIONS: Record<string, string[]> = {
  omniroute_route_request: ["execute:completions"],
  omniroute_test_combo: ["execute:completions", "read:combos"],
  omniroute_set_budget_guard: ["write:budget"],
  omniroute_sync_pricing: ["pricing:write"],
  omniroute_cache_flush: ["write:cache"],
  omniroute_memory_add: ["write:memory"],
  omniroute_memory_clear: ["write:memory"],
};

export const MCP_READ_TOOLS = Object.keys(READ);
export const MCP_ACTION_TOOLS = Object.keys(ACTIONS);

export const mcpActionsOn = () => kvGet<string>("features.mcp.actions", "off") === "on";

/** Whether a tool is announced and callable right now. */
export function mcpToolAllowed(name: string) {
  return name in READ || (name in ACTIONS && mcpActionsOn());
}

/** The scopes a client key gets when MCP is allowed for it: connect, plus every tool's own. */
export function mcpClientScopes(): string[] {
  return ["mcp:connect", ...new Set([...Object.values(READ), ...Object.values(ACTIONS)].flat())];
}
