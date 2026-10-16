# ExecPlan 023: 账号页大卡片排序、合并 ChatGPT 卡片与响应式布局

- **Status**: Completed
- **Completed**: 2026-10-17（v0.3.0）
- **Created**: 2026-10-17
- **Related**: ExecPlan 022（账号小卡片拖动排序）、ADR-012（Codex 订阅转发）、ADR-014（网页会话）、ADR-015（专用端点）

---

## 1. Goal

项目所有者（2026-10-17）：

1. 提供方大卡片也可拖动排序（例如交换 Codex 与 DeepSeek 的位置）。
2. 与 GPT 相关的三张卡片（Codex 用量、Codex 订阅反代、ChatGPT 网页会话）合并为一张：初始只有 Codex 用量，功能开启后卡片变大展示新功能；重复信息只显示一次（额度不显示两遍）。
3. 全屏时一行最多三张卡片、右侧大量留白，需要优化响应式布局。
4. 复查整体 UI/UX 并修复。

## 2. Existing behavior / 调研

用真实 daemon 数据（只读 IPC 导出，本地临时 mock 页面，不入库）在内置浏览器中检查：

- `.page { max-width: 1320px }`：1920 宽窗口右侧约 40% 留白。
- 顶部提供方磁贴占三行，与下面的卡片重复；"ChatGPT 订阅""ChatGPT 网页会话"两个登录磁贴单独存在。
- `codex/main` 与 `codex-sub/main` 显示同一个 ChatGPT 每周额度（67% / 66%），总览页也重复。
- "总 API"按钮出现在只能读用量的提供方（Claude Code、Codex 用量）上。
- 恢复时间超过 2 分钟时显示为"14634 分钟后恢复"。
- 多账号提供方（12 个商汤账号）每张卡片都重复"用量由 Loadout 自己的请求记录估算"；总览页每个账号一行。
- 切换页面时沿用上一页的滚动位置。

## 3. Decisions

- **提供方顺序只影响显示**：migration 0014 `providers.position`（按 slug 字母序回填，新提供方排最后），`provider.list` 按 position 排序，IPC `provider.reorder {slugs}`。路由顺序仍由延迟档位与账号顺序决定（ADR-003），界面提示里说明。
- **拖动**：抽出通用 `usePointerReorder`（指针事件，见 ExecPlan 022），大卡片与小卡片各自的作用域属性不同（`data-board-item` / `data-alias`），互不干扰。
- **合并 ChatGPT 卡片**（纯前端）：kind 为 `codex`、`codex-subscription`、`web-chatgpt` 的提供方在看板上是一个条目（拖动时三个 slug 一起移动）。按账号别名分组，每个已开启的功能是一块（带自己的提供方控件）；额度取第一个有额度窗口的来源显示一次，其余块只显示经它的请求数。未开启的功能显示为"显示 Codex 用量 / 开启 Codex 反代 / 添加网页会话"按钮，替代原来的两个登录磁贴。
- **布局**：页面最大宽度 2400px（设置页保持 1120px）；看板 4 列容器查询；提供方磁贴改为一行筛选标签（搜索框 + 每张卡片一个标签，含账号数）。
- **其他修复**：只对 API key 类提供方显示"总 API"；超过 2 分钟的恢复时间按天 / 小时显示；多账号且全部为估算用量时，说明只在提供方标题下显示一次；账号名保留最小宽度、按钮可换行；切换页面回到顶部；总览页把只有估算用量的同一提供方账号合并为一行、ChatGPT 额度只显示一次、"接入地址"列出已开启的专用端点。

## 4. Tests / Verification

- core：`providers_keep_the_display_order_given`（新提供方排最后、部分列表重排、重复与未知 slug 报错）；migration 0014 往返（`every_migration_round_trips`）。
- 内置浏览器 + 真实数据 mock：大卡片拖动（ChatGPT 卡片移到第一位，`provider.reorder` 的 slug 顺序正确展开为 codex、codex-sub、web-chatgpt）；大卡片内的小卡片拖动只调用 `account.reorder`；1920 宽时看板 4 列（1601px）；总览页分组与去重；切页回到顶部。

## 5. Progress

- [x] 2026-10-17 后端：migration 0014、`provider.reorder`、测试
- [x] 2026-10-17 前端：通用拖动 hook、大卡片拖动、合并 ChatGPT 卡片、筛选标签、布局与 UI 修复、总览页
- [x] 2026-10-17 内置浏览器验证（mock 已删除）
- [x] 2026-10-17 fmt / clippy / cargo test --workspace（394 通过）/ npm run build / release 构建
- [x] 2026-10-17 项目所有者实机测试（v0.3.0）
