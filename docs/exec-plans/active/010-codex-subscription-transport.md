# ExecPlan 010: Codex Subscription Transport (opt-in)

- **Status**: Active
- **Created**: 2026-10-04
- **Decision**: [ADR-012](../../adr/ADR-012-subscription-transport-opt-in.md) §1

---

## 1. Goal

用户用 Loadout 自己的 OAuth 登录一个 ChatGPT 账号（与官方 Codex CLI 互不干扰）。经两步风险确认后，该账号的 Codex 模型可以通过 Loadout 的 Responses / Chat / Messages 入口使用。

## 2. Decisions

### 2.1 新 Provider 与 SDK 扩展

新 Provider `codex-subscription`，固定 slug `codex-sub`：

- `AuthMethod::OAuth`（新增）；
- Transport 为 Responses，`TrustLevel::PrivateEndpoint`；
- drivers：auth、catalog、transport、health。

SDK 扩展（ADR-005，均为带默认值的 trait 方法，现有 Provider 不受影响）：

- `oauth() -> Option<&dyn OAuthFlow>`：
  - `start_device()` → `{verification_url, user_code, poll: DevicePoll}`；
  - `poll_device(&DevicePoll)` → `Pending | Done(TokenSet) | Failed`；
  - `refresh(&refresh_token)` → `TokenSet`。
  - `TokenSet { access: Secret, refresh: Option<Secret>, expires_at_ms, account_label }`，不派生 Debug 原文。
- `prepare_request(config, protocol, &mut Value)`：上游所需的请求体规范化。转换路径与透传路径都在发送前调用。
- `upstream_stream_only() -> bool`：上游只接受流式。客户端要非流式时，透传正文也强制 `stream=true`，由聚合渲染器输出完整响应。
- `account_concurrency() -> Option<u32>`：每账号并发上限；默认 `None`，codex-sub 为 2。

### 2.2 凭证与刷新

- 一个 OAuth 账号有两条 `credentials` 记录（`credential_type = 'oauth'`），`scope` 分别为 `access` / `refresh`。
- secret 本体在 Credential Manager 中（access token 约 1.8 KB，未超过 2560 B 上限）。
- access 记录的 `expires_at` 取 JWT `exp`。
- **刷新**：daemon 后台任务每 5 分钟检查一次，在到期前 24 小时内刷新。
  - 同一账号的刷新串行执行（每账号一把 mutex）。
  - 刷新成功：先写入新 secret、再更新 SQLite 引用、最后删除旧 secret；然后重建路由。热路径只读快照（规则 1）。
  - `invalid_grant`、`refresh_token_reused` 或 401：标记"需要重新登录"（账号 health 为 Auth），不重试。
- `account_id`（`Chatgpt-Account-Id` 头）与 plan 在构建 endpoint 时从 access token 的 claims 解析，不另存。

### 2.3 Opt-in、确认与路由

- Migration 0006：`accounts.transport_opt_in_at INTEGER NULL`。
- 启用：`account.set_transport {provider, alias, enabled, confirm}`，`confirm` 必须等于账号别名（第二步确认）。第一步"我了解风险"在 UI / CLI 侧完成。
  - 启用和关闭都写一条 `tracing` 审计日志（只含 provider/alias）。
- **路由构建**：低信任（非 Tier A）Transport 只在账号已 opt-in 时生成 RouteTarget，并标记 `RouteTarget.opted_in = true`。
- **选路**：`allowed_for_inference() || opted_in`。
- **虚拟模型**：opt-in 的低信任目标不进入 `loadout/realtime|fast|default`；用户 Pool 可以显式包含。
- **并发**：gateway 维护每账号的 semaphore（只针对声明了上限的模块）。拿不到许可时跳过该目标（原因为 "account concurrency limit"），可以 failover 到别的账号。

### 2.4 协议细节

依据 CLIProxyAPI `d7914af` 的实现知识，以官方 Codex 开源实现为准。

- **上游**：`POST https://chatgpt.com/backend-api/codex/responses`。
- **请求头**：
  - `Authorization: Bearer`、`Chatgpt-Account-Id`、`Accept: text/event-stream`；
  - `User-Agent: loadout/<版本>`（如实标识）；
  - `Originator` 只有在 provider 设置 `client_identity = "codex"` 时才发送 `codex_cli_rs`；默认不发。
- **请求体**：
  - 强制 `stream=true`、`store=false`；
  - 删除 `previous_response_id`、`max_output_tokens`、`max_tokens`、`temperature`、`top_p`、`user`、`truncation`、`prompt_cache_retention`、`safety_identifier`、`stream_options`、`service_tier`；
  - 缺少 `instructions` 时补 `""`；字符串形式的 `input` 转为消息数组；`system` 角色改为 `developer`。
- **错误分类**：
  - 429 + `usage_limit_reached` → QuotaExhausted（`resets_at` / `resets_in_seconds`）；
  - 其他 429 → RateLimited；
  - 401 或 `refresh_token_reused` → Auth；
  - "at capacity" / overloaded → Server；
  - `context_length_exceeded` → InvalidRequest。
- **模型目录**：`GET https://chatgpt.com/backend-api/codex/models?client_version=<版本>`，只取 `visibility == "list"` 且 `supported_in_api` 的模型，带上 context_window。
- 不声明 `RESPONSES_STATEFUL`；上游 `store=false`。

### 2.5 登录 IPC

- `account.oauth_start {provider, alias}` → `{login_id, verification_url, user_code, expires_in}`。
  - daemon 后台按间隔轮询，最长 15 分钟；
  - 账号在登录成功后才创建（`account_type = 'subscription'`）。
- `account.oauth_status {login_id}` → `pending | done(AccountInfo) | failed(message)`。
- 重新登录同一 alias 时替换凭证（删除旧 secret）。

### 2.6 范围外

- Claude 订阅 Transport（ADR-012 §2，方案 A）。
- TLS 指纹伪装、WebSocket、图像接口。
- 用量窗口：后续从响应头读取，本计划不做。现有的 `codex` provider 继续通过 app-server 提供用量。

## 3. Stages

1. SDK 扩展 + `codex-subscription` 模块（endpoint、prepare_request、classify、JWT claims、目录解析、OAuth 请求与响应解析）+ 单元测试（mock upstream）。
2. 存储 migration 0006 + routing 查询（oauth access secret、opt_in）+ 测试。
3. Core：OAuth 登录会话、刷新任务、`account.set_transport`、route_build 与 virtual 排除、IPC + dispatcher + bridge 白名单 + 测试（mock OAuth server，用 `--home-dir` / base URL 覆盖）。
4. Router / Gateway：opted_in 选路、`prepare_request`、强制上游流式、账号并发限制 + 测试与 benchmark（Harness §20）。
5. CLI：`loadout account login codex-sub <alias>`、`loadout account transport <provider/alias> on|off`（交互式两步确认，`--yes-i-understand` 加别名参数用于脚本）。
6. 桌面端：登录对话框（显示验证地址和代码）、启用开关加两步确认对话框。在 ExecPlan 009 合并后进行。
7. 文档：`docs/providers/codex-subscription.md`、README、system-test-plan；live test 说明（`LOADOUT_LIVE_TESTS=1`，由项目所有者用自己的账号执行）。

## 4. Tests

- 默认测试全部使用 mock OAuth / mock upstream，不访问真实服务。
- 未 opt-in 的 codex-sub 账号：不生成路由目标，`model list` 中标明"未启用"。
- opt-in 后：精确名称可以选到；`loadout/default` 不包含它；Pool 显式加入后可以选到。
- 刷新：
  - 到期前触发；新旧 secret 正确替换；
  - `invalid_grant` 时标记 Auth；
  - 并发刷新只发一次请求。
- prepare_request：字段删除和改写；非流式客户端 + stream-only 上游能得到完整 JSON 响应。
- 并发上限：第 3 个并发请求跳过该账号。
- 错误分类：quota 及 reset 时间、auth、capacity。

## 5. Definition of Done

- [ ] §2 全部完成，测试覆盖 §4
- [ ] clippy / fmt / 测试 / benchmark 无回退
- [ ] 文档；本文件移入 `completed/`
- [ ] 项目所有者用真实账号完成 live test（登录 → 启用 → 一次对话），结果记入 Progress

## 6. Progress

- [x] Stage 1 — SDK + provider module（2026-10-04）
  - SDK：`AuthMethod::OAuth`；`oauth::{OAuthFlow, TokenSet, DevicePoll, RefreshError, form_encode, jwt_claims}`；`ProviderModule::{prepare_request, upstream_stream_only, account_concurrency, oauth}`。
  - `codex_subscription.rs` 已实现 endpoint、请求体规范化、错误分类、模型目录、设备码登录与刷新，并用 mock 服务器测试。
  - 尚未在真实账号上验证的假设：`client_version=0.154.0`、如实的 `User-Agent` 能否被接受（见 §4 live test）。
  - 未引入新依赖：base64url 解码和 form 编码都是手写的小函数。
- [x] Stage 2 — storage（2026-10-04）
  - Migration 0006 `accounts.transport_opt_in_at`。
  - `set_oauth_tokens` 在一个事务内替换 access / refresh 两条记录，返回不再引用的 secret。
  - `oauth_expiring`；routing 查询返回 OAuth access 的 secret_ref 和 `transport_opt_in`。
  - 教训：用 Python heredoc 替换带 `\` 续行的 Rust 字符串时没有匹配上，`COLUMNS` 没改到，测试报 InvalidColumnIndex。这类编辑改用 Edit 工具（memory 已有记录）。
- [x] Stage 3 — core（2026-10-04）
  - `core::oauth`：登录会话保存在内存中，后台轮询；`finish_login` 写入 token、同步模型、重建路由。
  - 刷新任务每 5 分钟运行，提前 24 小时刷新，全局串行；失效的登录记入 dead 集合，不再重试，并把相关目标标记为 Auth 失败。
  - `set_transport` 校验别名确认、记 warn 级审计日志、重建路由。
  - OAuth provider 不能用 `account add` 添加；`codex-sub` provider 在首次登录时自动创建。
  - `key_ref()` 确保 sync 和 usage 读取的是 access token 而不是 refresh token。
  - IPC：`account.oauth_start`、`account.oauth_status`、`account.set_transport`（已加入 bridge 白名单）；`AccountInfo.transport_opt_in`、`login_expires_at_ms`。
- [x] Stage 4 — router / gateway（2026-10-04）
  - `RouteTarget.opted_in`；选路允许 Tier A 或已 opt-in 的目标；opt-in 的目标不进入内置虚拟模型（Pool 可以显式包含）。
  - Gateway：账号并发许可（`AccountSlots`，流式响应期间许可由 Pump 持有）；`prepare_request` 在透传和转换两条路径都会调用；stream-only 上游对非流式客户端走聚合。
  - 测试覆盖：非流式客户端加 stream-only 上游、请求体改写、未 opt-in 的目标被拒、AccountSlots。
  - Benchmark（Harness §20）：本机当前负载比 9/29 基线高约 1.5 倍，所以对改动前的提交 e680d7c 与当前代码做了**背靠背** A/B 对比。

    | 项目 | 改动前 | 改动后 |
    |---|---|---|
    | `route_selection/exact` | 353 ns | 346 ns |
    | `route_selection/provider_model` | 832 ns | 805 ns |
    | `route_selection/virtual+session` | 11.8 µs | 11.7 µs |
    | `stream_proxy/passthrough_200` | 316 µs | 256 µs |
    | `stream_proxy/convert` | 1.48 ms | 590 µs |
    | `ttft/via_gateway_passthrough` | 980 µs | 712 µs |
    | `ttft/via_gateway_converted` | 787 µs | 607 µs |

    改动后都不慢于改动前，没有回退。差异属于噪声：`ttft/direct_upstream` 不经过网关，两次也差 163 → 105 µs。
- [x] Stage 5 — CLI（2026-10-04）
  - `loadout account login <provider> <alias>`：显示验证地址和代码，轮询状态。
  - `loadout account transport <provider> <alias> on|off`：先显示风险说明，再依次确认 "I understand" 和别名；脚本可用 `--i-understand-the-risk --confirm <alias>`。
  - `account list` 显示 "(transport on)"。
- [ ] Stage 6 — desktop（等 ExecPlan 009 合并）
- [ ] Stage 7 — docs / live test
  - [x] 已完成文档：docs/providers/codex-subscription.md、providers 索引、README、system-test-plan、THIRD_PARTY_NOTICES。
  - [ ] 项目所有者的 live test。
