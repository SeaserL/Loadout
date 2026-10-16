# Moonshot AI (Kimi API) (`moonshot`)

- **Last Verified Date**: 2026-10-17
- **Module**: `crates/providers/src/presets.rs` (row `moonshot`, ExecPlan 018)

## Official Authentication Method

API key; `Authorization: Bearer <key>` (also on the Anthropic endpoint).

## Official API Availability

- Chat: `https://api.moonshot.cn/v1/chat/completions`. International: `--base-url https://api.moonshot.ai/v1`.
- Messages: `https://api.moonshot.cn/anthropic/v1/messages`.
- Responses: stateless only; not used.
- Catalog: `GET /v1/models` (`context_length`, `supports_*`).
- Balance: `GET /v1/users/me/balance` → `data.available_balance` (amount), `voucher_balance` (included), `cash_balance` (top-up), CNY.

Notes: This is the pay-as-you-go open platform. Kimi Code membership keys are the separate `kimi-code` provider.

## Subscription Transport Status

N/A (pay-as-you-go API key). Tier A. Plan-specific keys that need another host are not covered by the default base URL; use `--base-url`.

## Terms-of-Service Risk

Low (official API with the user's own key).

## Implementation References

- Official documentation only; no code copied.
- https://platform.kimi.com/docs/api/list-models.md
- https://platform.kimi.com/docs/api/messages
- https://platform.kimi.com/docs/api/balance.md
