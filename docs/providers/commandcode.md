# Command Code (`commandcode`)

- **Last Verified Date**: 2026-10-16
- **Module**: `crates/providers/src/commandcode.rs`

## Official Authentication Method

Provider API key (Studio → API keys); `Authorization: Bearer <key>` (Messages also accepts `x-api-key`). Plans with API access: GOAT, Pro, Max, Team, Provider (not Go).

## Official API Availability

- Base URL `https://api.commandcode.ai/provider/v1`: `/chat/completions`, `/responses`, `/messages`, `/models` (commandcode.ai/docs/provider).
- `/models` lists each model's `supported_endpoints`; Loadout maps them to protocols.
- **Usage（undocumented "alpha" routes，随时可能变化）**：官方 Provider API 文档仍未列出用量接口，但 `cmd` CLI 用同一个 API key 调用 API origin 上的 `/alpha/billing/*`（2026-10-16 起由 CodexBar 确认）。Loadout 从 provider base URL 推导 origin（`--base-url` 覆盖与测试 mock 同样适用），请求带 `Authorization: Bearer <key>`、`Accept: application/json`。结果的数据来源标为 `PRIVATE_ENDPOINT`（与其他 CLI 内部用量接口一致）：
  - `GET /alpha/billing/credits`（必需）→ `credits.{monthlyCredits, purchasedCredits, premiumMonthlyCredits, opensourceMonthlyCredits, monthlyCreditsGranted}` 与 `windowLimits.{fiveHour, weekly}.{cap, used, resetAt}`（`windowLimits` 也可能嵌在 `credits` 内）。数值可能是字符串；`resetAt` 可能是 epoch 秒 / 毫秒或 ISO-8601。
    - 5h / Weekly 窗口 = `used / cap` 百分比；`cap` 缺失或为 0 时跳过该窗口。
    - `monthlyCredits` 是剩余的月度额度（USD）。Monthly 窗口：`limit = monthlyCreditsGranted + purchasedCredits`，`used = granted − monthlyCredits`。购买的 top-up 额度在月度额度用完后继续可用，所以算进窗口上限，避免月度额度耗尽时误判账号不可用；有 top-up 时另外显示一行 USD 余额。
    - `monthlyCreditsGranted` 缺失时按订阅 `planId` 查内置额度表（Go 10 / Pro 30 / Pro v1 80 / GOAT 70 / Max 150 / Ultra 300 USD，2026-10-16）；仍未知时只显示 USD 余额。
    - `premiumMonthlyCredits` / `opensourceMonthlyCredits` 含义未确认，暂不显示。
  - `GET /alpha/billing/subscriptions`（可选，常超时，8 秒上限，与 credits 并发）→ `{success, data: null | {planId, status, currentPeriodEnd}}`。`success: true, data: null` = 免费档；`success: false` 或请求失败视为未知，不影响 credits 结果。`currentPeriodEnd` 作为 Monthly 重置时间与 plan 续费日期。
- **不使用** `/internal/billing/*`：那是网页端的 cookie 接口（Harness §12、ADR-009）。

## Subscription Transport Status

Allowed (tier A, `OFFICIAL_SUBSCRIPTION_API`).

## Terms-of-Service Risk

Low — documented provider API with a user-issued key. The usage routes are undocumented but use the same key the official CLI uses, read-only; if they change, usage fails and inference is unaffected.

## Implementation References

- Command Code Provider API docs.
- CodexBar (MIT) `CommandCodeUsageFetcher.swift`, `CommandCodePlanCatalog.swift` — API-key usage routes, field names, plan allowances. Interface knowledge only; no code copied.
