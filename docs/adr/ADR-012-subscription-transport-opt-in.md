# ADR-012: Opt-in Subscription Transport (Codex)

- **Status**: Accepted（2026-10-04，项目所有者确认；Claude 选择方案 A）
- **Date**: 2026-10-04
- **Amends**: ADR-009 §1、§4（Transport 范围）；ADR-004 §2（Loadout 自有 OAuth token）；ADR-005（Provider 扩展点）

## Context

项目所有者在 2026-10-04 决定：参考 CLIProxyAPI（MIT），把 Claude 与 Codex 的订阅作为可选的推理 Transport，启用前给出警告并二次确认。

2026-10-04 的调研结论：

**Codex（ChatGPT 登录）**

- OpenAI 官方推出了 "Sign in with ChatGPT"，允许开源 / 本地应用使用用户的套餐额度。但其条款要求额度只供用户连接的那个应用自己使用，不得给其他工具提供通用 API。Loadout 作为网关，正好属于后者。
- OpenAI 员工公开表示过支持在第三方工具中使用订阅（二手报道，原帖未核实）。
- 与代理直接相关的封号证据少（CLIProxyAPI #2396 等，细节不足）。

CLIProxyAPI 的做法：

- 使用 Codex CLI 的 OAuth client 自行登录，不复用 `~/.codex/auth.json`；
- 调用 `chatgpt.com/backend-api/codex/responses`；
- 伪装程度较低：`Originator` / `User-Agent` 头，以及可选的 TLS 指纹。

**Claude（Pro / Max）**

- Anthropic 官方文档明确禁止：第三方转发订阅凭证、提供 Claude 账号登录、收集或中转 token，并保留不经通知执法的权利。
- 2026-01 起服务端拒绝不像 Claude Code 的请求；2026-04 起第三方工具的订阅流量改按额外用量计费。

CLIProxyAPI 要能继续使用，靠的是深度伪装：

- 替换 system prompt 为 Claude Code 身份；
- 用逆向得到的盐生成计费头并签名请求；
- 伪造设备 ID；
- 重命名工具；
- 伪造 TLS 指纹。

这是在规避 Anthropic 的访问控制与计费机制，而不只是"条款灰区"。

## Decision

### 1. Codex：允许，默认关闭，逐账号 opt-in

- 新 Provider `codex-subscription`（与现有只读用量的 `codex` 分开）。Trust Level 为 `PRIVATE_ENDPOINT`，属于 Tier C，只有账号显式 opt-in 后才生成 RouteTarget。
- **登录**：Loadout 自己完成 OAuth，不复制或刷新官方 CLI 的 token，因此不会与官方 CLI 的 token 轮换冲突。
  - 优先使用 device code 流程（不占用端口，适合 GUI）；
  - PKCE 回调流程作为备选，回调端口 1455 被官方 CLI 占用时报错。
- **凭证**：
  - access token 与 refresh token 各存一条 Credential Manager 记录（ADR-004 §1）；
  - id_token 只用于提取 account id、email、plan，随后丢弃；
  - SQLite 只保存 `secret_ref`、`expires_at` 与非敏感元数据。
- **刷新**：后台任务在到期前 24 小时刷新；同一账号的刷新串行合并；`refresh_token_reused` 或 `invalid_grant` 时把账号标记为需要重新登录。热路径只读内存中的当前 access token（规则 1）。
- **二次确认**：启用 Transport 时，UI / CLI 显示风险说明（条款摘要与链接、封号可能、Loadout 不承担责任），需要两步确认：
  1. 勾选"我了解风险"；
  2. 输入账号别名确认。
  每次确认都写入审计日志（不含 secret）。
- **护栏**：
  - 每账号并发上限默认 2；
  - 订阅路由默认**不进入** `loadout/realtime|fast|default` 虚拟模型，只能通过精确名称或用户手动加入 Pool 使用；
  - 收到额度耗尽信号（`usage_limit_reached` 带 reset 时间）时冷却到 reset。
- **身份**：
  - 首选如实标识（`User-Agent: loadout/<版本>`）；
  - 只有实测被拒时，才在用户另行确认后发送 Codex CLI 的 `Originator`；
  - **不做** TLS 指纹伪装。
- **协议**：
  - 上游只支持流式 Responses；非流式客户端的响应由流拼装；
  - 必需的请求体规范化（`store=false`、删除不支持字段等）通过新的 ProviderModule 请求改写钩子完成（ADR-005 扩展）；
  - 不声明 `RESPONSES_STATEFUL`（ADR-006）。
- **用量**：从响应头和流事件读取 primary / secondary 窗口，更新 Usage Center，无需额外轮询。

### 2. Claude：不实现伪装式订阅 Transport

Loadout 不实现以下任何一项：替换 system prompt 冒充 Claude Code、伪造计费头或请求签名、伪造设备 ID、TLS 指纹伪装。理由见 Context：这些手段是在规避厂商的访问控制和计费，不是一个"警告 + 二次确认"可以兜底的风险。

项目所有者于 2026-10-04 选择 **方案 A**。方案 B 记录备查，如需启用须另行 ADR。

- **A. 维持现状**：Claude 订阅只读用量；推理使用 Claude Console API key（现有 `anthropic` Provider 已支持）。
- **B. 官方 CLI 桥接**：Loadout 维护常驻的、**未修改的**官方 `claude` 进程（stream-json 模式），由用户本人登录。
  - 官方文档明确允许"终端用户用自己的订阅登录未修改的 Claude Code"，但 2026-04 起这类第三方驱动的用量可能按额外用量计费。
  - 需要常驻进程池以遵守 Harness §2.4（禁止每请求启动 CLI）。
  - 能力受 CLI 限制：工具调用语义、延迟。

### 3. ADR-009 的变化

- §1 "Transport ✗" 改为：Codex 按本 ADR §1 允许 opt-in；其余订阅保持不变。
- §4 的"逐个 Provider 评估"由本 ADR 完成 Codex 部分。

## Alternatives

| 方案 | 否决原因 |
|---|---|
| 照搬 CLIProxyAPI 的 Claude 伪装 | 规避访问控制与计费；官方明确禁止且有执法记录 |
| 走 OpenAI SIWC（每个应用自己的 OAuth client） | 官方路径，但条款禁止"给其他工具提供通用 API"，与 Loadout 的定位冲突；保留为未来选项，若 OpenAI 开放网关类应用再评估 |
| 复用 `~/.codex/auth.json` | refresh token 轮换会与官方 CLI 互相使对方失效（ADR-009 Context） |

## Consequences

- 新增 OAuth 登录与 token 生命周期管理（后台刷新、Credential Manager 串行写入）。
- ProviderModule 增加请求改写钩子，以及为每个账号提供非敏感元数据（如 `Chatgpt-Account-Id`）的途径。
- 文档：`docs/providers/codex-subscription.md`（Subscription Transport Status: OPT-IN, PRIVATE_ENDPOINT）。
- 默认测试不访问真实服务；live test 需要 `LOADOUT_LIVE_TESTS=1`，并由项目所有者用自己的账号执行。
