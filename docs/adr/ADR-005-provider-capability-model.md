# ADR-005: Provider Capability Model

- **Status**: Accepted
- **Date**: 2026-09-29

## Context

不同 Provider 能力严重不对称：有的只有 API key + HTTP，有的只能读用量不能推理，有的没有用量 API。Harness §8 / §9 要求 Provider 按能力拆分、Router 依赖 capability 而不是 Provider 名称。产品文档写的是 5 种 driver，Harness 写的是 6 种（多 Health），需要统一。

## Decision

### 1. Provider = 最多 6 个可选 driver

```text
ProviderModule
 ├─ DiscoveryDriver   本机安装 / 登录状态 / 配置位置
 ├─ AuthDriver        API key / OAuth / ExternalManaged
 ├─ UsageDriver       balance / credits / quota windows / reset / renewal
 ├─ CatalogDriver     模型、context、capabilities、protocol、price
 ├─ TransportDriver   send(request) → response / stream
 └─ HealthDriver      Provider 级探测（低频 active probe、状态页）
```

- 每个 driver 是 `provider-sdk` crate 中定义的 trait；Provider 模块只实现它具备的部分。
- Provider 模块通过一个静态描述符（`ProviderDescriptor`）声明：slug、显示名、已实现的 driver、capabilities、每个 Transport 的 Trust Level。
- **缺少任何一个 driver 都不得阻止 Provider 接入**。例如只有 Discovery + Auth + Usage 的 Provider 仍可出现在账户面板中。

### 2. Capabilities

用枚举集合表达（Harness §9 的列表为初始集合）：

```text
协议     CHAT_COMPLETIONS · RESPONSES · RESPONSES_STATEFUL · ANTHROPIC_MESSAGES
特性     STREAMING · TOOLS · VISION · REASONING
数据     USAGE_QUERY · BALANCE_QUERY · RATE_LIMIT_QUERY · MODEL_DISCOVERY
认证     API_KEY · OAUTH · LOCAL_SESSION
传输     DIRECT_HTTP · PERSISTENT_BRIDGE
策略     ZDR
```

- Capability 可以在三个层级声明：Provider、AccountModel（模型级，如 VISION）、Transport。Router 使用三者的交集。
- 新增 `RESPONSES_STATEFUL`：上游原生支持 `previous_response_id` / `store`（见 ADR-006）。

### 3. Trust Level

- 每个 Transport 与每个 Usage 数据来源都必须标注 Trust Level（`OFFICIAL_API`、`OFFICIAL_SUBSCRIPTION_API`、`OFFICIAL_CLI`、`CLI_BRIDGE`、`PRIVATE_ENDPOINT`、`WEB_SESSION`、`LOCAL`）。
- Tier A–D 只是 UI 分组，由 Trust Level 推导（映射见 glossary §3）。
- UI 必须显示 Transport 的 Trust Level。

### 4. Usage 数据带可信度

Usage driver 返回的每条 quota / balance 必须带 `source`、`confidence`（`EXACT` / `ESTIMATED` / `UNKNOWN`）、`observed_at`。见 glossary §6。

### 5. Provider 特例

- Provider 特有逻辑封装在 `crates/providers/<slug>/` 内部。
- 公共代码中禁止 `match provider_slug`；需要的差异必须表达为 capability 或 descriptor 字段。
- 需要新增 capability 或修改 driver trait 时，更新本 ADR。

### 6. Provider 文档

每个 Provider 必须有 `docs/providers/<slug>.md`，包含：实现参考、源码 license、官方认证方式、官方 API 可用性、订阅 Transport 状态、ToS 风险、最后验证日期（reference-map §6）。

## Alternatives

| 方案 | 否决原因 |
|---|---|
| 单一 `Provider` trait 包含所有方法 | 部分 Provider 需要大量 "not supported" 实现，且鼓励 provider.rs 巨型文件 |
| 动态插件（DLL / WASM） | v1 不需要第三方插件，见 ADR-007 |

## Consequences

- 能力不对称成为常态而不是异常。
- 新 Provider 可以增量交付：先 Discovery + Usage 上线账户面板，再补 Transport。
