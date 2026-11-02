// SPDX-License-Identifier: Apache-2.0
export const fusionSafety = {
  requireApiKey: true, requireLogin: true, requestLoggingEnabled: false,
  debugMode: false, logToolSources: false, cloudEnabled: false,
  tailscaleEnabled: false, mcpEnabled: false, a2aEnabled: false,
  oidcEnabled: false, autoRefreshProviderQuota: false,
  localOnlyManageScopeBypassEnabled: false, credentialRedactionEnabled: true,
  requestRetry: 0, maxRetryIntervalSec: 0,
  fallbackStrategy: "fill-first", stickyRoundRobinLimit: 1,
  disableSessionStickiness: true, promptCacheAffinityEnabled: false,
  customSystemPromptEnabled: false,
};
