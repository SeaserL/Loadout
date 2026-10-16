# ExecPlan 022: 自动停用套餐不含的模型；拖动调整账号顺序

- **Status**: Active
- **Created**: 2026-10-17
- **Related**: ADR-003（顺序即 failover 顺序）、ADR-015 / ExecPlan 021（商汤总 API）

---

## 1. Goal

项目所有者（2026-10-17）：

1. 商汤部分账号的 Token Plan 不含某些模型（如 `deepseek-v4.1-flash`），上游返回 403 "model is not available in the current token plan"。希望自动处理：该账号上的这个模型自动停用并在界面说明，不再被路由。
2. 账号页可以拖动卡片调整账号顺序（原来顺序固定）。

## 2. Context / Existing behavior

- 商汤 `/v1/models` 对所有账号返回同一份平台目录，无法按账号区分权限；只有调用时才知道。
- ExecPlan 021 已把这类 403 识别为模型级错误（冷却 5 分钟）而不是凭证错误，但每 5 分钟仍会被再试一次。
- 账号有 `position` 字段，路由表按 provider → position → alias 排序（即 failover 顺序），但没有修改入口。

## 3. Decisions

- 新增 `FailureKind::ModelUnavailable`（"套餐 / 权限不含此模型，key 正常"）：健康状态同 `ModelNotFound`（冷却），错误分类 `model_not_found`，可 failover。只有 Provider 模块明确识别的文案才会产生它（目前只有商汤预设的 `model_not_in_plan`），通用 404 不会触发自动停用，避免错误 Base URL 把所有模型停掉。
- 上报路径：网关在请求路径上只调用 `TelemetrySink::model_unavailable(target_id)`（`try_send`，不阻塞）；控制面任务收到后把该账号模型 `enabled = 0`、`disabled_reason = 'not_in_plan'`，并重建路由表。测速、自动测速和恢复探测（`probe_all`、恢复监视）同样处理。
- Migration 0013：`account_models.disabled_reason`。用户手动启用 / 停用会清除原因；目录同步保留停用状态（原有行为）。
- 界面：账号卡片状态行显示"已自动停用 N 个套餐不含的模型：…"；模型页对这些模型显示"套餐不含"标记。
- 排序：IPC `account.reorder {provider, aliases}` 一次写入全部位置（必须列出该 Provider 的全部账号，不重复）；桌面端在同一 Provider 区块内拖动卡片，放下即保存，提示"这个顺序也是故障切换顺序"。

## 4. Files

`crates/provider-sdk/src/failure.rs`、`crates/router/src/health.rs`、`crates/providers/src/presets.rs`、`crates/gateway/src/{plane.rs,proxy/mod.rs}`、`crates/storage/{migrations/0013_model_unavailable,src/account_models.rs,src/accounts.rs}`、`crates/core/src/{unavailable.rs,sinks.rs,daemon.rs,bench.rs,latency.rs,endpoints.rs,control.rs,dispatcher.rs,check.rs}`、`crates/ipc/src/api.rs`、`apps/desktop/src/{api.ts,i18n.tsx,styles.css,pages/Accounts.tsx,pages/Models.tsx}`、bridge 白名单。

## 5. Tests

- presets：商汤 "not available in the current token plan" → `ModelUnavailable`，401 仍为 `Auth`。
- gateway：上游 403 套餐不含 → 上报 `model_unavailable`、切到下一个账号、状态为冷却而非 AUTH_FAILED。
- core：标记后模型停用并带原因、路由表移除、重复上报无变化、手动启用清除原因；`account.reorder` 改变 failover 顺序并拒绝缺漏 / 重复 / 未知别名。
- storage：migration 0013 往返（`every_migration_round_trips`）。

## 6. Performance / Security / Risks

- 请求路径只多一次非阻塞 `try_send`（仅在该错误发生时）。
- 无 secret 变更。
- 风险：上游误报会停用可用模型；用户可在模型页重新启用，原因随之清除。

## 7. Progress

- [x] 2026-10-17 后端（错误类型、上报、停用、迁移、排序 IPC）+ 测试
- [x] 2026-10-17 桌面端（状态行、模型页标记、拖动排序）
- [x] 2026-10-17 验证：fmt / clippy / cargo test --workspace（393 通过）/ npm run build；benchmark 与上次持平（route_selection 0.23–0.53 µs，snapshot build 42 µs，经网关 TTFT 585 µs）
- [ ] 项目所有者实机测试
