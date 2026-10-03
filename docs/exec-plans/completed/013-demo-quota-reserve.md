# ExecPlan 013: Quota-Aware Routing Step A (Per-Account Reserve, codex-sub Usage)

- **Status**: Completed
- **Created**: 2026-10-04
- **Phase**: 额度感知路由 第一步（docs/research/2026-10-04-feature-landscape.md §4 方案 A）

---

## 1. Goal

1. 保留阈值从全局一个值，扩展为**按账号、按窗口**设置（例如"5h 留 10%，周窗口留 20%"）。
2. `codex-sub` 账号有用量数据，阈值对它生效。

不改变 ADR-003 §9：低于阈值的账号仍然**退出候选**，不做降级或重排。

## 2. Decisions

- **存储**：migration `0008_account_quota_reserve` 给 `accounts` 加 `quota_reserve_json TEXT`，例如 `{"default":10,"5h":10,"weekly":20}`。NULL 表示使用全局 `routing.quota_reserve_percent`。
  - 目录编号为 0008（0007 预留给并行分支），在迁移列表中是第 7 项；合并时按合入顺序排列即可。
- **解析顺序**（`ReservePolicy::for_scope`）：
  1. 窗口 scope 精确匹配；
  2. 基础 scope（`weekly:opus` → `weekly`）；
  3. 账号的 `default`；
  4. 全局设置。
  - 显式写 0 表示该窗口不保留，即使全局值大于 0。
- **校验**：值必须是 0–99 的数字，窗口名非空；空对象等同于清除。
- **生效时机**：`account.set_reserve` 保存后，立即用该账号最后一次存储的用量重新计算路由状态，不必等下一次轮询。备份导出 / 导入包含该字段。
- **codex-sub 用量**（2026-10-04 对照 openai/codex `main` 源码核对）：
  - **被动（主要来源，符合 ADR-012 §1）**：推理响应头 `x-codex-{primary,secondary}-used-percent`、`-window-minutes`、`-reset-at`（Unix 秒），见 `codex-rs/codex-api/src/rate_limits.rs`；旧版 `-reset-after-seconds` 作为后备。
    - 新 SDK 钩子 `ProviderModule::usage_from_headers`，默认返回 None。
    - Gateway 在**选路之后**、收到 2xx 响应时调用它，结果交给 `TelemetrySink::observe_usage`（默认不处理）。
    - Core 的 `LatencyTap` 用 `try_send` 把结果放入有界队列（256 条，满了直接丢弃，不等待）。
    - 控制面任务 `usage::passive` 每次都更新路由状态；每账号最多 30 秒写一次数据库，并保留上次完整刷新得到的 plan 和 label。
    - 请求路径上没有数据库访问，也没有等待。
  - **主动后备**：`GET https://chatgpt.com/backend-api/wham/usage`，与官方客户端读取额度的接口相同（`codex-rs/backend-client` 的 `rate_limit_status_url`，ChatGPT 路径风格）。
    - 不发送 prompt，不消耗额度；间隔 30 分钟，只用于空闲账号。
    - ADR-012 §1 写的是"无需额外轮询"。这个低频后备轮询是否保留，由项目所有者决定；关闭方法是把 descriptor 的 `drivers.usage` 改回 false。
- **入口**：
  - IPC `account.set_reserve {provider, alias, reserve: object|null}`，桌面端 bridge 已放行；
  - CLI `loadout account reserve <provider> <alias> [--default N] [--window scope=N …] [--clear]`，不带参数时显示当前值；
  - 桌面端在账号的"套餐"对话框中增加"保留阈值（%）"和"按窗口设置"两个输入框。
  - `AccountInfo.quota_reserve` 返回当前值。

## 3. Tests

- storage：`quota_reserve_round_trips`（设置 / 读取 / `get_by_id` / 清除）；迁移往返测试自动覆盖 0008。
- core usage：
  - `reserve_policy_resolves_window_then_default_then_global`；
  - `per_window_reserve_blocks_only_the_window_below_its_threshold`；
  - `normalizes_reserve_input`；
  - `setting_a_reserve_reapplies_stored_usage`；
  - `passive_reports_are_stored_and_applied_off_the_request_path`。
- providers：
  - `parses_rate_limit_headers`、`parses_the_usage_payload`、`usage_url_sits_beside_the_codex_base`；
  - `fetches_usage_from_a_mock_backend`：mock 后端，校验 Bearer 与 `chatgpt-account-id` 请求头。
- gateway：`stream_only_upstream_serves_non_streaming_clients_and_prepares_bodies` 验证 `observe_usage` 每个响应调用一次，并带正确的 account id。
- CLI：
  - 单元测试 `reserve_options_become_ipc_values`；
  - e2e `scan_e2e`：set / show / 拒绝 150 / clear。

## 4. Benchmarks

Gateway 改动（每个 2xx 响应多一次虚函数调用；只有 codex-sub 会解析响应头）。

2026-10-04，本机，`cargo bench -p loadout-benchmarks`（中位数；括号内为 docs/performance/baseline.md 中的基线）。

| bench | 结果 |
|---|---|
| `route_selection/exact_100_targets` | 0.29 µs（0.23 µs） |
| `route_selection/provider_model_100_targets` | 1.25 µs |
| `route_selection/virtual_100_targets_with_session` | 14.0 µs |
| `ttft/direct_upstream` → `ttft/via_gateway_passthrough` | 93 µs → 502 µs（gateway 额外 ≈0.41 ms，基线 +0.43 ms） |
| `stream_proxy/passthrough_200_events` | 447 µs |

结论：Router 代码未改动，波动在噪声范围内；gateway 额外开销没有变化，远低于 P95 < 10 ms 的预算。

## 5. Progress

- [x] Storage + migration + round-trip test
- [x] Core：`ReservePolicy`、`apply_quota`、`set_account_reserve`、被动队列与任务、备份
- [x] IPC / dispatcher / bridge / CLI
- [x] codex-sub：`wham/usage` + 响应头解析；Gateway 钩子
- [x] Desktop：套餐对话框中的保留阈值输入框，i18n 新增 `// Demo: reserve` 一组键
- [x] fmt / clippy / `cargo test --workspace` / 前端构建 / benchmark

## 6. Limitations

- 只做**排除**（方案 A）。降级到链尾（方案 B）和按 reset 时间排序（方案 C）需要先修订 ADR-003。
- 修改全局 `routing.quota_reserve_percent` 后，不会立即重算，要等下一次用量刷新（这是原有行为）。
- 被动任务缓存每个账号的阈值。修改账号阈值后，最多 30 秒才会被该任务读到；`account.set_reserve` 本身会立即生效。
- 没有真实 ChatGPT 账号做联网验证。响应头名称和 `wham/usage` 字段来自官方源码，在 mock 上测试。
