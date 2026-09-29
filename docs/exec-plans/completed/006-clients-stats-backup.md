# ExecPlan 006: Client Policies, Log Filters, Statistics, Backup / Export

- **Status**: Completed
- **Created**: 2026-09-29
- **Phase**: Harness §26 Phase 9（Client keys、Advanced logs、Statistics、Backup/export）

---

## 1. Goal

1. **Client policies**（ADR-011 §2、产品文档 §11 / §38）：每个 client 可设置
   - `default_model`：请求的 model 不是 Loadout 名称（如工具写死的 `gpt-4o`）或为空时使用；
   - `allowed_models`：允许的模型名模式（精确名或以 `*` 结尾的前缀，如 `loadout/*`、`openrouter/*`），为空 = 不限；
   - `daily_token_limit`、`daily_cost_limit_usd`：UTC 自然日上限；
   - `requests_per_minute`：速率上限。
2. **日志筛选**：按 client、模型 / 路由关键字、状态、时间筛选；日志显示 client 名称而非 id。
3. **统计**：按 model / route / provider / client / day 聚合请求数、错误数、token、费用、平均 TTFT、平均 TPS。
4. **备份 / 导出**：配置导出为 JSON（不含任何密钥）、导入合并；数据库完整备份（`VACUUM INTO`）。

## 2. Constraints

- 热路径：策略与计数只在内存（ArcSwap 快照 + 每 client 的小计数器），不查数据库（ADR-011 §2）。
- 计数在请求结束后由 telemetry sink 更新（与 LatencyTap 同样的位置）；启动时从 request_events 恢复当天用量。
- 导出文件不含 API key / token；client key 只导出 SHA-256 hash（不可逆，允许迁移后原 key 继续可用）。导入后需要重新输入各账号 API key。
- Payload logging 在 v1 不存在（ADR-008 默认关闭），因此不提供 logging 策略开关。

## 3. Design

### 3.1 Policies

- Storage（migration 0005）：`clients.policy_json TEXT NOT NULL DEFAULT '{}'`。
- Gateway：`ClientPolicies`（ArcSwap<HashMap<client_id, Arc<Policy>>> + Mutex<HashMap<client_id, Counters>>）。
  - `resolve_model(client, requested)`：default_model 替换规则。
  - `check_model(client, name)`：不匹配 → 403 `model_not_allowed`。
  - `admit(client, now)`：RPM（固定 60 s 窗口）与当日 token / 费用 → 429 `client_limit`。
  - `record(client, tokens, cost, ts)`：由 core 的 telemetry tap 调用。
- `/v1/models` 按 allowed_models 过滤（ADR-010 §4）。
- IPC `client.set_policy {name, policy}`；`ClientInfo.policy`。CLI `loadout client policy <name> …`；UI Client keys 页面编辑。

### 3.2 Logs

- `requests.recent` 增加可选 `client`（名称）、`search`（requested_model / route 子串）、`status`（`ok` / `error`）、`since_ms`。返回的 `client` 为名称。

### 3.3 Statistics

- IPC `stats.summary {since_ms, group_by}` → 行：`key, requests, errors, input_tokens, output_tokens, cost_usd, avg_ttft_ms, avg_tps`。按 requests 降序（day 按日期）。
- CLI `loadout stats [--by model|route|provider|client|day] [--days N]`；UI Statistics 页面。

### 3.4 Backup

- IPC `backup.export {path?}`：写入给定路径或 `<data>\backups\loadout-config-<UTC 时间>.json`，返回路径与计数。
- IPC `backup.import {path}`：合并——不存在的 provider / account / model / pool / client 创建；已存在的跳过；settings 覆盖。返回每类的 created / skipped 与需要补 key 的账号列表。
- IPC `backup.database {path?}`：`VACUUM INTO` 到 `<data>\backups\loadout-<UTC 时间>.db`。
- CLI `loadout backup export|import|database`；UI Settings 页面按钮。

## 4. Stages

1. Policies：storage、gateway、core tap / 恢复、IPC / CLI、测试。
2. Logs 筛选 + Statistics：storage 查询、IPC、CLI、UI 页面。
3. Backup：导出 / 导入 / 数据库备份、IPC、CLI、UI、e2e（导出 → 新 daemon 导入 → 配置一致、无密钥）。

## 5. Tests

- 策略：默认模型替换、模式匹配、RPM、当日上限（含跨日重置）、`/v1/models` 过滤；gateway 集成测试返回 403 / 429。
- 统计 / 日志：storage 聚合与筛选单元测试。
- Backup：round-trip e2e，导出文件中不出现 API key。

## 6. Security

- 导出文件不含密钥；数据库备份同样不含密钥（ADR-004）。导出 / 备份路径由用户给定或位于数据目录。
- 策略错误信息不泄露其他 client 的信息。

## 7. Definition of Done

- [x] §1 全部功能，测试覆盖 §5
- [x] clippy / fmt / 测试 / 前端构建通过
- [x] 本文件移入 `completed/`

## 8. Progress

- [x] Stage 1 — policies（2026-09-29）
  - Migration 0005；gateway `ClientPolicies`（ArcSwap 策略快照 + 每 client 计数器，UTC 日 / 60 s 固定窗口）；错误 `model_not_allowed`（403）、`client_limit`（429）；`/v1/models` 按策略过滤。
  - 计数在 telemetry tap 中更新（请求结束后），启动时从 request_events 恢复当天用量。
  - IPC `client.set_policy`，`ClientInfo` 带策略与当日用量；CLI `loadout client policy`；UI 策略对话框。
  - gateway 集成测试：默认模型替换、禁止的模型、列表过滤、RPM（对分钟边界稳健）。
- [x] Stage 2 — logs / stats（2026-09-29）
  - `requests.recent` 支持 client / search / status / since（LIKE 通配符转义），日志显示 client 名称；`stats.summary` 按 model / route / provider / client / day(UTC) 聚合。
  - CLI `loadout logs --client --search --errors --hours`、`loadout stats --by --days`；UI Statistics 页面、Request log 筛选。
- [x] Stage 3 — backup（2026-09-29）
  - `backup.export`（JSON，无 API key，client key 仅 hash）、`backup.import`（合并、跳过已存在、列出需补 key 的账号）、`backup.database`（`VACUUM INTO`）。CLI 将相对路径转为绝对路径（daemon 工作目录不同）。
  - `backup_e2e`：A 导出 → B 导入，价格 / tier / 禁用状态 / pool / 设置 / client 策略一致，原 client key 在 B 上可用，导出文件与 B 的凭据存储中都没有 API key，重复导入全部跳过。
