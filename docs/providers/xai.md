# xAI (Grok) (`xai`)

- **Last Verified Date**: 2026-10-17
- **Module**: `crates/providers/src/presets.rs` (row `xai`, ExecPlan 018)

## Official Authentication Method

API key; `Authorization: Bearer <key>`.

## Official API Availability

- Chat: `https://api.x.ai/v1/chat/completions` (`max_completion_tokens`).
- Messages: `/v1/messages` is deprecated by xAI; not used.
- Catalog: `GET /v1/models` (image models filtered by id).
- Balance: Management API only (needs a management key); usage is estimated.

## Subscription Transport Status

N/A (pay-as-you-go API key). Tier A. Plan-specific keys that need another host are not covered by the default base URL; use `--base-url`.

## Terms-of-Service Risk

Low (official API with the user's own key).

## Implementation References

- Official documentation only; no code copied.
- https://docs.x.ai/developers/rest-api-reference/inference/models
- https://docs.x.ai/developers/rest-api-reference/management/billing
