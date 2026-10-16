# MiniMax (`minimax`)

- **Last Verified Date**: 2026-10-17
- **Module**: `crates/providers/src/presets.rs` (row `minimax`, ExecPlan 018)

## Official Authentication Method

API key; `Authorization: Bearer <key>`.

## Official API Availability

- Chat: `https://api.minimaxi.com/v1/chat/completions` (China; `api.minimax.cn` answers too). International: `--base-url https://api.minimax.io/v1`.
- Messages: `https://api.minimaxi.com/anthropic/v1/messages` (ignores `top_k`, `stop_sequences`).
- Catalog: `GET /v1/models`.
- Balance: no pay-as-you-go balance API (`/v1/token_plan/remains` is Token Plan only).

## Subscription Transport Status

N/A (pay-as-you-go API key). Tier A. Plan-specific keys that need another host are not covered by the default base URL; use `--base-url`.

## Terms-of-Service Risk

Low (official API with the user's own key).

## Implementation References

- Official documentation only; no code copied.
- https://platform.minimax.cn/docs/api-reference/models/openai/list-models.md
