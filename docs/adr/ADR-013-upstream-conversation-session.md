# ADR-013: Upstream Conversation Session Header

- **Status**: Accepted（2026-10-15，项目所有者确认）
- **Date**: 2026-10-15
- **Amends**: ADR-005（Provider 扩展点）

## Context

2026-10-15 实测：OpenCode Go 对所有模型返回 `HTTP 400: Request is missing x-opencode-session and cannot be routed efficiently`，`opencode-go` 的全部路由不可用。

官方文档（opencode.ai/docs/go，"Where can I use it"，2026-10-15 读取）的要求：

- 客户端为每段对话发送稳定的 session ID（`x-opencode-session`），用于路由优化和 prompt 缓存；
- 使用自己的 User-Agent，而不是通用 SDK / HTTP 库名称（Loadout 已发送 `Loadout/<版本>`）；
- 文档没有规定 session ID 的格式；Claude Code、Codex 各自的原生 session 头也被识别，代理转发时应保留。

现状：

- `ProviderModule` 只有 `prepare_request`（改请求体）和 `endpoint`（每个 RouteTarget 构建时固定的请求头）。Provider 看不到客户端请求，也无法按请求设置请求头。
- Gateway 只把 `anthropic-version`、`anthropic-beta`、`openai-beta` 三个客户端头转发给上游（ExecPlan 002 §9），客户端的 session 头会被丢弃。
- 经过协议转换、或客户端根本不发 session 头（如用 Chat 接口的脚本）时，没有可转发的值。

## Decision

### 1. 新的 Provider 扩展点：`session_header()`

```rust
fn session_header(&self) -> Option<HeaderName> { None }
```

Provider 声明"上游用哪个请求头接收对话 ID"。是否需要、头名是什么，由 Provider 决定；对话 ID 怎么得到，由 Gateway 统一处理。不出现 `if provider == …`（ADR-005）。

### 2. Gateway 生成对话 ID

只在被选中的 target 声明了 `session_header()` 时才计算，在选路之后进行，只使用内存中的请求数据（ADR-003 / Harness §2.2）。同一请求内只计算一次。

来源（按优先级，取第一个非空值）：

1. 客户端请求头：`x-opencode-session`、`x-loadout-session-id`、`x-claude-code-session-id`、`session_id`、`conversation_id`；
2. 请求体：`prompt_cache_key`（Responses / Chat，客户端明确给出的缓存分组键；Codex 填的是对话 ID）；
   - 不使用 Messages 的 `metadata.user_id`：很多客户端对所有对话发送同一个值，会把不同对话并成一个 session；
3. 兜底：对话开头的文本内容——system / instructions，加上第一条 user 消息。只取文本，不含 `cache_control` 等会随轮次变化的字段，因此同一段对话的后续轮次得到相同的值。

最终值 = `SHA-256(来源标签, 来源值, client id, account id)` 的前 16 字节，格式化为 UUID 字符串。

- **不直接转发原值**：客户端给的 ID 可能与其账号或设备关联，哈希后只保留"同一对话"这一事实。
- **按账号区分**：同一对话 failover 到另一个账号时得到不同的 ID，不同账号之间不能互相关联。
- **按 client 区分**：两个 client 开头相同的对话（如测速）不会共用一个 session。

### 3. 测速

主动测速（`loadout bench`、自动测速）不是对话，每次探测使用随机 UUID v4。

### 4. 隐私（ADR-008）

- 对话 ID 不写入数据库、日志或请求记录。
- 兜底方式哈希的是请求正文的一部分，但结果只发给本来就会收到这段正文的上游，不落盘。

## Alternatives

| 方案 | 否决原因 |
|---|---|
| 把客户端的 session 头加入 `FORWARDED_HEADERS` 直接转发 | 只对会发送该头的客户端有效；协议转换和普通脚本仍然失败；还会把客户端原始 ID 原样交给上游 |
| 在 `prepare_request` 里写入请求体 | 上游要求的是请求头 |
| 每个请求随机生成 session | 能通过校验，但违背"每段对话稳定"的要求，prompt 缓存失效，也可能被视为滥用 |
| 让 Provider 拿到整个客户端请求自行处理 | 扩展点过宽，每个 Provider 都要重复解析三种协议；Provider 代码会接触到更多客户端数据 |

## Consequences

- `ProviderModule` 新增一个有默认值的方法，现有 Provider 不受影响。
- Gateway 新增 `proxy/session.rs`；声明了该头的 target，每个请求多一次 SHA-256（只对对话开头的文本做哈希）。
- 以后其他上游需要对话 ID（例如 Codex 的 `session_id`）时，只需在各自的 Provider 里实现 `session_header()`。
