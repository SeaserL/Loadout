# ADR-006: Canonical Protocol, Passthrough, and Responses Statefulness

- **Status**: Accepted
- **Date**: 2026-09-29

## Context

Gateway 需对外提供三种协议：OpenAI Chat Completions、OpenAI Responses、Anthropic Messages，均支持 streaming 与 tool calls（产品文档 §10 / §45）。上游也使用这几种协议之一。

两两转换的问题：

- 统一格式是最小公分母，直接转换会丢失协议特有字段（Anthropic `cache_control`、thinking block 签名，Responses 内置工具、reasoning items 等）。
- Responses API 可以有状态（`previous_response_id`、`store`、`conversation`），状态保存在上游服务端。

对齐阶段决定：无状态用法完整支持；有状态请求仅在上游原生支持 Responses 时透传并固定路由，其他情况明确报错。

## Decision

### 1. 两条路径

```text
客户端协议 == 上游协议      →  Passthrough 路径
客户端协议 != 上游协议      →  Canonical 转换路径
```

**Passthrough 路径**：

- 只解析路由所需的字段：`model`、`stream`、是否含 tools / 图片、`previous_response_id` / `store`、输入长度估算。
- 请求体其余部分原样转发（只重写 `model` 为 upstream model id）；响应流逐事件转发（只在必要时重写 `model` 字段）。
- 这是**默认与首选路径**，保真且开销最低。

**Canonical 路径**：

- `CanonicalRequest` / `CanonicalResponse` / `CanonicalStreamEvent`，由 `protocol` crate 定义。
- 每种协议实现 `decode → canonical` 与 `canonical → encode`（请求与流事件两个方向）。
- 无法表达的字段：若影响语义（例如必需的 tool 类型不被目标协议支持），返回 `UNSUPPORTED_CAPABILITY`；若只是优化提示（如 `cache_control`），丢弃并在 route decision 中记录。

### 2. Router 对协议的偏好

- 候选链排序完成后，eligibility 允许"直连或可转换"的 target；
- **同等条件下**（同一 tier、用户未手动排序），优先选择与客户端协议相同、可走 passthrough 的 target。

### 3. Responses API 状态规则

| 请求 | 处理 |
|---|---|
| 无状态（无 `previous_response_id`，`store` 为 false 或未使用存储相关功能） | 完整支持；可路由到任意协议的 target（经转换） |
| 有状态，且候选 target 具备 `RESPONSES_STATEFUL` | Passthrough，并**固定到产生该 response 的 target** |
| 有状态，但没有可用的 `RESPONSES_STATEFUL` target，或原 target 不可用 | 返回明确错误（`UNSUPPORTED_CAPABILITY` / `TRANSPORT_UNAVAILABLE`），**不静默降级为无状态** |

固定机制：

- Gateway 记录 `response_id → route_target_id` 映射（内存 LRU + SQLite 持久化，默认保留 30 天，可配置），daemon 重启后仍能正确路由。
- 该映射只含 ID，不含内容。
- 有状态请求**不参与跨 target failover**：原 target 失败即返回错误，因为其他 target 没有该上游状态。
- 对 `store` 的默认值：客户端未显式指定时，按上游协议默认行为透传，不擅自改写。

### 4. Anthropic 特有语义

- Thinking block 签名只对签发它的上游有效。跨 Provider 转换时丢弃签名；粘性路由（ADR-003）使这种情况只在 failover 时发生。
- `cache_control` 在 passthrough 中保留；转换到不支持的协议时丢弃。

### 5. Streaming

- SSE 逐事件转发，不缓冲整段响应。
- Gateway 在收到上游首个有效事件前不向客户端写出响应头，以保留 failover 能力（ADR-003 commit point）。
- 上游在提交点之后中断：按客户端协议发送错误事件并结束流，记录 `stream_interruption`。
- Usage（token 计数）从流中的 usage 事件提取；上游不提供时，请求 `stream_options.include_usage` 等价选项（若上游支持），否则标记为 `ESTIMATED`。

### 6. 测试

- 每个 `decode` / `encode` 方向都必须有单元测试与 fixture 测试（Harness §10、§19）。
- Passthrough 路径需要"字节保真"测试：除 `model` 字段外输入输出一致。

## Alternatives

| 方案 | 否决原因 |
|---|---|
| 所有请求都经过 canonical 转换 | 丢失协议特有字段；增加不必要的开销 |
| Loadout 本地实现 Responses 状态存储 | 需要持久化请求/响应正文，违反 ADR-008 默认不存正文的原则，且工作量大 |
| 有状态请求静默降级为无状态 | 会产生错误的对话上下文，属于隐藏降级 |

## Consequences

- 同协议场景下 Loadout 接近透明代理。
- 使用 Responses 有状态功能的客户端，其路由实际上被固定，failover 能力受限；这是有意的取舍。
