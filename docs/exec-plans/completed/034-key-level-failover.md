# ExecPlan 034: Key-Level Failover — Per-Route Attempts, Shared Rate Limits, SenseNova Classification

## Goal
另一台设备的运行记录（`loadout/allfastmode` 3876 条、`loadout/deepseek-flash` 335 条，全部为 SenseNova 账号）显示，商汤高峰期 429 被 Loadout 的 failover 放大：

1. Pool failover 预算按 RouteTarget（协议变体）计数：每个 SenseNova 账号有 chat + messages 两个 target，`routing.max_attempts=3` 实际只试"第 1 个账号 chat → 第 1 个账号 messages → 第 2 个账号 chat"。allfastmode 397 个失败中 387 个在第 3 次后放弃，池里另外 4–5 个账号从未被尝试（deepseek-flash：153/158，12 个账号）。
2. 健康状态按 target 记录：chat 收到 429 后，同一 key 的 messages 变体仍被视为可用，立即再打同一个 key。结果 649 次 `upstream_error`（无 HTTP 状态：200 后流内报错或空流）、105 次卡满 120 s 首字超时；allfastmode 最终的 94 个 502 与 13 个 504 全部落在 messages target。
3. 分类错误：SenseNova 用同一个 429 `quota_exceeded_error` 表示限流与点数用完，通用规则（含 "quota" 且不含 "rate"）判为 `QUOTA_EXHAUSTED`，锁 1 小时；7 次中 5 次同一 key 在 1.3–4.4 分钟后即成功。流内错误一律归为 Server，客户端看到 502 而非 429。

目标：Pool / Virtual / exact failover 按路由（account model）计数；同一 key 的 429 / 额度状态在协议变体间共享；SenseNova 429 与流内限流错误归类为 `RATE_LIMITED`。

## Context
- ADR-003 §5/§7：有序粘性 failover 链，"单请求最大 failover 次数默认 3"，per-target circuit breaker；§4 已规定延迟统计按 account model 聚合（协议变体共用）。
- ADR-015 §3：专用端点已"每个账号只试一次（另一协议会撞同一个 per-key 限制）"，见 `select.rs` Endpoint 分支。
- Harness §18：每个 RouteTarget 独立维护 breaker 状态。本计划在 ADR-003 中补充例外：限流 / 额度是 key 级预算。
- `docs/providers/sensenova.md`：per-key（或 per key+model）TPM/RPM 限制，实测恢复约 20 s（`rate_limit_retry_secs: Some(20)`）。
- SenseNova 官方错误码页（2026-10-24 核对）：429 `quota_exceeded_error` =「速率/额度超限，建议指数退避重试」，没有区分点数耗尽的错误；未描述流内错误格式。积分页：通用积分 60,000 / 滚动 5 小时，600,000 / 滚动周；并说明高峰时段部分模型压力较大。
- 本机检查（2026-10-24）：12 个账号 × 2 个 DeepSeek 模型的 chat 测速全部成功，无封 IP 迹象。

## Existing behavior
- `select()`：按 account model 分组，组内 ingress 协议优先，最后 `attempts.truncate(max_attempts)` 按 target 截断。
- `proxy::handle_bytes`：按候选顺序依次尝试，循环内不再检查健康状态；失败只 `target.health.record_failure(...)`。
- `TargetHealth`：per-target；`RateLimited` 用 `retry_after` 或 30 s 起指数退避；`QuotaExhausted` 无 reset 时 1 h。
- `provider_sdk::classify`：429 + 正文像额度且不含 "rate" → `QuotaExhausted`。
- `gateway::proxy::stream_failure`：流内错误按 `type` 粗分类，未识别一律 `Server`。
- Messages 流解码器只按 `type` / SSE event 名分派，`data` 中带 `error` 对象但没有 `type` 的块被忽略。

## Constraints
- 热路径只读内存（Rule 1）：共享状态只在失败路径上做哈希查找与原子写，不访问网络 / 数据库。
- Capability-first（Rule 3）：SenseNova 特例只放在 provider 模块（preset 字段 + 钩子），gateway 不出现 provider 判断。
- 不偷偷换模型（Rule 7）：共享只在同一 account model 的协议变体之间。
- 不改 schema / IPC；`routing.max_attempts` 键与取值范围不变，只澄清语义。
- `X-Loadout-No-Failover: true` 仍只尝试一个 target。
- 修改 failover 规则前先修订 ADR-003（Rule 10）。

## Architecture impact
- Router：`select()` 按路由计数；`TargetHealth::share_block`；`RouteTable::record_failure`（记录 + 传播到兄弟变体）。
- Gateway：失败经 `RouteTable::record_failure`；每次尝试前重新检查可用性；一条路由超时 / 限流后，本请求跳过它的其余变体；流内错误交给 provider 分类。
- Provider SDK：`classify_stream_error`（通用规则移入）+ `ProviderModule::classify_stream_error` 钩子。
- Providers：`ApiPreset` 新增 `ambiguous_429` 与 `rate_limit_messages`，SenseNova 启用。
- Protocol：Messages 流解码器识别无 `type` 的 `error` 对象。
- Core：探测（bench）失败同样经 `RouteTable::record_failure`。

## Files expected to change
- `docs/adr/ADR-003-snapshot-routing.md`（§7 修订）
- `crates/router/src/{select,health,table}.rs`
- `crates/gateway/src/proxy/{mod,web}.rs`、`crates/gateway/src/plane.rs`、`crates/gateway/tests/proxy.rs`
- `crates/provider-sdk/src/{failure,module,lib}.rs`
- `crates/providers/src/presets.rs`
- `crates/protocol/src/messages.rs`
- `crates/core/src/{bench,endpoints,latency}.rs`、`crates/core/src/settings.rs`（设置说明）
- `docs/providers/sensenova.md`

## Implementation stages
1. ADR-003 §7 修订 + 本计划。
2. Router：按路由计数、`share_block`、`RouteTable::record_failure` + 单元测试。
3. Gateway：调用新接口、循环内复查、超时 / 限流后跳过同路由变体、No-Failover 仍为单 target + 集成测试。
4. 分类：SDK 流内分类 + 钩子、SenseNova preset、Messages 解码器 + 测试。
5. Core bench 接入；设置说明；provider 文档。
6. `cargo fmt`、`clippy -D warnings`、`cargo test --workspace`、benchmark 对比基线 `pre034`。

## Tests
- `select`：两个账号各两个协议，`max_attempts=1` 仍包含首个账号的两个变体，`max_attempts=2` 到第二个账号为止；stateful / endpoint 行为不变。
- `health`：`share_block` 复制 `RATE_LIMITED` / `QUOTA_EXHAUSTED` 及截止时间，不计失败次数，不缩短更长的封锁，不覆盖 `AUTH_FAILED`，不传播 Cooldown。
- `table`：`record_failure` 只影响同一 account model 的兄弟变体。
- Gateway 集成：
  - 账号 a 的 chat 返回 429 → a 的 messages 不被调用，b 成功；下一请求两个变体都被跳过；
  - 3 个账号 × 2 个协议、`max_attempts=3`：第三个账号会被尝试；
  - a 的 chat 首字超时 → 本请求不再试 a 的 messages；
  - a 的 chat 返回 5xx → 仍试 a 的 messages（协议相关故障）；
  - No-Failover 仍只有一次尝试；
  - SenseNova 模块：429 `quota_exceeded_error` → `RATE_LIMITED`，约 20 s；流内 `inference exceeds tpm/rpm limit` → 429 `rate_limit`。
- SDK / preset / decoder 单元测试。

## Performance validation
`cargo bench -p loadout-benchmarks --bench route_selection --bench stream_proxy`，先以 `--save-baseline pre034` 记录基线，改动后对比。`select()` 只多一次线性遍历；共享逻辑只在失败路径。

## Security considerations
共享时复制的 `last_error` 来自同一个 key 的上游错误文本，与现有 per-target `last_error` 同等对待（不进入 SQLite / 请求记录，只经 IPC 显示）。决策轨迹仍只记录元数据。

## Migration considerations
无 schema / 配置迁移。`routing.max_attempts` 现有值语义变宽（以路由计数）：同一请求最多可能多试同一路由的其他协议变体；超时与限流会跳过同路由变体，所以最坏首字等待仍为 `max_attempts × first_token_timeout`。

## Known risks
- SenseNova 若真的点数用完，也只按 20 s 限流处理：每 20 s 每条路由会多一次很快失败的尝试，然后转移到下一个账号。比误锁 1 小时的代价小。
- 流内错误的真实格式未抓到（导出只有元数据）；按官方 429 文案、无 `type` 的 `error` 块和通用限流措辞覆盖。需在那台设备上用 `loadout route status` 的 LAST ERROR 确认。
- 跳过同 key 的 messages 变体后，原来"chat 429 → 同 key messages 成功"的 757 次会改由下一个账号承接；高峰期如果所有账号同时被限流，失败仍会发生（上游原因）。

## Definition of done
上述行为由测试覆盖；fmt / clippy / workspace test 通过；benchmark 无明显回退；ADR-003、`sensenova.md`、设置说明已更新；计划移至 `completed/`。

## Progress
- [x] 2026-10-24 调研：导出记录分析（见 Goal）；本机商汤连通性与 24 条路由测速正常；官方错误码 / 积分文档核对；确认改动落点。
- [x] 2026-10-24 基线：`cargo bench … --save-baseline pre034`（route_selection、stream_proxy）在改代码前完成编译与测量。
- [x] Stage 1：ADR-003 §7 修订（按路由计数、key 级状态共享、本请求内跳过同路由变体、失败后复查）。
- [x] Stage 2：`select()` 按路由截断（`truncate_routes`）；`TargetHealth::share_block`；`RouteTable::record_failure`。单元测试：按路由计数、兄弟变体被封锁而其他模型不受影响、5xx 不传播、`share_block` 不计失败 / 不缩短 / 不覆盖 AUTH。
- [x] Stage 3：gateway 失败经 `table.record_failure`；`spent` 跳过超时 / 限流 / 额度后的同路由变体；本请求已有失败时复查健康状态（首个候选不复查，避免竞态下报出无原因的错误）；No-Failover 截断为 1 个 target；`UpstreamLimits.max_attempts` 说明。集成测试 4 个新增 + No-Failover 扩展，`proxy` 30/30。
- [x] Stage 4：SDK `classify_stream_error`（原 gateway `stream_failure` 规则 + `rate limit` / `too many requests` / code `429`）与 `ProviderModule::classify_stream_error`；gateway 与网页会话改用钩子；preset `ambiguous_429`、`rate_limit_messages`、`with_retry_window`；Messages 解码器识别无 `type` 的 `error` 对象。
- [x] Stage 5：`bench::probe` 接收路由表（探测 429 同样共享）、探测流内错误走钩子；`routing.max_attempts` 设置说明；`sensenova.md`。桌面端 i18n 中文说明（"每个请求最多尝试的路由数"）仍准确，且该文件有其他未提交改动，未修改。
- [x] Stage 6（2026-10-24）：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings` 通过；`cargo test --workspace`（PowerShell，含 Named Pipe / Credential Manager / e2e / headless Edge）46 个测试程序 414 passed、0 failed、1 ignored（原有）。Benchmark 对比 `pre034`：
  - 第一轮：`stream_proxy/*` 无变化；`route_selection/provider_model_100_targets` +2.8%（+14 ns）、`provider_model_100_web_gates` +7.2%（+51 ns）。同轮未改动的 `ttft/direct_upstream` 也 +3.6%，噪声约 ±4%。
  - `truncate_routes` 改为候选数 ≤ 上限时直接返回、缓存上一个 key 后重测：`provider_model_100_targets` 519.6 ns（+1.0%，噪声内）、`provider_model_100_web_gates` 530.0 ns（−24.9%）、`exact_100_targets` 226.3 ns（−4.4%）、`virtual_100_targets_with_session` 7.33 µs（无变化）。均远低于 ADR-003 §10 的 2 ms 预算。
  - 未在真实商汤高峰期复测（需要那台设备的流量）；流内错误的真实格式仍待那台设备的 `loadout route status` LAST ERROR 确认。

## Decisions
- **按路由（account model）计数，而不是按 `account_id`**：ADR-003 §4 已以 account model 为统计与会话粘性的单位；同一 key 的不同模型可能有各自的限额（sensenova.md：per key 或 per key+model），一个列了同账号多个模型的 Pool 仍应逐个尝试。对每个账号只有一个模型的 Pool（如 allfastmode），两者等价。
- **只共享 `RATE_LIMITED` / `QUOTA_EXHAUSTED`**：这是 key 级预算。5xx / 超时 / 连接失败可能只是某个协议端点的问题，不共享；`AUTH_FAILED` 可能来自协议相关的鉴权头差异，也不共享。
- **兄弟变体只复制状态与截止时间，不计失败**：OK/FAIL 统计与 degraded 判断仍反映各自 target 的真实结果。
- **本请求内，超时后跳过同路由的其他变体**：按路由计数后，每个槽位最多可能有两个 120 s 的首字等待。超时说明这个 key + 模型此刻很慢，换协议大概率同样慢，所以直接换下一条路由，最坏等待保持在原来的上限。
- **SenseNova 429 一律视为限流（20 s）**：官方文档用同一错误类型表示两种情况，并建议退避重试；实测被判为额度耗尽的 key 几分钟后即成功。
