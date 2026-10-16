# ExecPlan 021: 专用端点（商汤总 API）与账号健康状态

- **Status**: Completed
- **Completed**: 2026-10-17（v0.3.0）
- **Created**: 2026-10-17
- **Decision**: [ADR-015](../../adr/ADR-015-dedicated-endpoints.md)
- **Related**: ADR-003（快照路由与 failover）、ExecPlan 019（子号池）

---

## 1. Goal

项目所有者（2026-10-17）：

1. 确认商汤能否读到用量信息；
2. 账号页显示账号进入限流 / 出错的状态，并监视何时恢复；
3. 商汤专用的本地"总 API"：不切子号池；时刻在线；单号出问题立即换下一个；开启时自动包含当前号池的全部商汤账号。

## 2. Context / 调研结论

- **商汤用量不可读**：API 主机上没有任何用量 / 额度路径（用无效 key 探测，所有候选路径在鉴权前即 404，只有 `/v1/models` 等真实路由返回 401）。控制台的额度页（双积分池的 5 小时 / 每周额度、各模型 TPM/RPM）通过 SenseCore IAM 登录态读取，不接受 API key。Loadout 只能按自己的请求记录估算。
- 控制台文案确认："各模型独立 TPM / RPM 配额，超限返回 429"；实测恢复约 20 秒（`docs/providers/sensenova.md`）。
- 健康状态（`HealthRegistry`）按 Target（账号模型 × 协议）记录：`RATE_LIMITED`（30 秒起指数退避）、`COOLDOWN`、`QUOTA_EXHAUSTED`、`AUTH_FAILED`；到期后半开，由下一次请求决定。`route.status` 已对外提供每个 Target 的状态。

## 3. Existing behavior

- 只有统一入口 `/v1/*`；`sensenova/<model>` 可跨账号 failover，但受 `routing.max_attempts`（按 Target 计）限制，且全部冷却时立即报错。
- 账号卡片不显示健康状态；状态只在 Routes 页按 Target 显示。

## 4. Constraints

- 热路径只读 RouteSnapshot（Harness §2.2）：端点索引随路由表编译；等待恢复只是计时，不发网络请求。
- Router / Gateway 改动跑 benchmark（Harness §20）；schema 变更走 migration 0012 并测试。
- 不换模型（ADR-003）：端点只在成员账号的同一模型间切换。

## 5. Decisions

- 端点路径 `/e/<name>/v1/…`，共用端口与 client key。默认名 = provider slug。
- 成员 `null` = 该 Provider 在当前号池中的全部账号（满足"开启时自动框选"，之后新增的商汤账号也自动加入）；勾选部分账号则为显式列表。
- 每个账号只尝试一次（优先入口协议）；尝试次数 = 成员数。
- `wait_secs` 默认 30：全部冷却且最早恢复在此时限内则等待后重选；全部尝试都因限流失败也同样等待一次。
- 商汤 429 "tpm/rpm limit" 的 `retry_after` 设为 20 秒（实测），而不是通用的 30 秒起步。
- 恢复监视：仅端点成员；冷却到期后探测一次（`bench::probe`），记录 `recovered_at_ms`。
- 账号卡片：聚合该账号所有 Target 的状态，显示最严重的一个（鉴权失败 > 额度耗尽 > 限流 > 冷却），带模型名、错误、预计恢复时间；恢复后 10 分钟内显示"已于 HH:MM 恢复"。

## 6. Files expected to change

- `crates/router/src/{health.rs,table.rs,select.rs,lib.rs}`
- `crates/storage/migrations/0012_endpoints/*`、`crates/storage/src/{endpoints.rs,lib.rs,migrations.rs,database.rs}`
- `crates/core/src/{route_build.rs,endpoints.rs,dispatcher.rs,daemon.rs,control.rs,lib.rs}`
- `crates/gateway/src/{server.rs,routes.rs,proxy/mod.rs}`
- `crates/providers/src/presets.rs`（商汤 429 retry_after）
- `crates/ipc/src/api.rs`、`apps/desktop/src-tauri/src/bridge.rs`
- `apps/loadout/src/bin/loadout/{main.rs,endpoint_cmd.rs}`
- `apps/desktop/src/{i18n.tsx,styles.css,pages/Accounts.tsx,pages/Endpoints.tsx}`

## 7. Implementation stages

1. Router：恢复时间、端点索引与解析、按账号去重的选择
2. Storage + route_build + IPC + CLI
3. Gateway：`/e/<name>/…` 路由、等待恢复
4. 恢复监视任务；商汤 retry_after
5. 桌面端：账号健康状态、商汤总 API 对话框
6. 测试、benchmark、文档

## 8. Tests

- router：端点解析（裸名 / 前缀名 / 默认模型 / 未知）、只含成员、每账号一个 Target、恢复时间记录。
- storage：migration 往返、增删改。
- gateway：`/e/<name>/v1/chat/completions` 端到端（mock 上游：第一个账号 429 → 第二个账号成功；全部冷却时等待后成功；未知端点 404）。
- core：成员 `null` 跟随当前号池；子号池外账号不进入端点。

## 9. Performance validation

`cargo bench -p loadout-benchmarks`，重点 `route_selection/*`、`snapshot_swap/*`。

## 10. Security / Migration / Risks

- 不新增鉴权面：同一中间件与 client key。
- Migration 0012 只新增表。
- 风险：等待恢复会让请求最长多挂起 `wait_secs` 秒；客户端超时更短时会先断开。可调为 0 关闭。

## 11. Definition of done

商汤总 API 可在桌面端开启并复制地址；单号限流时请求自动落到下一个号；账号卡片显示状态与恢复；fmt / clippy / test / benchmark / `npm run build` 通过。

## 12. Progress

- [x] 2026-10-17 调研：商汤用量不可读（见 §2）
- [x] 2026-10-17 Stage 1 Router
- [x] 2026-10-17 Stage 2 Storage / IPC / CLI
- [x] 2026-10-17 Stage 3 Gateway
- [x] 2026-10-17 Stage 4 恢复监视
- [x] 2026-10-17 Stage 5 桌面端
- [x] 2026-10-17 Stage 6 验证：fmt / clippy / cargo test --workspace（389 通过）/ npm run build；benchmark：route_selection 0.25–0.52 µs、snapshot build 44 µs（+8%，多了端点索引），其余为噪声级波动
- [x] 2026-10-17 项目所有者实机测试（v0.3.0）
