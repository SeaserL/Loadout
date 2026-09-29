# ExecPlan 004: Latency Engine, Tiers, Virtual Models and Pools

- **Status**: In progress
- **Created**: 2026-09-29
- **Phase**: Harness §26 Phase 6（Benchmark engine、Health、Quota、RouteSnapshot）+ Phase 7（按 ADR-003 收敛后的 v1 路由：realtime / fast / default + 用户 Pool）

---

## 1. Goal

- 被动测速：真实请求自动产生 TTFT / TPS / 成败样本；主动测速：低频 synthetic probe，也可由用户手动触发（`loadout bench`）。
- 每个 RouteTarget 的延迟统计（加权 P50/P90/P95 TTFT、P50 TPS、错误率）与 Latency Tier（超快 / 快 / 普通 / 未测量），带滞后。
- Virtual Models：`loadout/realtime`、`loadout/fast`、`loadout/default`；用户 Pool `loadout/<pool>`（手动顺序或按延迟排序）。
- 会话粘性 `X-Loadout-Session-ID`（ADR-003 §5）。
- Health 增加 `DEGRADED`（Harness §18）。
- CLI / IPC：`loadout bench`、`loadout pool …`、`loadout model tier …`，`loadout status` 显示 tier 与延迟。

## 2. Constraints

- 热路径（ADR-003 §1、Harness §2.2）：解析、选择只读 RouteTable 快照与 per-target 原子状态。延迟统计的写入在请求**结束后**（telemetry sink），不在选择路径上。
- 排序变化只在 control plane 的评估周期内发生，并带滞后（ADR-003 §4），避免破坏上游 prompt cache。
- Active probe 不得高频消耗额度（Harness §15）：默认每个 target 至多每 6 小时一次，且仅当近期没有被动样本；每轮最多 4 个；输出价格高于阈值的模型不自动测。
- 不记录 prompt / 输出内容（ADR-008）。

## 3. Design

### 3.1 统计单位

统计按 **account model**（`account_models.id`）聚合，即"账号 + 模型 + 渠道"。同一 account model 的不同协议变体（Chat / Messages）指向同一上游与同一模型，共用统计；这满足 Harness §16（OpenCode Go 上的 Kimi K3 与 Command Code 上的 Kimi K3 是两个独立 target）。`route_target` 字段形如 `{account_model_id}:{protocol}`，统计键取 `:` 之前部分。

### 3.2 样本与统计（`loadout-router::latency`）

- 每个 account model 一个环形缓冲（64 个样本）：`{ts, source: passive|probe, ttft_ms, tps}`。
- TTFT 只取上游为流式的请求（非流式 passthrough 没有真实 TTFT）。TPS 只取输出 ≥ 16 token 的样本。
- 统计：按样本新旧指数加权（半衰期 16 个样本，即 EWMA 权重）的 P50 / P90 / P95 TTFT 与 P50 TPS；超过 7 天的样本丢弃。
- 被动样本 ≥ 3 时只用被动样本（真实负载优先，产品文档 §14），否则使用全部样本。
- 少于 3 个可用样本 → `UNMEASURED`。

### 3.3 Tier 与滞后

- 阈值（设置项）：`latency.ultra_fast_ms` = 800，`latency.fast_ms` = 2000，按 P90 TTFT 判定。
- 死区：已处于某 tier 时，越过边界 10% 才改变（例：FAST → ULTRA_FAST 需 P90 < 720 ms）。
- 排序用的 TPS（`rank_tps`）：新值与当前值相差 > 20% 且连续 3 个评估周期才更新。
- 用户可按 account model 固定 tier（`loadout model tier <name> ultra-fast|fast|normal|auto`）。
- 评估周期 60 s（control plane 任务）；任一 target 的 tier 或 `rank_tps` 改变 → 重建路由表。

### 3.4 Virtual Models 与 Pools

| 名称 | 成员 | 顺序 |
|---|---|---|
| `loadout/realtime` | ULTRA_FAST | `rank_tps` 降序 |
| `loadout/fast` | ULTRA_FAST + FAST | tier，再 `rank_tps` 降序 |
| `loadout/default` | 所有启用的模型 | tier（UNMEASURED 最后），再 `rank_tps`；未测量按 provider / 账号顺序 |
| `loadout/<pool>` | 用户指定的 account models | `manual`：用户顺序；`latency`：同 `default` 规则 |

- 同一 tier 内 `DEGRADED` 的 target 排在后面。
- realtime / fast 只包含已测量的 target（ADR-003 §6）。
- Pool 名称遵守 slug 规则，不得与内置名称及保留名（`balanced`、`quality`、`economy`、`coding`、`batch`、`auto`、`manual`）冲突。
- 存储（migration 0004）：`pools(id, name UNIQUE, ordering, created_at, updated_at)`、`pool_members(pool_id, account_model_id, position)`（级联删除）；`account_models.tier_override`；`latency_samples`（probe 样本）。

### 3.5 会话粘性

- `X-Loadout-Session-ID`：DataPlane 内有界 LRU（10 000 个会话，2 小时 TTL）记录会话 → account model。选择时若该 account model 在候选中且 eligible，移到最前；提交后更新映射。
- 与有状态 Responses 的固定（ADR-006）相互独立：stateful 固定优先。

### 3.6 Health：DEGRADED

- `TargetHealth` 以一个原子 u64 记录最近 32 次结果（成功 / 失败位图 + 计数）。`HEALTHY` 且最近 ≥ 4 次中失败率 ≥ 25% → 显示 `DEGRADED`（仍 eligible，只影响排序）。

### 3.7 Active probe

- 请求：`Reply with exactly: OK`，`max_tokens` 16，流式，按 target 协议编码，走 target 的 endpoint 与连接池。
- 结果：TTFT（首个内容事件）、总时长、输出 token；成功 / 失败同时更新 breaker。样本写入内存统计与 `latency_samples`。
- 自动：设置 `benchmark.auto_probe`（默认 true）、`benchmark.probe_interval_hours`（6）、`benchmark.max_probe_output_price`（USD/1M，默认 30；未知价格视为可测）。跳过不可用（breaker / 额度）的 target。
- 手动：IPC `bench.run {names?}`（exact 名、`provider/model`、`loadout/<name>`；为空则全部未测量的 target），并发 4，返回每个结果。

### 3.8 启动恢复

daemon 启动时从 `request_events`（流式、成功、有 ttft）与 `latency_samples` 读取每个 account model 最近 64 个、7 天内的样本，恢复统计与 tier（无需重新测速）。

## 4. Stages

1. Router：latency 统计、tier 分类与滞后、DEGRADED、会话粘性选择；单元测试。
2. Storage + control plane：migration 0004、样本回放、virtual / pool 表构建、评估任务、telemetry tap。
3. Probe 引擎 + 自动测速任务 + `bench.run`。
4. IPC / CLI（bench、pool、model tier、status 扩展）、session header、e2e、benchmarks、文档。

## 5. Tests

- 统计：加权分位数、被动优先、过期丢弃、UNMEASURED。
- 滞后：阈值附近抖动不改变 tier；TPS 小波动不改变顺序，持续变化 3 个周期后改变。
- 选择：会话粘性（eligible 时优先、不可用时 failover）；DEGRADED 排序。
- e2e：mock 上游不同延迟 → `loadout bench` → `loadout/realtime` / `loadout/fast` 按预期路由；pool 手动顺序；会话粘性。
- Benchmark：`route_selection` 增加 virtual + session 场景，仍满足 P95 < 2 ms。

## 6. Security

- 样本只含时间与 token 数；probe 请求内容固定，不含用户数据。
- Session ID 仅保存在内存（不入库、不写日志）。

## 7. Known Risks

| 风险 | 缓解 |
|---|---|
| 大上下文请求的 TTFT 偏高，与 probe 不可比 | 被动优先且不混用（§3.2）；阈值可配置 |
| 推理模型 16 token 内无可见输出 | 以首个内容事件（含 reasoning）计 TTFT；无输出记为失败样本但不触发 breaker |
| OpenRouter 等同步出大量模型导致 probe 过多 | 每轮 ≤ 4、间隔 6 h、价格阈值；用户可禁用模型或关闭自动测速 |

## 8. Definition of Done

- [ ] §1 全部功能，测试覆盖 §5
- [ ] clippy / fmt / 全部测试通过；route_selection benchmark 满足预算
- [ ] 文档：README / glossary / ADR-003 相关更新
- [ ] 本文件移入 `completed/`

## 9. Progress

- [x] Stage 1 — router latency / tier / session（2026-09-29）
  - `LatencyBook`：每个 account model 64 个样本的环形缓冲，按新旧指数加权的分位数；被动样本 ≥ 3 时不混用 probe。
  - Tier 死区 ±10%；`rank_tps` 变化 > 20% 且连续 3 次评估才生效；用户覆盖优先。
  - `TargetHealth` 最近 32 次结果位图 → `DEGRADED`（纯原子操作）。
  - `SessionAffinity`（有界 LRU + TTL）；`select` 把会话的 account model 移到最前；virtual 模型按 tools 过滤、图片请求优先 vision 模型（exact 名称不过滤，属于用户显式选择）。
- [x] Stage 2 — storage + control plane（2026-09-29）
  - Migration 0004：`latency_samples`、`pools`、`pool_members`、`account_models.tier_override`。
  - 路由表构建时生成 `loadout/realtime|fast|default` 与用户 Pool；`LatencyTap` 在请求结束后写入被动样本；启动时从历史恢复统计；每 60 s 评估一次，排名变化才重建。
  - 设置：`latency.ultra_fast_ms`、`latency.fast_ms`、`benchmark.auto_probe`、`benchmark.probe_interval_hours`、`benchmark.max_probe_output_price`（均即时生效）。
- [x] Stage 3 — probe（2026-09-29）
  - `bench::probe`：固定 prompt、16 token、流式，经 target 自身 endpoint；结果更新 breaker；无可见输出不算故障也不产生样本。
  - 自动测速：每 10 分钟最多 4 个到期 target（按最久未测排序）；`bench.run` 手动测速，并发 4。
- [ ] Stage 4 — IPC / CLI / e2e / docs

## 10. Decisions

- 统计按 account model 聚合（§3.1）。
- ADR-003 §4 阈值保持初始值（800 / 2000 ms），改为可配置；待用户真实数据校准。
