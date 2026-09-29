# ADR-003: Snapshot Routing and v1 Routing Policy

- **Status**: Accepted
- **Date**: 2026-09-29

## Context

产品文档 §3.3 / §22 与 Harness §2.2 要求热路径不做任何网络式决策，所有动态信息在后台编译成 RouteSnapshot。

对齐阶段确定了 v1 的实际需求：

- 分级只看**延迟**（TTFT，超快 / 快 / 普通三档）与**输出速度**（TPS）；模型能力强弱暂不量化。
- 用户大多数时候**不希望路由切换**；主要的切换需求是"某账号额度用完，自动切到下一个"。
- 产品文档中的加权打分（Quality / Latency / Quota / Cost / Reliability）v1 不需要。

## Decision

### 1. 两个循环

**Control Plane（后台，异步）**：health、usage、quota、balance、catalog、passive latency 统计、低频 active probe → 编译 `RouteSnapshot`。

**Data Plane（请求，同步）**：

```text
decode → resolve model name → load snapshot → eligibility filter
      → pick first eligible → transport → stream
```

### 2. RouteSnapshot

- 不可变结构，通过 `ArcSwap` 原子替换。热路径只执行 `snapshot.load()`。
- 内容：每个 Virtual Model / Pool / exact model 名称 → 已排序的 RouteTarget 候选列表，以及每个 target 的静态属性（context window、capabilities、protocol、tier）。
- **重建触发**：
  - 定时（默认 30 s）；
  - 事件触发：target 状态变化（429、401/403、quota exhausted、恢复）、配置变更、catalog 更新。
- 重建需要防抖（默认 250 ms 合并窗口）。

### 3. 快照之外的实时状态

以下状态变化太快，不能等快照重建，由 data plane 直接读写**内存原子状态**（per-target）：

- Circuit breaker 状态与 `cooldown_until`；
- 在途请求数。

Eligibility filter 同时读取快照与这些原子状态。两者都是内存操作，不访问数据库或网络。

### 4. Latency Tier

- 按 RouteTarget 计算，而非按 Model（Harness §16）。
- 依据：TTFT P90（EWMA 加权近期样本）。默认阈值：超快 < 800 ms，快 < 2000 ms，其余为普通；样本不足为 `UNMEASURED`。
  - **阈值是初始猜测**，Phase 6 用真实数据校准后更新本 ADR。
  - 2026-09-29（ExecPlan 004）：阈值改为设置项 `latency.ultra_fast_ms` / `latency.fast_ms`，默认值不变，待用户真实流量校准。统计按 account model 聚合（协议变体共用），被动样本足够时不混用 probe 样本。
- 同一 Tier 内按 TPS 从高到低排序。
- **用户手动排序优先于自动排序**。
- **滞后（hysteresis）**：排序只在差异显著时变化（默认：tier 变化，或 TPS 差距 > 20% 且持续 N 个窗口），避免因测量噪声频繁切换 target、破坏上游 prompt cache。

### 5. v1 选路规则：有序粘性 failover 链

- 每个名称对应一个**有序候选链**。
- 选择规则：**取链上第一个 eligible 的 target**。不做加权随机，不做负载均衡。
  - 这使同一个 Virtual Model 的流量稳定落在同一 target 上（天然粘性），符合"平时不切换"的需求。
- `X-Loadout-Session-ID`：同一 session 固定到首次选中的 target，直到该 target 不再 eligible。
- 有状态 Responses 请求的固定规则见 ADR-006。

### 6. Eligibility（硬条件）

```text
account enabled 且 status ∉ {AUTH_REQUIRED, QUOTA_EXHAUSTED, DISABLED}
target health ∈ {HEALTHY, DEGRADED, UNKNOWN*}
协议可达（直连或可转换）
context window ≥ 估算输入 token
capabilities ⊇ 请求所需（tools / vision / reasoning / streaming）
```

`UNKNOWN*`：仅 `loadout/default` 与 exact model 允许；`loadout/realtime` / `loadout/fast` 只使用已测量的 target（Harness §17）。

### 7. Failover

- 触发：`RATE_LIMIT`、`QUOTA`、`AUTH`、`UPSTREAM_5XX`、`UPSTREAM_TIMEOUT`（首字节前）、连接失败。
- 行为：标记当前 target 状态 → 取候选链的下一个 eligible target。**不重新计算路由**。
- **提交点（commit point）**：一旦向客户端发送了任何响应字节（含 SSE 首个事件），该请求不再 failover，上游中断时以流内错误事件结束。为此 Gateway 在收到上游首个有效内容事件前不向客户端写出响应头。
- 单请求最大 failover 次数默认 3。
- `X-Loadout-No-Failover: true` 禁用 failover。
- 状态反馈：
  - 429 → `RATE_LIMITED`，冷却至 `retry-after` / 已知 reset 时间，缺省指数退避；
  - 额度耗尽 → `QUOTA_EXHAUSTED`，至 reset；
  - 5xx / timeout → `COOLDOWN`，指数退避（起始 5 s，上限 5 min）；
  - 401/403 → `AUTH_FAILED`，需用户处理。

### 8. Exact Model 与 No Hidden Downgrade

- `provider/account/model`：只使用该 target；可选 failover 到同 Provider 其他账号的同一模型（默认开启，可关闭）。
- `provider/model`：该 Provider 下所有账号上该模型组成候选链（按用户账号顺序）。
- 跨 Provider 的同一模型 failover：需用户显式开启。
- 换成不同模型：只有 Virtual Model 允许。

### 9. Quota 处理（v1 简化版）

- v1 不实现产品文档 §21 / §42 的 pressure 评分。
- 只做：已知额度耗尽或低于用户设置的保留阈值 → 不 eligible；额度数据 `UNKNOWN` 时不预判，依赖上游实际返回。
- pressure 评分作为后续 ADR 的扩展，候选链结构不需要改变。

### 10. 性能预算

沿用 Harness §2.3：policy evaluation P95 < 1 ms，route selection P95 < 2 ms，本地 router 总开销 P95 < 5 ms，Gateway 额外 TTFT（不含网络/TLS）P95 < 10 ms。

与 Router 同等重要的是**上游连接复用**：每个 Provider host 维持 keep-alive 连接池，支持 HTTP/2 的上游复用连接；可选连接预热。

## Alternatives

| 方案 | 否决原因 |
|---|---|
| 产品文档的多因子加权打分 | 用户当前不需要；增加调参与不可预测性。结构上保留扩展空间 |
| 加权随机 / power-of-two-choices | 会让流量分散到多个 target，破坏粘性与 prompt cache |
| 每请求查询额度 | 违反 Harness §2.2 |

## Consequences

- 路由行为可预测、可解释："链上第一个可用的"。
- 负载不会自动分摊；如果一个账号有并发限制，需要靠 429 → failover 处理。
- `balanced` / `quality` / `economy` / `coding` / `batch` / `auto` 名称保留，未来以新 ADR 引入。
