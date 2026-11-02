# AxonHub 对比调研：可学习与可补充的功能（2026-10-19）

> 讨论稿，不是决定。落地前按 AGENTS.md §6 建 ExecPlan；凡是改变路由行为、数据保留或凭证存储的条目，先写或修订 ADR。
> 研究对象：[looplj/axonhub](https://github.com/looplj/axonhub)，固定 commit `a946fca495c52344ed073abb6c3562b9ff728b12`（2026-10-16）。源码 clone 在本次会话的 scratchpad，未纳入仓库。
> **许可证**：根 LICENSE 为 Apache-2.0，但根 NOTICE 与 `llm/LICENSE` 规定 **`llm/` 为 LGPL-3.0**，`llm/bedrock/` 拷贝自 anthropic-sdk-go。下文标 **[LGPL]** 的路径只能参考概念，不得复制或翻译实现。`internal/` 为 Apache-2.0，仍需按具体文件核对头部。
> 注意：AxonHub 的 `docs/` 有多处与代码不一致（见 §5），本文以代码为准。

## 0. 结论速览

AxonHub 是面向团队 / 多租户的服务端网关（Go + ent + GraphQL，可多实例 + Redis）。Loadout 是 Windows 本地单用户 daemon。两者的数据模型和威胁模型差别很大，所以本文只挑"放进 Loadout 架构里仍成立"的部分。

| 编号 | 方向 | 价值 | 工作量 | 需要 ADR？ | 建议 |
|---|---|---|---|---|---|
| 1 | 路由粘性自动识别会话（不再只认 `X-Loadout-Session-ID`） | 高 | S | 修订 ADR-003 §5 一句 | **P1** |
| 2 | 跨协议转换时自动补 prompt cache 断点 / `prompt_cache_key` | 高 | M | 否 | **P1** |
| 3 | 换 Provider 时过滤外来 reasoning signature | 中高 | S | 否 | **P1**（先写测试确认是否真的出错） |
| 4 | 用户可配的账号并发上限 + 排队 + 本地 RPM 准入 | 中高 | M | 否（扩展 ADR-012 §1 已有的机制） | **P2** |
| 5 | 定时同步模型列表（带过滤、保留手动项） | 中 | S | 否 | **P2** |
| 6 | 自定义 OpenAI 兼容站点自动探测支持的协议 | 中 | S–M | 否 | **P2** |
| 7 | 价格模型：长上下文阶梯价、5m/1h 缓存写入、分时段价格（DeepSeek 错峰） | 中 | M | 否 | **P2** |
| 8 | 额度"节奏"软规则：用量比例超过时间比例时只让已有会话继续 | 中 | M | **是**（ADR-003 §9） | **P2** |
| — | 订阅额度折算金额、提示词脱敏规则、Gemini 原生入口、Embeddings/Rerank、批量添加 key、进行中请求实时预览 | 低中 | — | 部分需要 | P3（§3） |
| — | 默认落盘正文、明文 key、热路径查库、任何错误都换渠道、身份伪装类订阅 transport | — | — | — | **不做**（§4） |

---

## 1. 值得做的（P1 / P2）

### 1.1 路由粘性自动识别会话（P1）

**AxonHub**：
- 每个请求自动解析 trace id：先看 `AH-Trace-Id`，再看 OpenCode session header、Claude Code 请求体 `metadata.user_id` 中的 session、Codex 的 `Session_id` header（默认全部开启，`conf/conf.go:227-234`；Claude Code 解析在 [LGPL] `llm/transformer/anthropic/claudecode/userid.go`）。
- 默认 `prefer_previous_channel`：把 trace / thread 上次用的渠道缓存 30 分钟，下次请求优先选它，目的是提高上游 prompt cache 命中率（`internal/server/orchestrator/candidates.go:770-860`、`internal/server/biz/request.go:1741-1806`）。
- 同一渠道内有多把 key 时，按 trace id 做 rendezvous（HRW）哈希选 key，再加 1024 项 LRU（`internal/server/biz/channel_apikey_provider.go`）。

**Loadout 现状**：
- `crates/gateway/src/proxy/session.rs` 的 `ConversationSeed` 已经能从 `x-opencode-session` / `x-claude-code-session-id` / `session_id` / `conversation_id`、`prompt_cache_key`、对话开头（system + 第一条 user）推导出稳定的会话标识。但它只用于给上游发 session header（ADR-013）。
- 路由粘性 `plane.sessions` 只认 `X-Loadout-Session-ID`（`crates/gateway/src/proxy/mod.rs:276-282`），普通客户端都不会发这个 header。

**建议**：
- 把 `ConversationSeed`（或它的一个变体）也作为 `plane.sessions` 的 key。这样 `loadout/default`、Pool 在多个账号之间选路时，同一会话能自动留在同一账号上，命中 prompt cache。
- **避开 AxonHub 的坑**：它在发上游请求*之前*就写粘性缓存（`UpdateRequestChannelID`），记住的是"最后尝试的"而不是"最后成功的"渠道，全部失败时最后一个失败渠道反而变成粘性目标。Loadout 应只在请求成功（committed）后写入。
- 仍只用内存数据、在选路时读取，不违反热路径规则。ADR-003 §5 目前写的是"`X-Loadout-Session-ID`：同一 session 固定到…"，需要补一句会话标识的来源。

### 1.2 跨协议转换时自动补 prompt cache（P1）

**AxonHub**：
- Anthropic 上游（[LGPL] `llm/transformer/anthropic/ensure_cache_control.go`）：
  - 请求带顶层 `cache_control`（自动模式）时原样透传；
  - 只要有任一 block 级 `cache_control`，就运行优化器：去掉 thinking / 空文本上的标记、裁到 4 个断点（先删最早的消息断点）、补 tools / system 锚点，且不会把生成的 5m 锚点放在客户端的 1h 锚点之前；没有消息锚点时在最后一个可缓存 block 上加一个，block 超过 20 个时在倒数约 20 处再加一个。
- OpenAI Chat 上游：没有 `prompt_cache_key` 时用 session / trace id 填入（[LGPL] `llm/transformer/openai/outbound_convert.go:48`）。
- Responses 上游：key = session id + "会话锚点"哈希（开头的 system/developer 与第一条 user，每段最多 4 KiB）。这样共用一个 session 的子 agent（如 Claude Code subagent）不会互相挤掉缓存（[LGPL] `llm/transformer/openai/responses/cache_anchor.go`）。

**Loadout 现状**：OpenAI 协议客户端经 Loadout 转到 Anthropic Messages 上游时不会加任何 `cache_control`，所以完全不走缓存；OpenAI 上游也不会自动带 `prompt_cache_key`。

**建议**：
- 只在"入口协议与上游协议不同、客户端自己没给任何缓存标记"时，在 `crates/protocol/src/messages.rs` 的编码阶段补断点。客户端已经标了就只做合法性裁剪（最多 4 个）。
- OpenAI 上游的 `prompt_cache_key` 直接用 `ConversationSeed::for_account` 的值：它已经与 client、account 绑定，不暴露原始内容。`ConversationSeed` 的"开头哈希"和 AxonHub 的会话锚点是同一个思路，可以复用。
- 这只加缓存提示，不改模型、不改内容，符合 ADR-010。需要用统计页的缓存命中率（ExecPlan 015）验证效果。

### 1.3 换 Provider 时过滤外来 reasoning signature（P1，先验证）

**AxonHub**（[LGPL] `llm/transformer/shared/signature.go` 等）：按特征识别不透明签名的来源：`gAAAA` 前缀是 OpenAI encrypted_content；base64 解码后含 `claude-…` 标记是 Anthropic；protobuf 形状是 Gemini。每个出站只回传自己认得的签名，其余丢弃。

**Loadout 现状**：`crates/protocol/src/canonical.rs:64` 的注释已经写明签名"只在发回同一 Provider 时有效"，但 `crates/protocol/src/messages.rs:283` 会把任何带 signature 的 reasoning 都编码成 `thinking` block。当 Pool / 虚拟模型在一个会话中途从 A 家换到 B 家（例如 DeepSeek 的 Anthropic 兼容端点 → Anthropic 官方），B 可能因为签名无效返回 400。

**建议**：
1. 先写一个 gateway 测试复现：会话里带上一家签发的 signature，failover 到另一家，观察上游错误与 `FailureKind` 分类。
2. 确认会出错后，在编码阶段根据目标 Provider 决定是否保留签名：首选按 1.1 的会话粘性记录"上一轮由哪个 Provider 回答"；无法判断时再用签名特征判断。丢掉签名时降级为普通文本或直接省略 thinking 内容。

### 1.4 账号并发上限 + 排队 + 本地 RPM 准入（P2）

**AxonHub**（`internal/server/orchestrator/channel_limiter.go`、`channel_limiter_manager.go`、`channel_request_tracker.go`）：
- 每渠道严格信号量 + FIFO 队列，空出的位置直接交给队首；`MaxConcurrent`、`QueueSize`、`QueueTimeoutMs` 可配。队满 / 超时变成本地合成的 429，触发换渠道。
- RPM 是请求前硬准入（按自然分钟计数）；TPM 只在请求后累加。
- 打分随负载单调下降，所以排队中的渠道会自然排到后面。

**Loadout 现状**：`crates/gateway/src/plane.rs:104` 的 `AccountSlots` 只给声明了并发上限的模块用（ADR-012 §1），用户不能配置。商汤这类"各模型独立 TPM/RPM，超限 429"的平台（`docs/providers/sensenova.md`），目前只能等 429 后再冷却。

**建议**：
- 账号级设置 `max_concurrent` / `requests_per_minute`。超限时视为"本地跳过"，在决策链里记为 `local limit`，**不**计入熔断与健康状态。AxonHub 的本地拒绝很可能被错误计入了失败统计（推断，代码中没有过滤），这一点要避开。
- 排队是否做：本地单用户场景下，"立即换下一个账号"通常比排队更符合 ADR-003 的低延迟目标。建议先只做准入；排队只给专用端点（ADR-015）作为可选项。

### 1.5 定时同步模型列表（P2）

**AxonHub**（`internal/server/biz/channel_model_sync.go`）：
- 每小时一次（cron `11 * * * *`），只处理开启了 `auto_sync_supported_models` 的渠道，可以用正则过滤（`auto_sync_model_pattern`）。
- 拉取结果来自兜底数据时放弃本次同步。
- 写入在事务里进行：重新读一次渠道，避免覆盖拉取期间用户的手动修改；合并"手动 + 拉取"的模型；清理已删除模型的协议覆盖；补上价格行。

**Loadout 现状**：`crates/core/src/control.rs:793` 的 `sync_models` 只在添加账号或手动触发时运行；后台任务里没有模型同步（现有任务：auto-backup / latency / notify / retention / usage）。

**建议**：增加 `spawn_restartable("catalog")` 后台任务，默认每 6–24 小时一次，复用 ExecPlan 011 的 `price_source` 规则（`user` 价格不被覆盖）。新出现的模型默认不启用，只发一条通知；消失的模型标记为不可用但不删除，以免破坏 Pool 配置。

### 1.6 自定义站点自动探测协议（P2）

**AxonHub**（`internal/server/biz/channel_endpoint_detect.go`）：
- 并行发三个 1 token 的 ping：Chat Completions、Responses、Anthropic Messages，每个超时 20 秒。
- 判定规则：2xx / 429 / 其他 4xx（如 400、422）算"支持"，因为路由存在；404 / 405 算不存在；401 / 403 算鉴权错误；5xx 算服务端错误；连接失败算不可达。
- 探测从不修改渠道配置。

**Loadout**：`loadout provider add --kind openai-compatible` 需要用户自己知道站点支持哪些协议。中转站常常同时支持 `/v1/messages`，用户不知道就用不上 Anthropic 原生协议。

**建议**：在 Core 控制面（`check.rs` 旁边）实现 `provider.detect`。添加 openai-compatible / anthropic-compatible 账号时可选执行，结果显示给用户确认后再写入。每次探测会消耗 1–3 个 token，需要用户主动触发。

### 1.7 更完整的价格模型（P2）

**AxonHub**（`internal/objects/price.go`、`internal/server/biz/cost_calc.go`）：
- 计价方式：按次（`flat_fee`）、按量（`usage_per_unit`）、累进阶梯（`usage_tiered`）、整量落档（`usage_volume`）。
- 缓存写入区分 5m / 1h 两种价格。
- 分时段价格：`Schedule.Overrides` 按时区、每天 HH:mm 区间（支持跨午夜）、星期、日期范围匹配，优先级数字小的生效。
- 每次改价生成 `ChannelModelPriceVersion`，用量记录保存 `cost_price_reference_id`；用 decimal 计算，不用浮点。

**Loadout 现状**：`crates/router/src/target.rs:17` 的 `Prices` 只有 input / output / cache_read / cache_write 四个可选值。

**建议**：
- 分时段价格对国内用户有实际意义（DeepSeek 错峰价）；阶梯价对长上下文定价（Gemini、Claude 长上下文）有意义。
- 实现方式：价格表新增可选的 `tiers_json` / `schedule_json`，用 migration 加列；`Prices::cost` 收到 token 数与时间戳后计算。比价页（ExecPlan 011）同步显示"当前时段价格"。
- 价格版本可以先不做：Loadout 的 `request_events` 本来就保存每次请求算好的费用。

### 1.8 额度"节奏"软规则（P2，需 ADR）

**AxonHub**（`internal/server/biz/provider_quota/routing.go:104-114`、`internal/server/orchestrator/candidates_quota.go`）：
- 当某个窗口的"已用比例"超过"已过时间比例"（例如 5 小时窗口过了 40%，额度却已用 70%），该渠道变成 `sticky_only`：只有已经粘在它上面的会话继续用，新会话去别处。
- 额度来自后台每 5 分钟轮询，热路径只读内存缓存。
- 三种模式：`remove_on_exhausted`（默认）、`backpressure`、`ignore_quota`。所有候选都被额度规则排除时返回明确的 `QuotaExhausted`，而不是"模型无效"。

**与 Loadout 的关系**：ExecPlan 013 的按窗口保留阈值是"硬退出"。节奏规则是更平滑的补充，和 1.1 的会话粘性配合最好：既保住已有会话的缓存，又让新会话分流。它改变选路行为，需要修订 ADR-003 §9（2026-10-04 调研 §4 已列为 P2）。AxonHub 这套判定可以作为具体算法参考。

---

## 2. 我们已经做对、无需再学的

| 方面 | AxonHub | Loadout |
|---|---|---|
| 流式 failover 的提交点 | 首个有意义内容到达前缓冲、可重试，之后不再重试（[LGPL] `llm/pipeline/stream.go:189-302`） | 相同：`crates/gateway/src/proxy/meter.rs:63` `committed()` = 已有 first_content |
| Retry-After | 只在 429 带 header 时冷却，默认 60 秒，上限 5 分钟 | `FailureKind::RateLimited { retry_after }`，带指数退避（`crates/router/src/health.rs`） |
| 按请求特征过滤候选 | 规则里的 `When` 条件：估算 token、是否有图片 / 视频、header、时间（`internal/server/orchestrator/candidates_condition.go`） | `crates/router/src/select.rs` 已按上下文窗口、图片、工具能力过滤（capability-first） |
| 子 agent 缓存分离 | Responses 会话锚点 | `ConversationSeed` 的对话开头哈希（只是还没用到路由和缓存 key，见 1.1 / 1.2） |
| 决策透明 | 只有 debug 日志里的打分明细 | 每条请求日志都有决策链（ExecPlan 015） |
| 账号终态 / 检测 | 按规则自动停用，到期后**不检测**直接恢复 | 终态 + 主动检测（ExecPlan 014）更稳妥 |

---

## 3. 可选的小功能（P3）

- **订阅额度折算金额**（`internal/server/biz/provider_quota_cost.go`）：用 Loadout 自己算出的窗口内花费 ÷ 上游报告的用量比例，估算"这个 5 小时窗口约等于 $X"。只用于展示，S 级工作量。
- **提示词保护规则**（`internal/server/biz/prompt_protection_rule.go`、`internal/server/orchestrator/prompt_protection.go`）：按角色范围用正则匹配，`mask` 替换或 `reject` 拒绝，例如在发给第三方中转站前遮掉 `sk-…` 之类的密钥。它运行在选路之后的请求变换阶段，不受热路径规则限制，但每个请求要多跑若干正则；需要预编译、限制规则数量并跑 benchmark。AxonHub 落库的仍是**未脱敏的原始正文**（推断），Loadout 本来不落正文，没有这个问题。
- **Gemini 原生入口**（`/v1beta/models/*action`，`internal/server/api/gemini.go`）：有了它，Gemini SDK / Gemini CLI 才能把 Loadout 当上游，可以补上 2026-10-04 调研 §1.1 里"Gemini CLI 不适用"那一格。需要新的 canonical 映射，M–L 级工作量。
- **Embeddings / Rerank / Images 端点**：AxonHub 都有。Loadout 定位是对话与编码 agent，按需再加。
- **批量添加 key**（`internal/server/biz/channel_bulk.go`）：一份配置 × N 个 key，一次建多个账号；对多个商汤账号的用户有用。别名需要自动生成后让用户确认（ADR-010）。
- **进行中请求的实时预览**（`internal/server/biz/stream_preview.go`、`internal/server/api/request_live.go`）：引用内存中正在流式传输的 chunk 缓冲，不复制、不落盘。即使不落盘，也是在 UI 中展示正文，需要先确认是否符合 ADR-008，并且默认关闭。
- **Responses 跨渠道续接**（`internal/server/orchestrator/responses_session.go`）：内存保存"已认证"的完整快照（2 小时、2048 条、64 MiB），在网关侧把 `previous_response_id` 展开为完整输入，于是有状态请求也能 failover。无法安全展开时，在发出请求前返回 `previous_response_not_found` 并提示"重发完整输入"，且不计入渠道健康。ADR-006 目前规定"有状态请求固定路由，否则报错"；这个方案在内存中保存正文，需要同时修订 ADR-006 和 ADR-008。其中"恢复类错误不惩罚健康状态"一条可以单独借鉴。

---

## 4. 不建议照搬

- **默认保存请求 / 响应正文**：`store_request_body` 与 `store_response_body` 默认都为 true，GC 清理默认全部关闭。这与 ADR-008 正好相反。
- **明文 secret**：API key 明文存在数据库里，备份文件也包含明文渠道凭证。与 ADR-004 冲突。Loadout 的加密导出（ExecPlan 020）更好。
- **热路径查库**：每个请求都会执行 trace / thread 的 `GetOrCreate`、额度检查的 `COUNT` / `SUM`、不带缓存的 `GetModelByModelID`，还有发上游前的 `Request` / `RequestExecution` 插入。从未保存过的设置项不进缓存，每次都会查库。这些都违反 Harness §2.2。
- **任何错误都换渠道**：换渠道时不判断能否重试，400 / 401 / 404 都会换下一个；加上默认每次重试固定等 1 秒，最坏情况会尝试 4 个渠道 × 3 次 = 12 次。Loadout 按 `FailureKind` 分类决定是否 failover，应保持。
- **性能记录可能阻塞请求**：结果写入容量 1024 的 channel，worker 跟不上时，发送方的请求 goroutine 会被阻塞。Loadout 的 telemetry sink 不能这样做。
- **身份伪装类订阅 transport**：Claude Code（伪造 system 行、`metadata.user_id` 和 UA）、Copilot（伪装 VS Code，推断 `X-Initiator` 影响计费）、Antigravity（使用官方 app 的 client secret）、xAI（用网页 SSO cookie 自动批准 device flow），以及给原版 Codex CLI 伪造 ChatGPT 身份的兼容模式。都超出 ADR-009 / ADR-012 的范围，并有 ToS 风险；AxonHub 自己的文档也因风控把 Claude Code 渠道降为不推荐。

---

## 5. AxonHub 文档与代码不一致（阅读时注意）

- `docs/en/guides/load-balance.md` 写有 "Trace Aware 1000 分"策略；代码中粘性是排序前的固定置顶，不是打分。
- 文档写着未配置 `MaxConcurrent` 时有 `ConnectionTracker` 兜底；代码中不存在这个组件。
- 文档说 RPM 是滑动窗口；代码是自然分钟计数。
- 文档说 Claude Code trace 提取需要手动开启；代码默认开启。
- prompt protection 文档说用 Go RE2；实际用的是 `dlclark/regexp2`（支持 lookahead，回溯型，有 ReDoS 风险）。
- 模型通配：`gpt-*` 按正则理解，意思是 "gpt" 后跟零个或多个 `-`，不是 glob。Loadout 的 `allowed_models` 只支持结尾 `*` 的前缀匹配，语义更不容易出错，应保持。
- 2026-10-04 调研 §7 把 AxonHub 标为 Apache-2.0，不完整：`llm/` 是 LGPL-3.0（见本文开头）。

---

## 6. 参考文件清单（commit `a946fca`）

| 主题 | 路径 | 许可证 |
|---|---|---|
| 会话识别 / 粘性 | `internal/server/middleware/trace.go`、`internal/server/orchestrator/candidates.go`、`internal/server/biz/request.go`、`internal/server/biz/channel_apikey_provider.go`、`llm/transformer/anthropic/claudecode/userid.go`、`llm/transformer/openai/codex/headers.go` | Apache-2.0 / [LGPL] |
| Prompt cache | `llm/transformer/anthropic/ensure_cache_control.go`、`llm/transformer/openai/responses/cache_anchor.go`、`llm/pipeline/cc/system_messages.go` | [LGPL] |
| Reasoning 签名 | `llm/transformer/shared/signature.go` | [LGPL] |
| 并发 / RPM | `internal/server/orchestrator/channel_limiter.go`、`channel_limiter_manager.go`、`channel_request_tracker.go`、`rate_limit_tracking.go`、`internal/server/biz/channel_rate_limit.go` | Apache-2.0 |
| 负载均衡 / 熔断 | `internal/server/orchestrator/load_balancer.go`、`lb_strategy_*.go`、`internal/server/biz/model_circuit_breaker.go`、`channel_metrics.go`、`channel_auto_disable.go` | Apache-2.0 |
| 重试 / 流式提交 | `internal/server/orchestrator/retry.go`、`outbound.go`、`llm/pipeline/pipeline.go`、`llm/pipeline/stream.go` | Apache-2.0 / [LGPL] |
| 额度节奏 | `internal/server/biz/provider_quota.go`、`internal/server/biz/provider_quota/routing.go`、`internal/server/orchestrator/candidates_quota.go`、`internal/server/biz/provider_quota_cost.go` | Apache-2.0 |
| 模型同步 / 探测 | `internal/server/biz/channel_model_sync.go`、`model_fetcher.go`、`channel_endpoint_detect.go`、`channel_bulk.go` | Apache-2.0 |
| 价格 | `internal/objects/price.go`、`internal/server/biz/cost_calc.go`、`channel_price.go`、`catalog.go` | Apache-2.0 |
| Responses 续接 | `internal/server/orchestrator/responses_session.go`、`responses_attempt.go`、`internal/server/api/responses_websocket.go`、`llm/transformer/openai/responses/websocket_executor.go` | Apache-2.0 / [LGPL] |
| 提示词保护 / 请求覆盖 | `internal/server/biz/prompt_protection_rule.go`、`internal/server/orchestrator/prompt_protection.go`、`internal/server/biz/channel_override.go`、`internal/server/orchestrator/override.go` | Apache-2.0 |
| 观测 / 保留 | `internal/server/biz/trace.go`、`internal/server/biz/stream_preview.go`、`internal/server/gc/gc.go`、`internal/server/backup/` | Apache-2.0 |
