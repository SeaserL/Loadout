# ExecPlan 014: Account Checks and Terminal Auth States (demo)

- **Status**: Completed
- **Created**: 2026-10-04
- **Background**: docs/research/2026-10-04-feature-landscape.md §5（健康与检测、账号终态）

---

## 1. Goal

1. 每个账号可"检测"：用 Catalog 调用（`list_models`）判断 key 有效 / 失效 / 停用 / 限流 / 欠费 / 无法连接；可选对每个已启用模型发 1 token 请求，报告成功与否、TTFT 和流式健康。
2. 区分"暂时冷却"和"终态"认证失败：终态不自动恢复，直到用户处理。

## 2. Decisions

- **IPC** `account.check {provider, alias, models?}` → `AccountCheckResult`（追加在 `crates/ipc/src/api.rs` 末尾）。在 Core 控制面执行（`crates/core/src/check.rs`），热路径不等待它。
  - status：`ok` / `invalid_key` / `account_disabled` / `rate_limited` / `no_credit` / `unreachable` / `upstream_error` / `unsupported`（Provider 没有 Catalog 调用）/ `missing_key`。
  - `models: true` 复用 bench 的探测（`bench::probe` / `probe_all`，延迟样本照常记录），每个模型返回 ok、TTFT、错误和 `stream`：`clean`（收到 finish 或协议结束标记）/ `truncated`（有事件但没有结束）/ `malformed`（事件无法解码）/ `empty`（200 但没有事件）。
  - 检测结果也作用于熔断器：`ok` 清除该账号所有 target 的 `AUTH_FAILED`（含终态）；`invalid_key` 记一次 Auth 失败；`account_disabled` 直接进入终态。`unsupported` 时，只要有一个模型探测成功也视为凭证有效。
- **终态**（`crates/router/src/health.rs`）：仍是 `AUTH_FAILED`，`until_ms = i64::MAX`，所以 `is_available` 不变、热路径没有新增工作。进入条件：
  - 连续第二次 Auth 失败（1 小时后的半开重试仍被拒，或并发请求同时被拒）；
  - 新的 `FailureKind::AccountDisabled`：401/403 且正文含 deactivated / suspended / banned / account disabled / organization has been disabled 等（`looks_like_account_disabled`，流内 authentication/permission 错误同样判断）；
  - OAuth refresh token 失效（原来记一次普通 Auth 失败）。
  - 终态不会被之后的其他失败降级为可恢复状态，也不会被 Usage 的额度标记覆盖。
- **解除**：成功的 `account.check` 或 bench 探测（`record_success`）、`account.set_key`、OAuth 重新登录或成功刷新 token、用户重新启用账号（`account.set_enabled true` 现在显式重置该账号的熔断器）。
- **route.status**：终态显示为 `AUTH_FAILED` 且 `unavailable_until_ms` 为空（不显示 RECOVERING），UI 的现有状态徽标即可显示；`TargetStatus` 未改。
- **CLI**：`loadout account check <provider> <alias> [--models]`；文本模式在发现问题时以非零码退出，`--json` 总是输出结果。
- **桌面端**：账号卡片新增"检测"图标按钮（`Stethoscope`），结果以 toast 显示；i18n 新键放在两份字典末尾的 `// Demo: checks` 块中。

## 3. Validation

- `crates/router/src/health.rs`：首次 Auth 可恢复、重复 Auth 终态、终态不被降级 / 不被额度覆盖、成功或 reset 解除、AccountDisabled 立即终态。
- `crates/provider-sdk/src/failure.rs`：403 organization disabled / 401 account_deactivated → `AccountDisabled`；429 不会被判为终态。
- `crates/core/src/check.rs`：Catalog 失败到 status 的映射。
- `crates/gateway/tests/proxy.rs::disabled_or_repeatedly_rejected_accounts_become_terminal`：mock 上游，停用账号与连续 401 进入终态并 failover，终态 target 不再被调用。
- `apps/loadout/tests/checks_e2e.rs`：真实 daemon + mock 上游，覆盖 ok / account_disabled / no_credit / rate_limited / unreachable、四种流式判定、终态不再调用上游、检测成功与重新启用解除终态。
- 性能：`cargo bench -p loadout-benchmarks --bench route_selection`（`is_available` 未改；2026-10-04 本机）：
  - `exact_100_targets` ≈ 567 ns
  - `provider_model_100_targets` ≈ 1.75 µs
  - `virtual_100_targets_with_session` ≈ 15.8 µs

## 4. Limitations

- 只有 Catalog 调用可用时才能区分"key 无效"；没有 Catalog 的 Provider（订阅类）返回 `unsupported`，需要 `--models`。
- 停用关键词是启发式的，覆盖 OpenAI / Anthropic / Gemini 常见文案；Provider 特有的错误码可在各自 `classify` 中细化。
- 检测请求在 IPC 调用内同步执行：带 `--models` 时耗时取决于最慢的模型（单个探测最长 90 秒，4 个并发）。
- `bench.run` 的结果未加入 `stream` 字段（避免修改已有 IPC 类型），流式判定只通过 `account.check` 暴露。
- 终态只存在于内存：daemon 重启后清空，下一次失败重新判断。

## 5. Progress

- [x] FailureKind::AccountDisabled + 分类
- [x] 终态熔断（health.rs）与解除路径（set_key / set_enabled / OAuth / check）
- [x] `account.check` IPC、dispatcher、桌面端白名单、CLI
- [x] bench 流式健康判定
- [x] 桌面端检测按钮 + i18n
- [x] 测试、fmt、clippy、benchmark
