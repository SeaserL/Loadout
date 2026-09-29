# ExecPlan 002: Data-Plane Foundation — Accounts, Protocols, Proxying, Telemetry

- **Status**: In progress
- **Created**: 2026-09-29
- **Phase**: Harness §26 Phase 2（canonical protocol · OpenAI-compatible provider · streaming · usage logging）+ Phase 3 的数据模型部分（provider SDK · account model）

---

## 1. Goal

让 Loadout 第一次**真正转发 AI 请求**：

```text
client ──/v1/chat/completions──▶ gateway ──▶ resolve provider/account/model
       ──/v1/responses─────────▶         ──▶ passthrough 或 canonical 转换
       ──/v1/messages──────────▶         ──▶ upstream (OpenAI-compatible / Anthropic-compatible)
                                         ◀── streaming SSE（首个内容事件前可 failover）
                                         └─▶ async telemetry → request_events
```

完成后：

- 用 CLI 添加 Provider（通用 OpenAI-compatible / Anthropic-compatible，填 base URL）、账号（alias + API key，key 进 Credential Manager）、模型（从上游 `/models` 同步或手动添加）。
- 客户端用 `provider/account/model` 或 `provider/model` 调用三种协议任意一种，Loadout 转发到上游，支持流式、tool calls、图片输入。
- 同协议 passthrough，跨协议经 canonical 转换。
- `provider/model` 在同 Provider 多个账号间按顺序 failover（429 / 额度 / 5xx / 超时 / 认证失败）。
- 每个请求的 metadata 异步写入 `request_events`（不含正文）。
- `/v1/models` 列出所有可用 exact model。

## 2. Why Phase 2 and 3 are merged

Harness 把"协议 + OpenAI-compatible provider"（Phase 2）与"account model"（Phase 3）分开。但没有账号与模型数据，代理无法端到端测试；先写一套临时配置再在 Phase 3 替换，属于 Harness 禁止的"为了先跑起来引入错误架构"。因此本计划一次建立正式的数据模型。Phase 3 剩余的 Discovery / Usage driver 随 Phase 4/5 各 Provider 实现。

---

## 3. Context

- ADR：003（路由、failover、提交点）、004（secret）、005（capability / driver）、006（passthrough / canonical / Responses 状态）、008（telemetry）、010（寻址）、011（安全）。
- 现状：gateway 对 `POST /v1/*` 返回 501；数据库只有 `settings`、`clients`。

## 4. Constraints

- 热路径只读内存：路由表（`RouteTable`，ArcSwap）在账号 / 模型变更时由控制面重建，API key 以 `Secret` 形式缓存在路由表中；请求路径不访问 SQLite、不访问 Credential Manager。
- Telemetry 通过有界 channel 发给独立 writer 线程；channel 满时丢弃并计数，绝不阻塞请求（ADR-008）。
- 请求 / 响应正文不落盘、不写日志。
- Provider 差异通过 descriptor + capability 表达；公共代码中不出现 provider slug 分支（ADR-005）。

---

## 5. Design

### 5.1 New crates

```text
crates/protocol       纯函数：canonical 类型；3 种协议的请求 / 响应 / 流事件编解码；SSE 解析与序列化
crates/provider-sdk   ProviderDescriptor、Capability、TrustLevel、driver traits、HTTP 上游客户端
crates/providers      Provider 模块实现：openai-compatible、anthropic-compatible（Phase 4/5 继续添加）
crates/router         RouteTable（名称解析 → 有序候选链）、eligibility、per-target circuit breaker
```

### 5.2 Data model（migration 0002）

```sql
providers(id, slug UNIQUE, kind, display_name, base_url, config_json, created_at, updated_at)
accounts(id, provider_id→providers ON DELETE CASCADE, alias, display_name, account_type,
         enabled, position, subscription_plan, renew_at, created_at, updated_at,
         UNIQUE(provider_id, alias))
credentials(id, account_id→accounts ON DELETE CASCADE, credential_type, source, secret_ref,
            scope, expires_at, created_at, updated_at)
models(id, canonical_name UNIQUE, display_name, context_window, max_output_tokens,
       capabilities_json)
account_models(id, account_id→accounts ON DELETE CASCADE, upstream_model_id, model_id→models,
               protocols_json, context_window, max_output_tokens, capabilities_json,
               input_price, output_price, cache_read_price, cache_write_price,
               enabled, source, created_at, updated_at, UNIQUE(account_id, upstream_model_id))
request_events(id, ts, client_id, requested_model, resolved_route, provider, account,
               upstream_model, protocol_in, protocol_out, mode, stream, status,
               error_category, http_status, attempts, input_tokens, output_tokens,
               cache_read_tokens, cache_write_tokens, reasoning_tokens, ttft_ms, duration_ms,
               output_tps, estimated_cost_usd, usage_confidence)
```

- `kind`：Provider 模块 id（`openai-compatible`、`anthropic-compatible`，Phase 4 起 `opencode-go` 等）。
- RouteTarget 不单独建表：`(account_model, protocol)` 即一个 RouteTarget，id 为 `"{account_model_id}:{protocol}"`；健康与延迟状态按此 id 保存在内存（Phase 6 持久化统计）。
- 价格单位：USD / 1M tokens。
- `credentials.secret_ref` 指向 Credential Manager；删除账号时同时删除 secret。

### 5.3 Protocol layer（`crates/protocol`）

Canonical 类型：

```text
CanonicalRequest  model, instructions, messages[{role, parts}], tools, tool_choice,
                  max_output_tokens, temperature, top_p, stop, reasoning, response_format,
                  parallel_tool_calls, user, stream
Part              Text | Image{url | base64} | ToolCall{id, name, arguments} |
                  ToolResult{call_id, content, is_error} | Reasoning{text, signature}
CanonicalEvent    Start{id, model} | TextDelta | ReasoningDelta | ToolCallStart{index,id,name} |
                  ToolCallArgs{index, delta} | Usage | Finish{stop_reason} | Error
CanonicalResponse 由事件聚合得到
```

规则：

- **上游在转换路径上一律使用流式请求**：统一 TTFT 测量与提交点逻辑；客户端要非流式时，Loadout 聚合事件后一次性返回。
- 每种协议实现：`decode_request`、`encode_request`、`StreamDecoder`（上游 SSE → canonical 事件）、`StreamEncoder`（canonical 事件 → 客户端 SSE，状态机）、`encode_response`（非流式）。
- Passthrough 只解析 `RouteHints`：model、stream、是否有 tools / 图片、`previous_response_id` / `store`、输入长度估算。请求体只改写 `model` 字段；Chat 流式请求补 `stream_options.include_usage = true`（仅当客户端未设置），以获得 usage。
- Passthrough 流式响应原样转发字节，同时旁路解析 SSE 以获得 TTFT 与 usage。

### 5.4 Request lifecycle

```text
1  auth（已有）
2  读取 body（上限 32 MiB），按路径确定 ingress 协议，解析 RouteHints
3  RouteTable.resolve(model) → 有序候选链；不存在 → MODEL_NOT_FOUND
4  对每个候选：eligibility（账号启用、breaker 状态、协议可达、capability、context）
5  选择：同一候选链中，优先与 ingress 协议相同的 target（ADR-006 §2）
6  构造上游请求（passthrough 或 canonical 编码），发送
7  提交前失败（连接失败 / 超时 / 401 / 403 / 429 / 5xx）→ 更新 breaker → 下一个候选
   400 / 404 等请求类错误 → 直接返回客户端，不 failover
8  收到首个有效内容事件 → 提交（写响应头 + 缓冲的字节），之后不再 failover
9  流结束 → telemetry 事件入队
```

- 上游超时：连接 10 s；首字节 120 s（推理模型可能思考很久）；流空闲 300 s。可配置。
- 单请求最多尝试 3 个候选（ADR-003）。`X-Loadout-No-Failover: true` 只尝试第一个。
- 有状态 Responses 请求（有 `previous_response_id`）：记录 `response_id → target` 映射（内存 LRU + SQLite，30 天），只路由到原 target，不 failover；上游不支持原生 Responses 时报 `UNSUPPORTED_CAPABILITY`（ADR-006 §3）。

### 5.5 Circuit breaker（per RouteTarget，内存原子状态）

| 上游结果 | 状态 | 恢复 |
|---|---|---|
| 429（retry-after / reset 头） | `RATE_LIMITED` | 到期；缺省 30 s 起指数退避 |
| 429/402/403 且响应表明额度耗尽 | `QUOTA_EXHAUSTED` | 缺省 1 h，Phase 4 起由 Usage driver 给出 reset |
| 5xx / 超时 / 连接失败 | `COOLDOWN` | 5 s 起指数退避，上限 5 min |
| 401 / 403 | `AUTH_FAILED` | 用户更新凭证后清除 |
| 成功 | `HEALTHY` | — |

### 5.6 Control-plane operations（IPC + CLI）

```text
loadout provider add <slug> --kind openai-compatible|anthropic-compatible --base-url <url>
loadout provider list | remove <slug>
loadout account add <provider> <alias> [--api-key-stdin]   API key 从 stdin 或交互提示读取，不出现在命令行参数中
loadout account list | enable | disable | remove | set-key
loadout model sync <provider>/<account>                     调用上游 /models
loadout model add <provider>/<account> <upstream-model> [--protocol chat|messages|responses]
loadout model list | remove | set-price
```

任何变更后重建 `RouteTable` 并通过 ArcSwap 发布。

---

## 6. Implementation Stages

- Stage 1 — `protocol` crate：canonical 类型、SSE 编解码、Chat / Messages / Responses 请求与响应编解码、流事件编解码；fixture 测试。
- Stage 2 — `provider-sdk` + `providers`：descriptor、capability、trust level、HTTP 客户端（reqwest + rustls，连接池、超时）、generic OpenAI-compatible / Anthropic-compatible（catalog + transport）。
- Stage 3 — storage migration 0002 + repositories + telemetry writer 线程。
- Stage 4 — `router` crate：`RouteTable` 构建（从 DB + secret store）、名称解析（ADR-010）、eligibility、breaker。
- Stage 5 — gateway 代理：lifecycle §5.4、passthrough 与转换、提交点、错误映射、`/v1/models`、Responses 状态映射。
- Stage 6 — IPC 方法 + CLI 命令 §5.6。
- Stage 7 — 端到端测试（模拟上游：OpenAI-compatible 与 Anthropic-compatible，含流式、tool calls、错误与 failover）、benchmark（`canonical_conversion`、`stream_proxy`、`route_selection`、`sqlite_usage_write`）、文档。

## 7. Tests

- Protocol：每种协议每个方向的单元测试与 fixture；跨协议往返（Chat → canonical → Messages → canonical → Chat 语义不变）；流式编码器输出按协议规范事件顺序校验。
- Passthrough 字节保真：除 `model`（与 Chat 的 `stream_options`）外请求体不变；响应字节原样。
- Router：解析规则、歧义防护、breaker 状态迁移。
- Gateway（模拟上游）：三种 ingress × 两种上游协议 × 流 / 非流；429 → 下一账号；提交后上游中断 → 客户端收到流内错误；400 不 failover；`X-Loadout-No-Failover`。
- Telemetry：channel 满时丢弃计数；批量写入；不含正文。
- 安全：API key 不出现在日志、错误、IPC 输出（除 `set-key` 输入）中。

## 8. Performance Validation

| Benchmark | 目标 |
|---|---|
| `route_selection`（解析 + eligibility，100 targets） | P95 < 2 ms（预期 µs 级） |
| `canonical_conversion`（典型 Chat 请求 → Messages） | P95 < 1 ms |
| `stream_proxy`（本机模拟上游，passthrough 与转换，每事件开销） | Gateway 额外 TTFT P95 < 10 ms |
| `sqlite_usage_write`（批量写 request_events） | ≥ 5,000 events/s |

## 9. Security Considerations

- API key 只经 stdin / 交互输入进入 CLI，经 IPC 传给 daemon 后立即写入 Credential Manager；IPC 请求类型对 key 字段做 Debug 脱敏。
- 发往上游的请求只携带该账号的 key；客户端的 `Authorization` / `x-api-key` 绝不转发。
- 转发的请求头白名单：`content-type`、`accept`、`anthropic-version`、`anthropic-beta`、`openai-beta`；其余客户端头部不转发。
- 上游 base URL 必须是 https，`localhost` / `127.0.0.1` 例外（本地模型）；http 的非本地地址需要显式确认（ADR-011 §5）。

## 10. Migration Considerations

- Migration 0002 新增表，不修改 0001。up/down 往返测试。

## 11. Known Risks

| 风险 | 缓解 |
|---|---|
| 三种协议语义不完全对等（内置工具、audio、logprobs 等） | 不支持的字段：影响语义 → `UNSUPPORTED_CAPABILITY`；仅优化类 → 丢弃并记录 |
| Responses API 事件种类多 | v1 编码器覆盖 text / reasoning summary / function call；其余 output item 类型在 passthrough 路径中原样转发 |
| `aws-lc-rs`（reqwest 默认 TLS provider）在 Windows 上的构建依赖 | Stage 2 验证；失败则改用 `ring` provider |
| 上游 SSE 细节各异（心跳注释、`[DONE]`、错误事件） | SSE 解析器容忍注释与空行；fixture 覆盖 |

## 12. Definition of Done

- [ ] §1 全部行为可用，端到端测试覆盖
- [ ] 所有测试、clippy、fmt 通过
- [ ] §8 基准记录
- [ ] 日志 / telemetry / 错误中无正文与 secret
- [ ] 新依赖许可证核对
- [ ] AGENTS.md / glossary 更新
- [ ] 本文件移入 `completed/`

## 13. Progress

- [x] Stage 1 — protocol（2026-09-29）
  - Canonical 模型；Chat / Messages / Responses 的请求编解码、流解码器、流编码器（状态机）、非流式响应编码；`Aggregator`；SSE 增量解析器（保留原始字节；注释 / 心跳块的字节并入下一事件，保证 passthrough 字节完整）。
  - Usage 语义：canonical `input_tokens` 含缓存（OpenAI 口径）；Anthropic 编解码时加减 cache 字段。
  - Anthropic / Responses 编码器把 tool call 缓冲到结束时整块输出（Anthropic 内容块不能交错，而 OpenAI 上游可能交错并行 tool call）；Chat 编码器实时输出。
  - Passthrough 改写只动 `model`；Chat 流式且客户端未要求 usage 时注入 `stream_options.include_usage`，并在转发时丢弃注入产生的 usage 块（`is_injected_usage_chunk`）。开启 `serde_json/preserve_order` 以保持字段顺序。
  - 37 个测试（含三协议互转、编码器输出结构校验、自解码往返）。
- [x] Stage 2 — provider-sdk + providers（2026-09-29）
  - `provider-sdk`：`Capabilities`、`TrustLevel`（含 tier 与"v1 仅 tier A 可推理"）、`ProviderDescriptor`、`ProviderModule`（transport / catalog / usage driver 默认"不支持"）、`HttpClient`（reqwest + rustls + 系统证书；loopback 绕过系统代理）、上游错误分类（401/403/402/429 额度 vs 限流/5xx/超时…，Retry-After）、`UsageReport` 数据模型（为 Phase 4/5 准备）。
  - `providers`：`openai-compatible`（Bearer；Chat + Responses；loopback 自动标记 `LOCAL`；`max_tokens_field` 设置）、`anthropic-compatible`（`x-api-key` 或 bearer；`anthropic-version`；`default_max_tokens`）、`Registry`。
  - 脱敏函数从 core 移到 `secrets::redact`，上游错误信息同样经过脱敏（上游可能回显部分 key）。
  - reqwest 默认 TLS（rustls + aws-lc-rs）在本机构建通过，无需改用 ring。
  - 17 个测试。
- [x] Stage 3 — storage 0002 + telemetry writer（2026-09-29）
  - Migration 0002：providers / accounts / credentials / models / account_models / request_events / response_routes；v1 → v2 升级保留数据有测试。
  - 目录同步语义：新模型插入；已有 catalog 模型刷新元数据，但保留用户的启用状态与已设置的价格；上游不再提供的 catalog 模型置为禁用（不删除）；手动添加的模型不受影响。
  - 删除 Provider / 账号时级联删除，并返回需要从 Credential Manager 清除的 secret_ref。
  - Telemetry：有界 `sync_channel`（默认 10,000）+ 独立线程与独立 SQLite 连接，批量事务写入（≤512 条或 200 ms）；队列满时丢弃并计数，`send` 永不阻塞（测试：1 万条写入 4 容量队列 < 2 s）。Responses 路由映射也经此线程写入。
  - 18 个测试。
- [x] Stage 4 — router（2026-09-29）
  - `RouteTable`（按 provider → 账号顺序 → alias → 模型 → 协议排序，保证确定性）+ `RouteTableHandle`（ArcSwap）；名称解析遵循 ADR-010，精确名称后接同 Provider 其他账号的同一模型（ADR-003 §8，可关闭）。
  - `select`：按候选链顺序（粘性），同一账号模型内优先 ingress 协议；过滤 v1 不允许推理的 transport（非 tier A）、context window 超限（仅在已知时）、熔断中的 target；有状态 Responses 只允许原生 Responses target、可固定到原 target、不 failover。
  - 能力"未知即允许"：目录普遍不声明 vision 等能力，只在明确不满足时排除。
  - 熔断器：原子状态；5xx/超时 5 s 起指数退避（上限 5 min）；429 用 Retry-After 或 30 s 起退避；额度耗尽默认 1 h（Usage driver 可给出准确 reset）；401/403 阻断 1 h 后半开探测，凭证更新时清除；请求类错误不计入。`HealthRegistry` 按 target id 保存状态，路由表重建不丢失。
  - 14 个测试。
- [x] Stage 5 — gateway proxy（2026-09-29）
  - `DataPlane`（路由表句柄、HTTP 客户端、`TelemetrySink`、`ResponseRouteStore`、上游超时与尝试次数）；gateway 不直接依赖数据库。
  - 请求生命周期按 §5.4 实现；首个输出事件之前的任何失败（连接、超时、非 2xx、流内错误事件、空流）都 failover；非流式客户端在整个响应聚合完成前都算"提交前"，因此同样可 failover。
  - 提交后上游中断：passthrough 按客户端协议补发错误事件（Chat 附 `[DONE]`），转换路径由编码器输出错误；记录 `upstream_interrupted`。
  - 请求类错误（400 等）直接返回客户端且不计入熔断；错误信息包含路由名与上游消息（已脱敏）。
  - 只转发白名单客户端头（`anthropic-version`、`anthropic-beta`、`openai-beta`，仅 passthrough）；客户端 key 永不转发。
  - `/v1/models` 由路由表生成；带 `anthropic-version` 头时返回 Anthropic 格式。
  - 16 个代理集成测试（脚本化 mock 上游）+ 连续 5 次运行稳定。
  - **发现**：Windows 上对端以 RST 立即中断连接时，接收方可能丢弃已到达但未读取的数据。此时 gateway 会视为提交前失败并 failover，这是可接受的行为；mock 在断开前等待 100 ms 以模拟真实上游。
- [x] Stage 6 — IPC + CLI（2026-09-29）
  - Core 控制面 `ControlPlane`：Provider / 账号 / 模型的增删改、目录同步、路由表构建与发布（串行化，阻塞工作在 blocking 线程池）；每次变更触发重建并发出 `routes.changed` 事件。
  - 路由表构建时按账号读取一次 Credential Manager，并由 Provider 模块预先生成 endpoint；要求 API key 的账号缺少 key 时，其模型不可路由（给出警告）。
  - 更换 key：新 key 写入后删除旧 secret，并清除该账号所有 target 的熔断状态。删除 Provider / 账号：同步删除 Credential Manager 中的 secret。
  - 目录同步的协议优先级：目录声明 → Provider 设置 `model_protocols` → 模块默认；同步结果列出与账号 alias 冲突的模型 id（ADR-010 §3）。
  - Telemetry sink（→ 写入线程）与 Responses 路由存储（内存 1 万条 + SQLite 回退）适配器。
  - Telemetry 写入线程改为停止标志驱动，关闭耗时有上限（即使仍有存活的 sender）。
  - 新设置：`upstream.first_token_timeout_secs`（默认 120）、`routing.max_attempts`（默认 3）。
  - 过期数据清理：restartable 后台任务每 6 小时运行（request_events 400 天、response_routes 30 天）。
  - CLI：`provider kinds|add|list|remove`、`account add|list|enable|disable|set-key|remove`、`model sync|add|list|enable|disable|price|remove`、`route status`、`logs`。API key 通过关闭回显的控制台输入或 `--api-key-stdin` 读取，从不作为命令行参数。
  - `--secret-prefix`（隐藏参数）让 e2e 测试的 daemon 使用独立的 Credential Manager 前缀；测试结束时清理。
  - 端到端测试 `proxy_e2e`：CLI 配置 → 目录同步 → 两种协议经真实 daemon 转发 → telemetry 落库 → 错误 key 触发 `AUTH_FAILED` → 恢复 → 删除 Provider 后凭据被清除；并断言数据库文件中不含上游 key。
- [ ] Stage 7 — e2e + benchmarks + docs

## 14. Decisions

- Phase 2 与 Phase 3 数据模型合并（§2）。
- 转换路径上游一律流式（§5.3）。
- RouteTarget 不建表（§5.2）。
- 模拟上游服务器用 axum 编写，不引入额外 mock 库。
