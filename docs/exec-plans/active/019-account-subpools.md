# ExecPlan 019: 子号池（Account Sub-pools）

- **Status**: Active
- **Created**: 2026-10-17
- **Related**: ADR-003（快照路由）、ADR-010（模型寻址）、ExecPlan 004（模型 Pool，与本计划无关）

---

## 1. Goal

项目所有者（2026-10-17）：账号页增加"子号池"。现有全部账号构成**主号池**；用户可以创建若干由部分账号组成的**子号池**，并像切换账户一样切换当前环境：

- 切到子号池后，路由只在子号池的账号中选择；
- 程序启动时默认回到主号池；
- 设置项"保留当前切换状态"开启后，重启沿用上次切换到的子号池。

## 2. Context

- "Pool" 在 Loadout 中已经指模型 Pool（`loadout/<name>`，ExecPlan 004）。为避免混淆，代码中称 **Account Set**（表 `account_sets`），界面称"子号池 / Sub-pool"，CLI 为 `loadout subpool`。
- 路由表由 `core/src/route_build.rs` 从数据库构建后整体发布（ADR-003）；禁用账号就是在构建时跳过。

## 3. Existing behavior

所有启用账号的模型都进入路由表；`loadout/default|fast|realtime`、模型 Pool、`provider/model` 与精确名都在全部账号中解析。

## 4. Constraints

- 热路径不变：子号池只在构建路由表时生效，请求路径仍只读 `RouteSnapshot`（Harness §2.2）。
- 数据库变更走 migration 0011，含往返测试（Harness §21）。

## 5. Architecture impact / Decisions

- **语义 = 账户切换**：子号池生效时，不在子号池中的账号在路由表中不存在（与禁用等效，但不改变账号自身的启用状态）。因此虚拟模型、模型 Pool、`provider/model`、同模型换账号 failover，以及精确名 `provider/account/model` 都只在子号池内解析；对池外账号的精确名请求返回"model not found"。用量轮询、账号管理不受影响。
- 主号池不落库：`active` 为空即主号池。
- 同一时刻最多一个子号池 `active = 1`（部分唯一索引）。
- 启动：设置 `account_sets.remember_active`（默认 `false`）为 `false` 时，daemon 启动先清除 `active`，即回到主号池。
- 子号池名：1–40 个字符，不能为空白，不能叫 `main` / `主号池`。
- 新增账号不会自动加入任何子号池。

## 6. Files expected to change

- `crates/storage/migrations/0011_account_sets/{up,down}.sql`、`crates/storage/src/{account_sets.rs,lib.rs,migrations.rs,database.rs}`
- `crates/core/src/{route_build.rs,control.rs,dispatcher.rs,settings.rs,daemon.rs}`
- `crates/ipc/src/api.rs`、`apps/desktop/src-tauri/src/bridge.rs`
- `apps/loadout/src/bin/loadout/{main.rs,subpool_cmd.rs}`
- `apps/desktop/src/{api.ts,i18n.tsx,pages/Accounts.tsx}`

## 7. Implementation stages

1. Migration + storage 仓库 + 测试
2. `route_build` 过滤 + 启动清除 + 设置项
3. IPC `account_set.list|save|remove|activate` + CLI
4. 桌面端：账号页顶部切换器 + 子号池管理对话框 + 池外账号标记

## 8. Tests

- storage：增删改、成员级联删除、唯一 active、migration 往返。
- core：子号池生效时路由表只含其账号；切回主号池恢复；`remember_active=false` 时启动清除。
- dispatcher 端到端：save → activate → `route.virtuals` 只含池内账号。

## 9. Performance validation

请求路径无变化（只少了一部分 target）。`cargo bench -p loadout-benchmarks` 用于确认无回归。

## 10. Security / Migration / Risks

- 无 secret 相关变更。Migration 0011 只新增表与索引，down 删除。
- 风险：用户切到子号池后忘记切回，池外账号"消失"。缓解：账号页顶部的号池横条在子号池生效时变色并显示账号数，池外账号卡片变淡并标记"不在当前子号池"。

## 11. Definition of done

CLI 与桌面端都能创建、编辑、删除、切换子号池；路由按子号池生效；设置项可保留切换状态；fmt / clippy / test / benchmark / `npm run build` 通过。

## 12. Progress

- [x] 2026-10-17 Stage 1 storage
- [x] 2026-10-17 Stage 2 路由与设置
- [x] 2026-10-17 Stage 3 IPC / CLI
- [x] 2026-10-17 Stage 4 桌面端
- [x] 2026-10-17 验证：fmt / clippy / cargo test --workspace（383 通过）/ npm run build / 许可证检查通过；benchmark 只有噪声级波动（Router 未改，子号池只在构建路由表时过滤）
- [ ] 项目所有者实机测试
