# ExecPlan 016: OpenCode Go session header and endpoint mapping

- **Status**: In progress
- **Created**: 2026-10-15
- **Decision**: [ADR-013](../../adr/ADR-013-upstream-conversation-session.md)（Proposed）

---

## 1. Goal

恢复 `opencode-go` 的可用性：

1. 每个发往 OpenCode Go 的请求带上稳定的 `x-opencode-session`（ADR-013）；
2. 模型按官方文档映射到正确的接口（GPT / Muse Spark → Responses，Claude → Messages）。

## 2. Context

2026-10-15 用户测速时发现：

- `opencode-go/ling/*` 全部返回 `HTTP 400: Request is missing x-opencode-session and cannot be routed efficiently`（测了 `gpt-6-luna`、`gpt-5.6-luna`、`deepseek-v4-flash`）；
- 官方文档（opencode.ai/docs/go，2026-10-15 读取）的模型表里，`gpt-6-luna`、`gpt-5.6-luna`、`muse-spark-*` 走 `/responses`，`claude-haiku-5-5` 走 `/messages`。Loadout 的 `protocol_for` 只认 minimax / qwen → Messages、grok → Responses，其余都按 Chat。

## 3. Existing behavior

- `crates/providers/src/opencode_go.rs`：`endpoint()` 只设置 Bearer 认证；`protocol_for()` 按模型家族选协议。
- `crates/gateway/src/proxy/mod.rs`：请求头 = `RouteTarget.endpoint.headers`（构建路由时固定）+ content-type / accept + 透传时的三个客户端头。没有任何按请求设置的 Provider 头。
- `crates/core/src/bench.rs`：测速直接用 `endpoint.headers` 发送，不经过 Gateway。
- 模型同步（`sync_catalog`）会更新 `source = 'catalog'` 的已有模型的 `protocols_json`，所以修正映射后，重新同步即可让现有账号生效；用户用 `model add --protocol` 手动添加的模型不受影响。

## 4. Constraints

- 热路径规则（ADR-003、Harness §2.2）：对话 ID 在选路之后计算，只用内存数据，无 I/O。
- Capability-first（ADR-005）：Gateway 不出现 provider 名称判断。
- 隐私（ADR-008）：对话 ID 不写入日志或数据库；发给上游的是哈希值。
- Router / Gateway 改动必须跑 benchmark（Harness §20）。
- 默认测试不访问真实 Provider。

## 5. Architecture impact

- `provider-sdk`：`ProviderModule::session_header() -> Option<HeaderName>`，默认 `None`。
- `gateway`：新模块 `proxy/session.rs`，负责从请求中取得或推导对话 ID，并按账号派生最终值；`Attempt::run` 在 target 声明了该头时写入。
- `core/bench.rs`：声明了该头的 target，探测时附带随机 UUID v4。
- `providers/opencode_go.rs`：实现 `session_header()`；修正 `protocol_for()`。

不改数据库 schema，不改 IPC，不改路由选择逻辑。

## 6. Files expected to change

```text
crates/provider-sdk/src/module.rs
crates/gateway/src/proxy/mod.rs
crates/gateway/src/proxy/session.rs        (new)
crates/gateway/tests/proxy.rs
crates/core/src/bench.rs
crates/providers/src/opencode_go.rs
crates/benchmarks/…                         (session derivation bench)
docs/adr/ADR-013-upstream-conversation-session.md (new)
docs/adr/README.md
docs/providers/opencode-go.md
```

## 7. Implementation stages

1. **SDK 扩展点**：`session_header()`。
2. **对话 ID 推导**（`proxy/session.rs`）：
   - 来源优先级：客户端头 → `prompt_cache_key` → 对话开头文本（system / instructions + 第一条 user 消息，只取文本）；
   - `Seed`：每个请求最多计算一次（来源标签 + SHA-256 摘要）；
   - `for_account(account_id)`：派生最终 UUID 字符串。
3. **Gateway 接入**：`handle` 中惰性计算 `Seed`，`Attempt` 携带它；`run` 在构建请求头时写入。
4. **测速**：`bench::run` 附带随机 UUID。
5. **OpenCode Go**：实现 `session_header()`，修正 `protocol_for()`，更新单元测试与 provider 文档。
6. **验证**：fmt / clippy / test / benchmark；在本机安装上重新同步模型并测速（需要用户的 OpenCode Go 额度，只发测速用的极小请求）。

## 8. Tests

- `session.rs` 单元测试：
  - 三种协议下同一段对话的第 1 轮和第 3 轮得到相同的 ID（含 `cache_control` 位置变化）；
  - 第一条 user 消息不同 → ID 不同；client 不同 → ID 不同；account 不同 → ID 不同；
  - 客户端头优先于请求体，请求体优先于兜底；空值被忽略；
  - 输出是合法的 UUID 字符串，且不包含原始值。
- Gateway 集成测试（`tests/proxy.rs`）：声明了 `session_header()` 的 mock module 收到该头，两次相同对话的值一致；未声明的 module 不带该头；协议转换路径也带该头。
- `opencode_go.rs`：`protocol_for` 覆盖 gpt / muse / claude / grok / minimax / qwen / 其他；`session_header()` 返回 `x-opencode-session`。
- `bench.rs`：探测请求带该头（mock upstream）。

## 9. Performance validation

`cargo bench -p loadout-benchmarks --bench stream_proxy --bench gateway_skeleton`，与修改前对比；另测对话 ID 推导在 50 KB system prompt 下的耗时。目标：没有 session 头的 target 无可测回退；推导耗时远小于额外 TTFT 预算（P95 < 10 ms）。

## 10. Security considerations

- 原始客户端 ID 与正文不出现在发给上游的头里，只发送加入 client id 和 account id 的哈希。
- 对话 ID 不进入 tracing、请求记录或数据库。
- 头值由 UUID 格式化得到，一定是合法的 header 值，不存在注入。

## 11. Migration considerations

无 schema 变更。已有 `opencode-go` 账号需要重新同步模型才能拿到新的协议映射（`loadout model sync opencode-go <alias>` 或桌面端的同步按钮）。

## 12. Known risks

- OpenCode Go 的要求可能继续变化（文档注明"不保证继续可用"）。
- 文档里的 base URL 已变为 `https://opencode.ai/inference/go/v1`，Loadout 默认仍用 `https://opencode.ai/zen/go/v1`（2026-10-15 仍有响应）。用量接口在新路径下是否存在未知，本计划不改，记为后续事项。
- 文档中 Gemini 模型使用 Google 原生接口（`/models/<id>`），Loadout 没有该协议；这类模型仍映射为 Chat，调用会失败。
- 兜底方式下，开头相同的两段不同对话（同一 client、同一账号）会共用一个 session，只影响缓存命中，不影响正确性。

## 13. Definition of done

- 上述测试通过；`cargo fmt`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace` 通过；
- benchmark 无回退并记录在本文件；
- 本机实测：`loadout bench opencode-go/ling/gpt-6-luna` 和一个 Chat 模型成功；
- 文档更新；ADR-013 等待项目所有者确认。

## 14. Progress

- [x] 2026-10-15 复现问题、读取官方文档、写 ADR-013 与本计划
- [x] 2026-10-15 Stage 1 SDK 扩展点：`ProviderModule::session_header()`
- [x] 2026-10-15 Stage 2 对话 ID 推导：`crates/gateway/src/proxy/session.rs`（`ConversationSeed`，6 个单元测试）
- [x] 2026-10-15 Stage 3 Gateway 接入：`handle` 惰性计算 seed，`Attempt.conversation` 写入请求头；集成测试覆盖透传与 Messages → Chat 转换
- [x] 2026-10-15 Stage 4 测速：探测请求附带随机 UUID v4
- [x] 2026-10-15 Stage 5 OpenCode Go：`session_header()`、`protocol_for()`（gpt / muse → Responses，claude → Messages），provider 文档
- [x] 2026-10-15 e2e：`apps/loadout/tests/opencode_go_e2e.rs`——mock 上游缺少该头时返回 400；验证模型同步的协议、测速、同一对话 ID 稳定、Responses 请求
- [x] 2026-10-15 fmt / clippy 通过；benchmark 见下
- [x] 2026-10-15 全量 `cargo test --workspace` 通过（含新增 e2e）；提交到分支 `fix/opencode-go-session`
- [ ] 本机实测（需要用户换用新构建的 daemon，并重新同步 `opencode-go` 模型）

### Benchmarks（Harness §20）

本机，均值；基线是同一 session 内改动前在本分支上用 `--save-baseline before016` 跑的结果。

| Benchmark | 改动前 | 改动后 | criterion 判定 |
|---|---|---|---|
| `gateway_skeleton/get_models_keepalive` | 49.9 µs | 50.5 µs | 噪声范围内 |
| `stream_proxy/passthrough_200_events` | 166.3 µs | 169.9 µs | +2.5% |
| `stream_proxy/convert_chat_to_messages_200_events` | 460.2 µs | 439.3 µs | −3.7% |
| `ttft/direct_upstream` | 88.2 µs | 89.3 µs | +2.0%（不经过 Gateway，可见是本机噪声） |
| `ttft/via_gateway_passthrough` | 599.5 µs | 601.3 µs | 无变化 |
| `ttft/via_gateway_converted_messages` | 625.5 µs | 617.3 µs | 无变化 |
| `conversation_session/opening_50kb`（新增） | — | 23.7 µs | 兜底方式，50 KB system prompt + 81 条消息 |
| `conversation_session/client_header`（新增） | — | 1.5 µs | 客户端带 session 头 |

结论：没有 session 头的 target 无可测回退（这些 bench 的 target 都不声明该头，差异与完全不经过 Gateway 的 `direct_upstream` 同量级）；推导对话 ID 最坏约 24 µs，远低于额外 TTFT 预算。

## 15. Decisions

- 对话 ID 一律哈希后发送，不透传客户端原值（ADR-013 §2）。
- 不使用 `metadata.user_id` 作为来源（常被客户端设为每用户固定值）。
- 测速使用随机 UUID：探测不是对话，也不应与用户对话共享缓存。
