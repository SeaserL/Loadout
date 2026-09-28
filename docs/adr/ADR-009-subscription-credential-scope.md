# ADR-009: Subscription Credential Scope in v1

- **Status**: Accepted
- **Date**: 2026-09-29

## Context

产品文档 §4 / §31 设想 Claude Code、Codex、Kimi Code、Gemini CLI 等订阅类账号可选作为推理 Transport（"optional subscription transport"）。

风险：

- **服务条款**：多数订阅的 OAuth 凭证面向其官方客户端使用。经第三方软件转发任意客户端请求，可能违反服务条款并导致账号受限（reference-map §6 以 Gemini CLI 为例）。
- **稳定性**：平台可随时通过技术手段封堵非官方客户端访问。
- **凭证冲突**：复制 refresh token 会与官方 CLI 的 token rotation 互相干扰。

对齐阶段决定：v1 优先把 API 类账号的监测做好；订阅登录凭证只做风险较小的用量读取和状态展示。

## Decision

### 1. v1 范围

对 Tier B Provider（Codex、Claude Code、Kimi Code、Gemini CLI）及其他订阅类账号：

| Driver | v1 |
|---|---|
| Discovery | ✓ |
| Auth（状态检测，ExternalManaged） | ✓ |
| Usage（额度、reset、续费） | ✓ |
| Catalog | ✓（仅用于展示） |
| Transport | ✗ |
| Health | ✓（基于认证状态与用量数据，不发推理请求） |

- 这些账号出现在账户面板与 Usage Center 中，**不生成 RouteTarget**。
- v1 推理 Transport 只允许 Trust Level 为 `OFFICIAL_API`、`OFFICIAL_SUBSCRIPTION_API`（官方明确面向第三方开放的订阅 API，如 Provider 公开文档中的 OpenAI/Anthropic-compatible 端点）或 `LOCAL` 的 Transport。

### 2. 用量读取的来源优先级

1. 官方 API / 官方文档化的接口（包括官方 CLI 提供的 RPC，如 Codex app-server）；
2. 官方 CLI 的本地文件（会话日志、状态文件，只读）；
3. 未文档化的私有端点：允许，但必须
   - 在 `docs/providers/<slug>.md` 中记录端点、来源、风险；
   - 数据来源标记为 `PRIVATE_ENDPOINT`；
   - 使用 ExternalManaged 凭证只读访问，低频（默认 ≥ 5 min 间隔），失败后退避；
   - 用户可逐个关闭。

禁止：读取浏览器 cookie、Web session 抓取（`WEB_SESSION`）、复制或刷新官方 CLI 的 token。

### 3. Provider 文档必填项

每个订阅类 Provider 的 `docs/providers/<slug>.md` 必须包含 "Subscription Transport Status: NOT SUPPORTED in v1" 与 ToS 风险说明、最后验证日期。

### 4. 未来

若要为某个订阅开放 Transport，需要新 ADR，逐个 Provider 评估：官方是否允许、Trust Level、默认开关（至少默认关闭 + 明确警告）。

## Alternatives

| 方案 | 否决原因 |
|---|---|
| 按产品文档提供可选订阅 Transport | 服务条款与账号风险由用户承担，但产品会被视为推荐这种用法；稳定性不可控 |
| 完全不接入订阅账号 | 失去"统一看到所有 AI 资源"的核心价值 |

## Consequences

- Phase 5 工作量显著降低，Transport 相关的测试只覆盖 Tier A。
- 订阅的额度只能"看"不能"用"于 Loadout 路由；"Subscription Value Captured" 指标基于官方 CLI 本地用量数据计算。
- 参考 reference-map 时，CLIProxyAPI 在 v1 只借鉴协议转换与 SSE 部分，不借鉴订阅 OAuth Transport 部分。
