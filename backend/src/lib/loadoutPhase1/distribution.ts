// SPDX-License-Identifier: Apache-2.0
// Settings no one can change in this distribution. Credential masking, cache affinity, session
// stickiness and MCP were fixed here in 1.0 and are switches since 1.1 (control/features.ts); their
// 1.1 defaults are written once by releaseDefaults.ts.
export const fusionSafety = {
  requireApiKey: true, requireLogin: true, requestLoggingEnabled: false,
  debugMode: false, logToolSources: false, cloudEnabled: false,
  tailscaleEnabled: false, a2aEnabled: false,
  oidcEnabled: false, autoRefreshProviderQuota: false,
  localOnlyManageScopeBypassEnabled: false,
  requestRetry: 0, maxRetryIntervalSec: 0,
  fallbackStrategy: "fill-first", stickyRoundRobinLimit: 1,
  customSystemPromptEnabled: false,
};
