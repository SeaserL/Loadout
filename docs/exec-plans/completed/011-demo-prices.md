# ExecPlan 011: 跨账号比价（Demo）

- **Status**: Completed
- **Created**: 2026-10-04
- **Completed**: 2026-10-04
- **Phase**: Demo 构建；来源 `docs/research/2026-10-04-feature-landscape.md` §3

---

## 1. Goal

1. 修复 `sync_catalog` 中 `COALESCE(input_price, ?)` 导致目录价格永远无法刷新的 bug，并记录价格来源。
2. 新增跨账号比价：同一个模型在各账号的输入 / 输出 / 缓存价格，用用户自己近 30 天的 token 构成算出综合 $/1M。入口：IPC `price.compare`、CLI `loadout price compare [model]`、桌面端"比价 / Prices"页面。

## 2. Decisions

- **Migration 0007 `price_source`**：`account_models` 新增 `price_source TEXT`（`user` / `catalog` / NULL，CHECK 约束）与 `price_updated_at INTEGER`。
  - 回填：已有价格的 `manual` 模型记为 `user`；`catalog` 模型记为 `catalog`（升级前无法区分来源；只有会返回价格的目录，目前只有 OpenRouter，才可能覆盖它们）。
- **同步规则**：目录返回了任一价格时，覆盖所有 `price_source` 不是 `user` 的价格，并记为 `catalog`；目录没有返回价格时保留已存价格（避免把用户在不报价的服务商上手填的价格清空）。
- `model.set_price` 记为 `user`；把四个价格全部清空则 `price_source = NULL`，交还给目录。
- 备份导出 / 导入携带 `price_source`（`#[serde(default)]`，兼容旧备份）。
- **模型分组键**：小写 → 去掉最后一个 `/` 之前的厂商前缀 → 去掉 `:suffix` → 去掉结尾日期（`-YYYYMMDD`、`-YYYY-MM-DD`、`@YYYYMMDD`）→ `.` 统一为 `-`（使 `claude-sonnet-4.5` 与 `claude-sonnet-4-5` 归为一组）。这是 demo 级启发式；models.dev `canonical_model_id` 映射留给后续。
- **Token 构成**：`request_events` 近 30 天 `status = 'ok'` 的请求，按分组键聚合；该模型无历史则用全部模型的构成；完全无历史用默认 75% 输入 / 25% 输出。
- **综合价格**复用 `loadout_router::Prices::cost`（输入 token 含缓存；缺缓存价按输入价计）。输入或输出价格缺失 → `blended_per_million = None`（未知），**永不当 0**，排序时放最后。
- 比价只在控制面（IPC，阻塞线程池）运行，不触碰热路径（Harness §2.2）。没有改动 Router / Gateway，无需 benchmark。
- 为了与并行分支合并：IPC 类型追加在 `api.rs` 末尾；i18n 键集中在两个字典末尾的 `// Demo: prices` 块；前端类型与调用写在 `pages/Prices.tsx` 内，不改 `api.ts`。

## 3. Tests

- `migrations::price_source_backfill_marks_existing_prices`：v6 → v7 回填、CHECK 约束、降级保留价格；`every_migration_round_trips` 覆盖 0007。
- `repos::catalog_prices_refresh_unless_user_set`：目录刷新、用户价格保留、无价格目录不清空、清空用户价格后交还目录、手动模型记为 `user`。
- `storage::prices::token_totals_count_recent_successes_only`。
- `core::prices`：分组键规范化、跨服务商分组与排序、缓存构成、回退构成与过滤。
- `dispatcher::price_compare_lists_priced_models`：IPC 端到端。

## 4. Progress

- [x] Migration 0007 + 同步 bug 修复 + 测试
- [x] `price.compare` IPC + 纯函数比价逻辑 + 测试
- [x] CLI `loadout price compare [model] [--all]`
- [x] 桌面端"比价 / Prices"页面 + bridge 白名单
- [x] `cargo fmt`、`cargo clippy -D warnings`、`cargo test --workspace`、`npm run build` 通过

## 5. Not done / follow-ups

- 打包 models.dev 价格快照、`loadout model link` 手动关联、"本月走别家要花多少"、订阅价值倍数（调研 §3.3）。
- 比价表未显示 TTFT / TPS / 延迟档位。
- 桌面端模型价格编辑仍只支持输入 / 输出价格。
