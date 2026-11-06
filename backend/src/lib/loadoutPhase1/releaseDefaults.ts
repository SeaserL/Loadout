// SPDX-License-Identifier: Apache-2.0
// Defaults a Loadout release changes in OmniRoute's settings. Each step runs once per instance,
// before requests are served, so a choice the user makes afterwards is never overwritten.
import { getSettings, updateSettings } from "../db/settings";
import { clearCache } from "../semanticCache";
import { kvGet, kvSet } from "./control/shared";

const record = (value: unknown): Record<string, any> => {
  if (typeof value === "string") {
    try {
      return record(JSON.parse(value));
    } catch {
      return {};
    }
  }
  return value && typeof value === "object" && !Array.isArray(value) ? { ...(value as object) } : {};
};

export const STEPS: { id: string; run: () => Promise<void> }[] = [
  {
    // OmniRoute queues every API-key account at 60 rpm with 350 ms between requests, so parallel
    // requests wait in line and the 61st in a minute waits 30 s before failing over. Loadout
    // relies on the upstream's own 429 and failover; a limit can still be set per account.
    id: "1.1-request-queue-off",
    run: async () => {
      const resilience = record((await getSettings()).resilienceSettings);
      resilience.requestQueue = { ...record(resilience.requestQueue), autoEnableApiKeyProviders: false };
      await updateSettings({ resilienceSettings: resilience });
    },
  },
  {
    // The response cache stores reply bodies on disk (ADR-008): off unless the user turns it on,
    // and whatever 1.0 stored is removed.
    id: "1.1-response-cache-off",
    run: async () => {
      await updateSettings({ semanticCacheEnabled: false });
      clearCache();
    },
  },
];

/** Mainland China formats; OmniRoute's built-in PII types cover the US and Brazil only. */
export const CHINESE_PII_PATTERNS = [
  { id: "loadout-cn-mobile", name: "中国手机号", type: "regex", pattern: "(?<!\\d)(?:\\+?86[- ]?)?1[3-9]\\d{9}(?!\\d)", flags: "g", enabled: true },
  {
    id: "loadout-cn-id",
    name: "身份证号",
    type: "regex",
    pattern: "(?<![0-9Xx])[1-9]\\d{5}(?:18|19|20)\\d{2}(?:0[1-9]|1[0-2])(?:0[1-9]|[12]\\d|3[01])\\d{3}[0-9Xx](?![0-9Xx])",
    flags: "g",
    enabled: true,
  },
  { id: "loadout-cn-bankcard", name: "银行卡号", type: "regex", pattern: "(?<!\\d)62\\d{14,17}(?!\\d)", flags: "g", enabled: true },
];

STEPS.push({
  // 1.0 fixed these in fusionSafety; 1.1 makes them switches and keeps 1.0's behaviour as the default.
  // The vision bridge is off: with it on, an image sent to a text-only model was answered by another
  // model the client never asked for.
  id: "1.1-feature-defaults",
  run: async () => {
    await updateSettings({
      credentialRedactionEnabled: true,
      promptCacheAffinityEnabled: false,
      disableSessionStickiness: true,
      mcpEnabled: false,
      memoryEnabled: false,
      modalityBridgeVisionEnabled: false,
      visionBridgeEnabled: false,
      modalityBridgeAudioEnabled: false,
      modalityBridgeVideoEnabled: false,
    });
    const { setFeatureFlagOverride } = await import("../db/featureFlags");
    setFeatureFlagOverride("INJECTION_GUARD_MODE", "off");
    setFeatureFlagOverride("PII_REDACTION_ENABLED", "false");
    setFeatureFlagOverride("PII_RESPONSE_SANITIZATION", "false");
    const { getPiiSettings, setPiiSettings } = await import("../db/piiSettings");
    const ours = new Set(CHINESE_PII_PATTERNS.map((p) => p.id));
    const theirs = (getPiiSettings().customPatterns ?? []).filter((p: { id: string }) => !ours.has(p.id));
    setPiiSettings({ customPatterns: [...(CHINESE_PII_PATTERNS as any[]), ...theirs].slice(0, 20) });
    const { updateCompressionSettings } = await import("../db/compression");
    await updateCompressionSettings({ enabled: false, defaultMode: "off" } as any);
  },
});

export async function applyReleaseDefaults() {
  const done = new Set(kvGet<string[]>("releaseDefaults", []));
  for (const step of STEPS) {
    if (done.has(step.id)) continue;
    await step.run();
    done.add(step.id);
    kvSet("releaseDefaults", [...done]);
  }
}
