# Providers

每个 Provider 一份说明（ADR-005 §6）。"Subscription Transport Status" 与 "ToS Risk" 依据 ADR-009 / ADR-012 / ADR-014。

| Slug / kind | 推理 | 用量 | 文档 |
|---|---|---|---|
| `opencode-go` | Chat / Responses / Messages（按模型） | 官方 API | [opencode-go.md](opencode-go.md) |
| `commandcode` | Chat / Responses / Messages | 未公开的 alpha API（5 小时 / 每周 / 每月额度） | [commandcode.md](commandcode.md) |
| `kimi-code` | Chat / Messages | 官方 API / 官方 CLI 登录 | [kimi-code.md](kimi-code.md) |
| `openrouter` | Chat | 官方 API（余额与消费） | [openrouter.md](openrouter.md) |
| `deepseek` | Chat / Messages | 官方 API（余额） | [deepseek.md](deepseek.md) |
| `sensenova` | Chat / Messages | 估算（Token Plan 额度只在控制台） | [sensenova.md](sensenova.md) |
| `moonshot` | Chat / Messages | 官方 API（余额） | [moonshot.md](moonshot.md) |
| `zhipu` | Chat / Messages | 估算 | [zhipu.md](zhipu.md) |
| `dashscope` | Chat / Messages | 估算 | [dashscope.md](dashscope.md) |
| `volcengine` | Chat / Messages | 估算 | [volcengine.md](volcengine.md) |
| `siliconflow` | Chat / Messages | 估算（余额接口已下线） | [siliconflow.md](siliconflow.md) |
| `minimax` | Chat / Messages | 估算 | [minimax.md](minimax.md) |
| `stepfun` | Chat / Messages | 估算 | [stepfun.md](stepfun.md) |
| `qianfan` | Chat / Messages | 估算 | [qianfan.md](qianfan.md) |
| `xai` | Chat | 估算 | [xai.md](xai.md) |
| `gemini` | Chat（OpenAI 兼容端点） | 估算 | [gemini.md](gemini.md) |
| `mistral` | Chat | 估算 | [mistral.md](mistral.md) |
| `groq` | Chat | 估算 | [groq.md](groq.md) |
| `cerebras` | Chat | 估算 | [cerebras.md](cerebras.md) |
| `openai` | Chat / Responses（含有状态） | 估算 | [openai.md](openai.md) |
| `anthropic` | Messages | 估算 | [anthropic.md](anthropic.md) |
| `ollama` | Chat（本地） | — | [ollama.md](ollama.md) |
| `openai-compatible` | Chat / Responses | — | [openai-compatible.md](openai-compatible.md) |
| `anthropic-compatible` | Messages | — | [anthropic-compatible.md](anthropic-compatible.md) |
| `codex` | **不支持**（ADR-009） | 官方 CLI app-server / 本地日志 | [codex.md](codex.md) |
| `codex-sub` (`codex-subscription`) | Responses（逐账号 opt-in，`PRIVATE_ENDPOINT`，ADR-012） | — | [codex-subscription.md](codex-subscription.md) |
| `web-chatgpt` | ChatGPT 网页会话：Chat / Responses / Messages，只限文本（全局开关 + 逐账号 opt-in，`WEB_SESSION`，ADR-014） | 估算 | [web-chatgpt.md](web-chatgpt.md) |
| `workbuddy` (`workbuddy-bridge`) | 国际版已登录客户端 IPC；文本 Chat / 缓冲 SSE，逐账号 opt-in，`PRIVATE_ENDPOINT`，ADR-016 | 额度未知 | [workbuddy.md](workbuddy.md) |
| `opencode-free` (`opencode-bridge`) | 官方 v2 Agent 单轮文本 Chat / 缓冲 SSE，逐账号 opt-in，`CLI_BRIDGE`，ADR-017 | 额度未知 | [opencode-free.md](opencode-free.md) |
| `claude-code` | **不支持**（ADR-009 / ADR-012：维持只读） | 桌面应用本地文件 / 私有端点（可切换） | [claude-code.md](claude-code.md) |
| `gemini-cli` | **不支持**（ADR-009） | 私有端点 | [gemini-cli.md](gemini-cli.md) |

"估算"：由 Loadout 自己的请求记录（token 与价格）计算，置信度 `ESTIMATED`。

`sensenova` 到 `cerebras` 这 14 个是表驱动的预设（`crates/providers/src/presets.rs`，ExecPlan 018）：固定 slug、默认国内 / 官方 Base URL，国际站或专属 Plan 主机用 `loadout provider add <slug> --base-url …` 覆盖。
