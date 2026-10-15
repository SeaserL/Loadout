# Providers

每个 Provider 一份说明（ADR-005 §6）。"Subscription Transport Status" 与 "ToS Risk" 依据 ADR-009 / ADR-012 / ADR-014。

| Slug / kind | 推理 | 用量 | 文档 |
|---|---|---|---|
| `opencode-go` | Chat / Responses / Messages（按模型） | 官方 API | [opencode-go.md](opencode-go.md) |
| `commandcode` | Chat / Responses / Messages | 未公开的 alpha API（5 小时 / 每周 / 每月额度） | [commandcode.md](commandcode.md) |
| `kimi-code` | Chat / Messages | 官方 API / 官方 CLI 登录 | [kimi-code.md](kimi-code.md) |
| `openrouter` | Chat | 官方 API（余额与消费） | [openrouter.md](openrouter.md) |
| `deepseek` | Chat / Messages | 官方 API（余额） | [deepseek.md](deepseek.md) |
| `openai` | Chat / Responses（含有状态） | 估算 | [openai.md](openai.md) |
| `anthropic` | Messages | 估算 | [anthropic.md](anthropic.md) |
| `ollama` | Chat（本地） | — | [ollama.md](ollama.md) |
| `openai-compatible` | Chat / Responses | — | [openai-compatible.md](openai-compatible.md) |
| `anthropic-compatible` | Messages | — | [anthropic-compatible.md](anthropic-compatible.md) |
| `codex` | **不支持**（ADR-009） | 官方 CLI app-server / 本地日志 | [codex.md](codex.md) |
| `codex-sub` (`codex-subscription`) | Responses（逐账号 opt-in，`PRIVATE_ENDPOINT`，ADR-012） | — | [codex-subscription.md](codex-subscription.md) |
| `web-chatgpt` | ChatGPT 网页会话：Chat / Responses / Messages，只限文本（全局开关 + 逐账号 opt-in，`WEB_SESSION`，ADR-014） | 估算 | [web-chatgpt.md](web-chatgpt.md) |
| `claude-code` | **不支持**（ADR-009 / ADR-012：维持只读） | 桌面应用本地文件 / 私有端点（可切换） | [claude-code.md](claude-code.md) |
| `gemini-cli` | **不支持**（ADR-009） | 私有端点 | [gemini-cli.md](gemini-cli.md) |

"估算"：由 Loadout 自己的请求记录（token 与价格）计算，置信度 `ESTIMATED`。
