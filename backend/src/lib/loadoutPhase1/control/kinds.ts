// SPDX-License-Identifier: Apache-2.0
// Provider presets: Loadout's built-in platforms (crates/providers/src/presets.rs) as native
// OpenAI-compatible nodes. `usage` names the OmniRoute usage fetcher for that platform.
export type Kind = {
  kind: string;
  display_name: string;
  fixed_slug: string | null;
  default_base_url: string | null;
  auth: "api_key" | "optional_api_key" | "external_managed" | "oauth" | "browser_login";
  protocols: string[];
  requires_transport_opt_in?: boolean;
  drivers: string[];
  usage_sources: string[];
  api_type?: "chat" | "responses";
  usage?: string;
  group: "platform" | "relay" | "subscription" | "local" | "custom";
  hint?: string;
};

const CHAT = ["chat", "responses", "messages"];
const preset = (
  kind: string,
  display_name: string,
  default_base_url: string,
  group: Kind["group"] = "platform",
  extra: Partial<Kind> = {}
): Kind => ({
  kind,
  display_name,
  fixed_slug: kind,
  default_base_url,
  auth: "api_key",
  protocols: CHAT,
  drivers: ["inference", "models"],
  usage_sources: [],
  group,
  ...extra,
});

export const KINDS: Kind[] = [
  preset("deepseek", "DeepSeek", "https://api.deepseek.com/v1", "platform", { usage: "deepseek" }),
  preset("sensenova", "SenseNova 商汤日日新", "https://token.sensenova.cn/v1"),
  preset("moonshot", "Moonshot AI (Kimi API)", "https://api.moonshot.cn/v1", "platform", {
    usage: "moonshot",
  }),
  preset("zhipu", "智谱 GLM", "https://open.bigmodel.cn/api/paas/v4"),
  preset("dashscope", "阿里云百炼 Qwen", "https://dashscope.aliyuncs.com/compatible-mode/v1"),
  preset("volcengine", "火山方舟 Doubao", "https://ark.cn-beijing.volces.com/api/v3"),
  preset("minimax", "MiniMax", "https://api.minimaxi.com/v1", "platform", { usage: "minimax" }),
  preset("stepfun", "阶跃星辰 StepFun", "https://api.stepfun.com/v1"),
  preset("qianfan", "百度千帆", "https://qianfan.baidubce.com/v2"),
  preset("openai", "OpenAI", "https://api.openai.com/v1"),
  preset("gemini", "Google Gemini API", "https://generativelanguage.googleapis.com/v1beta/openai"),
  preset("xai", "xAI (Grok)", "https://api.x.ai/v1", "platform", { usage: "xai" }),
  preset("mistral", "Mistral AI", "https://api.mistral.ai/v1"),
  preset("groq", "Groq", "https://api.groq.com/openai/v1"),
  preset("cerebras", "Cerebras", "https://api.cerebras.ai/v1"),
  preset("siliconflow", "硅基流动 SiliconFlow", "https://api.siliconflow.cn/v1", "relay"),
  preset("openrouter", "OpenRouter", "https://openrouter.ai/api/v1", "relay", {
    usage: "openrouter",
  }),
  preset("commandcode", "Command Code", "https://api.commandcode.ai/provider/v1", "relay"),
  preset("opencode-go", "OpenCode Go", "https://opencode.ai/zen/go/v1", "relay", {
    api_type: "responses",
    usage: "opencode-go",
  }),
  {
    kind: "workbuddy",
    display_name: "WorkBuddy",
    fixed_slug: "workbuddy",
    default_base_url: null,
    auth: "external_managed",
    protocols: CHAT,
    drivers: ["inference", "models"],
    usage_sources: [],
    group: "subscription",
    hint: "复用本机已登录的 WorkBuddy 桌面端，在“扫描本机”里连接。",
  },
  {
    kind: "codex-subscription",
    display_name: "ChatGPT / Codex 订阅",
    fixed_slug: "codex-sub",
    default_base_url: "https://chatgpt.com/backend-api/codex",
    auth: "oauth",
    protocols: CHAT,
    requires_transport_opt_in: true,
    drivers: ["inference", "models"],
    usage_sources: [],
    api_type: "responses",
    group: "subscription",
    hint: "用设备码登录；默认不参与路由，需要你逐账号确认开启。",
  },
  {
    kind: "openai-compatible",
    display_name: "OpenAI 兼容（自定义）",
    fixed_slug: null,
    default_base_url: null,
    auth: "api_key",
    protocols: CHAT,
    drivers: ["inference", "models"],
    usage_sources: [],
    group: "custom",
  },
  {
    kind: "local-openai",
    display_name: "本机服务（Ollama / LM Studio）",
    fixed_slug: null,
    default_base_url: "http://127.0.0.1:11434/v1",
    auth: "optional_api_key",
    protocols: CHAT,
    drivers: ["inference", "models"],
    usage_sources: [],
    group: "local",
  },
];

export function kindOf(name: string, baseUrl: string | null): Kind {
  const exact = KINDS.find((k) => k.kind === name || k.fixed_slug === name);
  if (exact) return exact;
  if (baseUrl) {
    const byUrl = KINDS.find((k) => k.default_base_url && k.default_base_url === baseUrl);
    if (byUrl) return byUrl;
  }
  return {
    ...(KINDS.find((k) => k.kind === "openai-compatible") as Kind),
    display_name: name,
  };
}
