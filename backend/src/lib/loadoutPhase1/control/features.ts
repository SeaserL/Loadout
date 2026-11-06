// SPDX-License-Identifier: Apache-2.0
// The 1.1 feature switches (ExecPlan 071). Each one reads and writes OmniRoute's own settings in
// this process; nothing here adds a request path or a second store.
import { getDbInstance } from "../../db/core";
import { getSettings, updateSettings } from "../../db/settings";
import { getCompressionSettings, updateCompressionSettings } from "../../db/compression";
import { getFeatureFlagOverride, setFeatureFlagOverride } from "../../db/featureFlags";
import { invalidateMemorySettingsCache } from "../../memory/settings";
import { clearCache } from "../../semanticCache";
import { isVisionModelId } from "../../../shared/constants/visionModels";
import { aiPackState } from "../aiPack";
import { invalid, kvGet, kvSet } from "./shared";
import { listModels } from "./models";
import { packStatus } from "./updates";

type Group = "save" | "safety" | "extend";
type Choice = { value: string; pack?: boolean };
type State = { settings: Record<string, any>; compression: Record<string, any>; pack: boolean };
type Option = {
  key: string;
  /** "model": a model name from this install's list, shown as a picker; else a short fixed set. */
  kind?: "model";
  choices: (s: State) => Promise<Choice[]> | Choice[];
  read: (s: State) => string;
  write: (value: string, s: State) => Promise<void>;
};
type Def = {
  key: string;
  group: Group;
  /** The whole feature needs the AI pack. */
  pack?: boolean;
  /** Keeps request or reply text on this computer while on (ADR-008). */
  stores: (s: State) => boolean;
  read: (s: State) => boolean;
  write: (on: boolean, s: State) => Promise<void>;
  options?: Option[];
};

const own = <T>(key: string, fallback: T) => kvGet<T>(`features.${key}`, fallback);
const remember = (key: string, value: unknown) => kvSet(`features.${key}`, value);

// --- compression -------------------------------------------------------------------------------
// One choice in the console stands for a set of OmniRoute engines. OmniRoute 3.8.52 builds the
// stacked pipeline from its per-engine toggle map (ordered by each engine's stack priority), so the
// map is what gets written. Caveman detects the language itself, so Chinese gets the zh rule pack.
// LLMLingua uses the multilingual model, the only one the AI pack ships, read from `ai/models`.
const ENGINES = ["session-dedup", "ccr", "lite", "rtk", "codex-responses", "headroom", "relevance", "caveman", "aggressive", "llmlingua", "ultra", "omniglyph"];
const LEVELS: Record<string, { engines: Record<string, string | true>; pack?: boolean; stores: boolean }> = {
  light: { engines: { rtk: "standard", caveman: "lite" }, stores: false },
  standard: { engines: { rtk: "standard", "session-dedup": true, caveman: "full" }, stores: true },
  strong: { engines: { rtk: "aggressive", "session-dedup": true, caveman: "ultra" }, stores: true },
  model: { engines: { rtk: "standard", "session-dedup": true, llmlingua: true }, pack: true, stores: true },
};
const compressionLevel = () => own("compression.level", "light");

async function applyCompression(on: boolean, level: string) {
  const current = await getCompressionSettings();
  const chosen = LEVELS[level].engines;
  const engines = Object.fromEntries(
    ENGINES.map((id) => {
      const value = chosen[id];
      return [id, value === undefined ? { enabled: false } : value === true ? { enabled: true } : { enabled: true, level: value }];
    })
  );
  await updateCompressionSettings({
    enabled: on,
    defaultMode: on ? "stacked" : "off",
    engines,
    // Upstream only runs lossy engines when each request asks for them; choosing a level here is
    // the owner asking for all requests.
    loadoutLossyOptIn: on,
    cavemanConfig: { ...(current.cavemanConfig as any), enabled: on, autoDetectLanguage: true },
  } as any);
  if (!on || !LEVELS[level].stores) purgeCompressionStore();
}

/** session-dedup keeps what it removed so the model can fetch it back; none of it outlives the switch. */
function purgeCompressionStore() {
  try {
    getDbInstance().prepare("DELETE FROM ccr_blocks").run();
  } catch {
    // table absent: nothing stored
  }
  void import("@omniroute/open-sse/services/compression/engines/ccr/index")
    .then((ccr) => ccr.resetCcrStore())
    .catch(() => {});
}

// --- memory ------------------------------------------------------------------------------------
const MEMORY_SEARCH: Record<string, Record<string, unknown>> = {
  keyword: { memoryEmbeddingSource: "auto", memoryEmbeddingProviderModel: "", memoryTransformersEnabled: false, memoryStaticEnabled: false },
  provider: { memoryEmbeddingSource: "remote", memoryTransformersEnabled: false, memoryStaticEnabled: false },
  local: { memoryEmbeddingSource: "transformers", memoryTransformersEnabled: true, memoryStaticEnabled: false },
};
async function saveSettings(updates: Record<string, unknown>) {
  await updateSettings(updates);
  invalidateMemorySettingsCache();
}

async function chatModels(filter: (upstream: string) => boolean) {
  const rows = await listModels();
  return rows.filter((m) => m.enabled && filter(m.upstream_model)).map((m) => ({ value: m.name }));
}

// --- the switches ------------------------------------------------------------------------------
const DEFS: Def[] = [
  {
    key: "response_cache",
    group: "save",
    stores: () => true,
    read: (s) => s.settings.semanticCacheEnabled === true,
    write: async (on) => {
      await updateSettings({ semanticCacheEnabled: on });
      if (!on) clearCache();
    },
  },
  {
    key: "cache_affinity",
    group: "save",
    stores: () => false,
    read: (s) => s.settings.promptCacheAffinityEnabled === true,
    write: async (on) => {
      await updateSettings({ promptCacheAffinityEnabled: on, disableSessionStickiness: !on });
    },
  },
  {
    key: "compression",
    group: "save",
    stores: () => LEVELS[compressionLevel()]?.stores ?? false,
    read: (s) => s.compression.enabled === true,
    write: async (on) => applyCompression(on, compressionLevel()),
    options: [
      {
        key: "level",
        choices: () => Object.entries(LEVELS).map(([value, p]) => ({ value, pack: p.pack })),
        read: () => compressionLevel(),
        write: async (value, s) => {
          if (!LEVELS[value]) throw invalid(`未知压缩强度 ${value}`);
          remember("compression.level", value);
          await applyCompression(s.compression.enabled === true, value);
        },
      },
    ],
  },
  {
    key: "credential_mask",
    group: "safety",
    stores: () => false,
    read: (s) => s.settings.credentialRedactionEnabled === true,
    write: async (on) => {
      await updateSettings({ credentialRedactionEnabled: on });
    },
  },
  {
    key: "pii_redaction",
    group: "safety",
    stores: () => false,
    read: () => getFeatureFlagOverride("PII_REDACTION_ENABLED") === "true",
    write: async (on) => {
      setFeatureFlagOverride("PII_REDACTION_ENABLED", String(on));
      setFeatureFlagOverride("PII_RESPONSE_SANITIZATION", String(on));
    },
  },
  {
    key: "injection_guard",
    group: "safety",
    stores: () => false,
    read: () => getFeatureFlagOverride("INJECTION_GUARD_MODE") === "block",
    write: async (on) => {
      setFeatureFlagOverride("INJECTION_GUARD_MODE", on ? "block" : "off");
    },
  },
  {
    key: "memory",
    group: "extend",
    stores: () => true,
    read: (s) => s.settings.memoryEnabled === true,
    write: async (on) => saveSettings({ memoryEnabled: on }),
    options: [
      {
        key: "search",
        choices: () => [{ value: "keyword" }, { value: "provider" }, { value: "local", pack: true }],
        read: () => own("memory.search", "keyword"),
        write: async (value) => {
          if (!MEMORY_SEARCH[value]) throw invalid(`未知检索方式 ${value}`);
          remember("memory.search", value);
          await saveSettings(MEMORY_SEARCH[value]);
        },
      },
      {
        key: "embedding_model",
        kind: "model",
        choices: () => chatModels((id) => /embed|bge|e5-|gte|m3e/i.test(id)),
        read: (s) => String(s.settings.memoryEmbeddingProviderModel ?? ""),
        write: async (value) => saveSettings({ memoryEmbeddingProviderModel: value }),
      },
    ],
  },
  {
    key: "modality_bridge",
    group: "extend",
    stores: () => false,
    read: (s) => s.settings.modalityBridgeVisionEnabled === true,
    write: async (on) => {
      await updateSettings({ modalityBridgeVisionEnabled: on, visionBridgeEnabled: on });
    },
    options: [
      {
        key: "mode",
        choices: () => [{ value: "describe" }, { value: "ocr", pack: true }],
        read: () => own("modality.mode", "describe"),
        write: async (value) => {
          if (value !== "describe" && value !== "ocr") throw invalid(`未知方式 ${value}`);
          remember("modality.mode", value);
          // Never a whole-request reroute: the model the client asked for always answers.
          await updateSettings({ modalityBridgeVisionMode: "describe", visionBridgeRerouteTextOnly: false });
        },
      },
      {
        key: "vision_model",
        kind: "model",
        choices: () => chatModels((id) => isVisionModelId(id)),
        read: (s) => String(s.settings.modalityBridgeVisionModel ?? ""),
        write: async (value) => {
          await updateSettings({ modalityBridgeVisionModel: value, visionBridgeModel: value });
        },
      },
    ],
  },
  {
    key: "mcp",
    group: "extend",
    stores: () => false,
    read: (s) => s.settings.mcpEnabled === true,
    write: async (on) => {
      await updateSettings({ mcpEnabled: on, mcpTransport: "streamable-http" });
    },
    options: [
      {
        key: "actions",
        choices: () => [{ value: "off" }, { value: "on" }],
        read: () => own("mcp.actions", "off"),
        write: async (value) => {
          if (value !== "off" && value !== "on") throw invalid(`未知值 ${value}`);
          remember("mcp.actions", value);
        },
      },
    ],
  },
];

async function state(): Promise<State> {
  return {
    settings: (await getSettings()) as Record<string, any>,
    compression: (await getCompressionSettings()) as Record<string, any>,
    pack: aiPackState().installed,
  };
}

async function view(def: Def, s: State) {
  return {
    key: def.key,
    group: def.group,
    enabled: def.read(s),
    available: !def.pack || s.pack,
    stores_content: def.stores(s),
    options: await Promise.all(
      (def.options ?? []).map(async (o) => ({
        key: o.key,
        kind: o.kind ?? "choice",
        value: o.read(s),
        choices: (await o.choices(s)).map((c) => ({ value: c.value, available: !c.pack || s.pack })),
      }))
    ),
  };
}

export async function featureList() {
  const s = await state();
  const pack = aiPackState();
  return {
    edition: pack.edition,
    ai_pack: { installed: pack.installed, version: pack.version, ...packStatus(pack.installed) },
    features: await Promise.all(DEFS.map((d) => view(d, s))),
  };
}

function find(key: string) {
  const def = DEFS.find((d) => d.key === key);
  if (!def) throw invalid(`未知功能 ${key}`);
  return def;
}

export async function featureSet(key: string, enabled: boolean) {
  const def = find(key);
  const s = await state();
  if (enabled && def.pack && !s.pack) throw invalid("这个功能需要先安装 AI 组件");
  await def.write(enabled, s);
  return view(def, await state());
}

export async function featureOption(key: string, option: string, value: string) {
  const def = find(key);
  const opt = def.options?.find((o) => o.key === option);
  if (!opt) throw invalid(`未知选项 ${option}`);
  const s = await state();
  const choice = (await opt.choices(s)).find((c) => c.value === value);
  if (!choice) throw invalid(`不能选 ${value}`);
  if (choice.pack && !s.pack) throw invalid("这个选项需要先安装 AI 组件");
  await opt.write(value, s);
  return view(def, await state());
}
