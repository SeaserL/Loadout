# Codex / ChatGPT subscription transport (`codex-subscription`, slug `codex-sub`)

- **Last Verified Date**: 2026-10-04（基于源码与文档；真实账号 live test 待项目所有者执行）
- **Module**: `crates/providers/src/codex_subscription.rs`
- **Decision**: [ADR-012](../adr/ADR-012-subscription-transport-opt-in.md)，[ExecPlan 010](../exec-plans/active/010-codex-subscription-transport.md)

## Subscription Transport Status

**OPT-IN, `PRIVATE_ENDPOINT`**（Tier C）。默认不路由，账号所有者需要两步确认后才生效。

与只读用量的 [`codex`](codex.md) provider 相互独立：

- `codex` 读取官方 CLI 的登录，只用于展示用量；
- `codex-sub` 使用 Loadout 自己的登录，可以转发请求。

## 使用

```text
loadout account login codex-sub main                  设备码登录（浏览器打开地址并输入代码）
loadout account transport codex-sub main on           风险说明 → 输入 "I understand" → 输入别名
loadout model list                                    codex-sub/main/<model>
loadout account transport codex-sub main off          停止路由
```

客户端用精确名称 `codex-sub/main/<model>`。它不会出现在 `loadout/realtime|fast|default` 中；如需参与，可以手动加入 Pool。

## Authentication（Loadout 自有登录）

设备码流程，使用 Codex CLI 的公开 OAuth client `app_EMoamEEZ73f0CkXaXp7hrann`：

1. `POST https://auth.openai.com/api/accounts/deviceauth/usercode` `{client_id}` → `{device_auth_id, user_code, interval}`。用户打开 `https://auth.openai.com/codex/device` 并输入代码。
2. 轮询 `POST …/api/accounts/deviceauth/token` `{device_auth_id, user_code}`：403 / 404 表示尚未批准；成功时返回 `{authorization_code, code_verifier}`。
3. `POST https://auth.openai.com/oauth/token`（form）：`grant_type=authorization_code`、`client_id`、`code`、`code_verifier`、`redirect_uri=https://auth.openai.com/deviceauth/callback` → `{access_token, refresh_token, id_token}`。

**刷新**：`grant_type=refresh_token`、`scope=openid profile email`。

- daemon 每 5 分钟检查一次，在到期前 24 小时内刷新。
- refresh token 会轮换：新 secret 先写入，SQLite 引用随后切换，旧 secret 最后删除。
- 遇到 `invalid_grant` 或 `refresh_token_reused`：账号标记为需要重新登录，不再重试。

**存储**：

- access token 与 refresh token 各存一条 Credential Manager 记录（access token 约 1.8 KB，小于 2560 B 上限）。
- SQLite 只保存 `secret_ref`、`expires_at`。
- id_token 只用来读取 email（作为显示名），不保存。
- `chatgpt_account_id` 在构建路由时从 access token 的 claims 读取。

**与官方 CLI 的关系**：Loadout 不读取、不刷新 `~/.codex/auth.json`，两份登录互不影响 token 轮换。

## Inference

- `POST https://chatgpt.com/backend-api/codex/responses`，只支持流式 Responses。
  - 非流式客户端：上游仍走流式，Loadout 聚合后返回完整 JSON。
  - Chat 和 Messages 客户端经协议转换使用。
- **请求头**：
  - `Authorization: Bearer`、`Chatgpt-Account-Id`、`Accept: text/event-stream`；
  - `User-Agent: loadout/<版本>`（如实标识）；
  - 只有设置 `client_identity=codex` 时才发送 `Originator: codex_cli_rs`。
- **请求体规范化**（`prepare_request`）：
  - 强制 `stream=true`、`store=false`；
  - 删除 `previous_response_id`、`max_output_tokens`、`max_tokens`、`temperature`、`top_p`、`user`、`truncation`、`prompt_cache_retention`、`safety_identifier`、`stream_options`、`service_tier`；
  - 缺少 `instructions` 时补 `""`；字符串 `input` 转为消息数组；`system` 角色改为 `developer`。
  - 因此**不支持有状态 Responses**（`previous_response_id`）。
- **错误分类**：
  - `usage_limit_reached` → 额度耗尽，冷却到 `resets_at` / `resets_in_seconds`；
  - 401 / `refresh_token_reused` → 认证失败；
  - "at capacity" / overloaded → 上游繁忙；
  - `context_length_exceeded` → 请求错误，不 failover。
- **护栏**：
  - 每账号最多 2 个并发请求，超出时跳过该账号；
  - 不使用 TLS 指纹伪装。
- **模型目录**：`GET …/codex/models?client_version=0.154.0`，只取 `visibility=list` 且 `supported_in_api` 的模型。
- **用量**（ExecPlan 013，2026-10-04 对照 openai/codex `main` 源码核对）：
  - 被动（主要来源，ADR-012 §1）：推理响应头 `x-codex-{primary,secondary}-used-percent`、`-window-minutes`、`-reset-at`（Unix 秒）。见 `codex-rs/codex-api/src/rate_limits.rs`；旧版 `-reset-after-seconds` 作为后备。Gateway 在选路之后只解析响应头，交给控制面队列；每账号最多 30 秒写一次数据库，路由状态每次都更新。
  - 主动（后备，每 30 分钟）：`GET https://chatgpt.com/backend-api/wham/usage`，与官方客户端读取额度的接口相同（`codex-rs/backend-client` 的 `rate_limit_status_url`）。不发送 prompt，不消耗额度；字段 `rate_limit.{primary,secondary}_window.{used_percent, limit_window_seconds, reset_at}`、`plan_type`。
  - 窗口按时长映射为 `5h` / `weekly`，可用于 `loadout account reserve`。

## Settings

| 键 | 默认 | 说明 |
|---|---|---|
| `client_identity` | `loadout` | `codex` 时附加 `Originator: codex_cli_rs`。只在如实标识被上游拒绝、且你接受更高风险时使用：`loadout provider set codex-sub client_identity=codex` |
| `auth_base_url` | `https://auth.openai.com` | 测试用 |

## Terms-of-Service Risk（2026-10-04 复核）

**中至高。**

- [OpenAI Terms of Use](https://openai.com/policies/terms-of-use/)（2026-01-01 生效）的 "What you cannot do" 一节禁止：
  - 程序化提取输出；
  - 绕过速率限制或保护措施；
  - 共享账号凭证。
- [Sign in with ChatGPT](https://developers.openai.com/siwc/token-sharing-open-source) 是 OpenAI 为开源 / 本地应用提供的正式通道。但其条款要求额度只供连接的应用自己使用，不得给其他工具提供通用 API。Loadout 作为网关，与这一点冲突，因此本 provider 没有走 SIWC。
- 社区报告的封号案例与代理的直接关联弱于 Claude 和 Gemini，但确实存在，例如 [CLIProxyAPI #2396](https://github.com/router-for-me/CLIProxyAPI/issues/2396)。高并发和多人共享是常见诱因。
- 结论：只在个人自用、低并发的场景下使用，不要把 client key 分享给他人。

## Implementation References

- OpenAI Codex（Apache-2.0）：官方认证与 app-server 文档（learn.chatgpt.com/docs/auth）；用量接口与响应头见 `codex-rs/backend-client/src/client/rate_limit_resets.rs`、`codex-rs/codex-api/src/rate_limits.rs`（只用接口知识，未复制代码）。
- CLIProxyAPI（MIT，`d7914afdedca7af95ee974a42453dc49fc1388ce`）：
  - `sdk/auth/codex_device.go`、`internal/auth/codex/openai_auth.go`、`internal/auth/codex/jwt_parser.go`；
  - `internal/runtime/executor/codex_executor_*.go`、`helps/codex_quota.go`；
  - `internal/registry/models/codex_client_models.json`。
  - 只使用了接口知识（端点、字段、错误类型），没有复制代码；见 THIRD_PARTY_NOTICES.md。
