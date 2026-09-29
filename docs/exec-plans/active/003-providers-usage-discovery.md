# ExecPlan 003: Built-in Providers, Usage Tracking and Local Agent Discovery

- **Status**: In progress
- **Created**: 2026-09-29
- **Phase**: Harness §26 Phase 3（Discovery、Usage model）+ Phase 4（OpenCode Go、Command Code、OpenRouter、generic）+ Phase 5（Codex、Claude Code、Kimi Code、Gemini CLI —— 按 ADR-009 只做发现与用量）

---

## 1. Goal

- 内置 Provider 模块：`opencode-go`、`commandcode`、`kimi-code`、`openrouter`、`deepseek`、`openai`、`anthropic`、`ollama`（全部是 tier A 推理 transport），以及订阅类 `codex`、`claude-code`、`gemini-cli`（只有 Discovery / Auth / Usage）。
- 用量模型：每个账号最新的额度窗口、余额、订阅计划，后台定期刷新；额度耗尽时自动让该账号的路由不可用，直到 reset（ADR-003 §9）。
- 本机 Agent 扫描：`loadout scan` 显示已安装 / 已登录的 AI 工具，用户选择 `import` 后才建立账号（Harness §12）。
- CLI：`loadout usage`、`loadout scan`、`loadout scan import`、`loadout account plan`。

## 2. Research Summary（2026-09-29 验证）

| Provider | 推理 | 用量来源 | Trust |
|---|---|---|---|
| OpenCode Go | `https://opencode.ai/zen/go/v1`；按模型区分 Chat / Responses / Messages；Bearer | `GET /zen/go/v1/usage` → `usage.{rolling,weekly,monthly}.{usagePercent \| used/limit, resetInSec}` | OFFICIAL_SUBSCRIPTION_API |
| Command Code | `https://api.commandcode.ai/provider/v1`；Chat / Responses / Messages；Bearer（Messages 也接受 `x-api-key`）；`/models` 每个模型带 `supported_endpoints` | 无 API key 可用的用量接口（官方只有网页 cookie 接口，ADR-009 禁止）→ 由本地请求记录估算 | OFFICIAL_SUBSCRIPTION_API |
| Kimi Code | `https://api.kimi.ai/coding/v1`（国内 `api.kimi.com`）；Chat + Messages；API key 在 Kimi Code 控制台创建，官方支持第三方工具 | `GET {base}/usages`；也可用官方 CLI 登录（`~/.kimi-code/credentials/kimi-code.json`，只读，不刷新） | OFFICIAL_SUBSCRIPTION_API |
| OpenRouter | `https://openrouter.ai/api/v1`；Chat；`/models` 含价格（USD/token 字符串）、context、模态、支持参数 | `GET /api/v1/key` → `limit`、`limit_remaining`、`usage_daily/weekly/monthly`、`limit_reset` | OFFICIAL_API |
| DeepSeek | `https://api.deepseek.com/v1`（Chat），`https://api.deepseek.com/anthropic`（Messages） | `GET https://api.deepseek.com/user/balance` | OFFICIAL_API |
| OpenAI / Anthropic | 官方 API | 普通 key 无用量接口 → 估算 | OFFICIAL_API |
| Ollama | `http://127.0.0.1:11434/v1` | — | LOCAL |
| Codex | ADR-009：不做推理 | 官方 `codex app-server`（JSON-RPC over stdio JSONL，无 `jsonrpc` 字段；`initialize` → `initialized` → `account/rateLimits/read`）；兜底：`~/.codex/sessions/**/rollout-*.jsonl` 中最后的 `token_count.rate_limits` | OFFICIAL_CLI / LOCAL_FILES |
| Claude Code | ADR-009：不做推理 | `GET https://api.anthropic.com/api/oauth/usage`（`anthropic-beta: oauth-2025-04-20`，Bearer = `~/.claude/.credentials.json` 的 `claudeAiOauth.accessToken`）；`five_hour` / `seven_day` / `seven_day_opus` / `seven_day_sonnet`，`utilization` 为百分比 | PRIVATE_ENDPOINT |
| Gemini CLI | ADR-009：不做推理 | `POST https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist` → project；`POST …:retrieveUserQuota` → `buckets[].{remainingFraction, resetTime, modelId}`；token 来自 `~/.gemini/oauth_creds.json` | PRIVATE_ENDPOINT |

参考：CodexBar（MIT，`docs/providers.md` 与各 Provider 源码）——只借鉴接口与字段信息，实现为原创代码；官方文档：Command Code Provider API、OpenCode Go、Kimi Code、OpenRouter、Codex app-server。

## 3. Constraints

- ADR-009：订阅类官方 CLI 的凭据**只读、按需读取、不缓存、不刷新、不复制**；token 过期时报告"需要运行官方 CLI 刷新"。私有端点刷新间隔 ≥ 5 分钟，失败退避。
- Harness §12：扫描只由用户触发；不读浏览器 cookie；不输出 token；import 需用户明确选择。
- 用量刷新属于控制面，绝不在请求路径上。
- Provider 特有逻辑（模型协议映射、区域 URL、响应解析）封装在各自模块中（ADR-005）。

## 4. Design

### 4.1 Usage storage（migration 0003）

```sql
usage_snapshots(account_id PK→accounts, fetched_at, source, confidence, plan_json, account_label, error)
quota_windows(account_id, scope, label, unit, used, limit_value, window_seconds, resets_at, observed_at,
              PRIMARY KEY(account_id, scope))
balances(account_id, currency, amount, included, topup, granted, observed_at, PRIMARY KEY(account_id, currency))
```

每次刷新整体替换该账号的窗口与余额（只保留最新值；历史趋势由 request_events 与 Phase 9 统计提供）。

### 4.2 Usage poller（控制面，restartable）

- 每个启用、且模块实现了 Usage driver 的账号：API 类默认每 5 分钟、订阅 / 私有端点类默认每 10 分钟刷新；失败时指数退避（上限 1 小时）。
- 结果写入 4.1；若 `UsageReport::is_exhausted()`（或低于 `routing.quota_reserve_percent`），把该账号所有 target 标记为 `QUOTA_EXHAUSTED` 直到最早的 reset；恢复后清除。
- `loadout usage --refresh` 立即刷新。

### 4.3 Discovery（Phase 5）

- `ProviderModule::discover()`（新 driver 方法，默认"不支持"）：检测可执行文件（PATH）、配置 / 凭据文件是否存在、登录类型；**不读取 token 内容**（只判断存在与过期时间字段）。
- `discovery.scan` → 列表；`discovery.import {kind}` → 建立固定 slug 的 Provider（若不存在）+ 账号（alias 默认 `main`，credential `external_managed`，source 为 CLI 名）。
- 用户目录可通过隐藏参数 `--home-dir` 覆盖（测试用）。

### 4.4 Model protocol mapping

- Command Code：`supported_endpoints` → 协议。
- OpenCode Go：按官方文档的模型族映射（MiniMax / Qwen → Messages，Grok → Responses，其余 → Chat），可被 `model add --protocol` 覆盖。
- OpenRouter：模态含 image → VISION；`supported_parameters` 含 tools / reasoning → 对应能力；价格 × 1e6 → USD / 1M tokens。

## 5. Stages

1. Usage storage + poller + 额度驱动的路由可用性 + `usage` IPC/CLI。
2. API 类内置 Provider（8 个）+ 目录 / 用量驱动 + fixture 测试。
3. 订阅类 Discovery + Usage（Codex、Claude Code、Gemini CLI、Kimi CLI 登录）+ `scan` / `import`。
4. `account plan`（价格、续费日，与额度 reset 分开，产品文档 §35）、端到端测试、文档。

## 6. Tests

- 每个用量解析器：真实形状的脱敏 fixture（含字段缺失、百分比 / 绝对值两种形式、秒 / 毫秒时间戳）。
- Poller：额度耗尽 → target 不可用 → reset 后恢复。
- Discovery：假的 home 目录（含 / 不含凭据文件、过期 token），断言不读取 token 值到任何输出。
- Codex app-server：用假的可执行脚本模拟 JSONL 协议。

## 7. Security

- 订阅凭据文件只在 usage 刷新时读取，读出的 token 仅用于该次 HTTP 请求（`Secret` 包装），不写日志、不入库、不经 IPC。
- 诚实的 User-Agent（`Loadout/<version>`），不冒充官方客户端。
- 私有端点逐个可关闭：账号设置 `usage_enabled`。

## 8. Known Risks

| 风险 | 缓解 |
|---|---|
| 私有端点字段变化 / 被封 | 宽容解析；失败只影响用量显示，不影响路由；记录 error |
| Codex app-server 首次调用返回空 | 握手后重试一次；兜底读本地会话日志 |
| Windows 上 `codex` 是 npm 的 `.cmd` 包装 | 通过 `cmd /c` 启动或解析 PATHEXT |
| Kimi / OpenCode 模型族映射随上游变化 | 用户可覆盖；文档记录 |

## 9. Definition of Done

- [ ] §1 全部功能；测试覆盖 §6
- [ ] clippy / fmt / 全部测试通过
- [ ] 每个 Provider 的 `docs/providers/<slug>.md`（ADR-005 §6）
- [ ] THIRD_PARTY_NOTICES 记录借鉴来源（接口知识，无代码复制）
- [ ] 本文件移入 `completed/`

## 10. Progress

- [x] Stage 1 — usage storage + poller（2026-09-29）
  - Migration 0003（usage_snapshots / quota_windows / balances），刷新失败时保留上次的有效值并记录错误。
  - `DriverContext.home`（官方 CLI 状态所在的用户目录，daemon 隐藏参数 `--home-dir` 可覆盖）；`ProviderModule::usage_interval()`（默认 5 分钟）。
  - 后台 poller（restartable）：每 60 秒检查到期账号；失败按间隔指数退避，上限 1 小时。
  - 额度驱动路由：报告耗尽（或低于 `routing.quota_reserve_percent`）→ 该账号所有 target 标记 `QUOTA_EXHAUSTED` 至最早 reset（无 reset 时 1 小时）；后续报告恢复时解除。
  - `usage.list`（可选先刷新）+ `account.set_plan`；CLI `loadout usage [--refresh]`、`loadout account plan --name --price --renews YYYY-MM-DD`。计划信息中用户填写的值优先于上游报告的值；续费日与额度 reset 分开展示。
  - 今日 / 本月消费按 UTC 划分周期（daemon 不依赖时区数据库），来源为 request_events。
- [ ] Stage 2 — API providers
- [ ] Stage 3 — subscription discovery + usage
- [ ] Stage 4 — plans, e2e, docs

## 11. Decisions

- Command Code 用量无 API key 接口 → 估算（来自 request_events 的成本），置信度 `ESTIMATED`。
- 订阅 CLI 的 token 过期时不刷新，报告 `AUTH_REQUIRED`（"运行一次官方 CLI"），避免与官方 CLI 的 token 轮换冲突（ADR-004 §2）。
