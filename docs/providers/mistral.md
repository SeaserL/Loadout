# Mistral AI (`mistral`)

- **Last Verified Date**: 2026-10-17
- **Module**: `crates/providers/src/presets.rs` (row `mistral`, ExecPlan 018)

## Official Authentication Method

API key; `Authorization: Bearer <key>`.

## Official API Availability

- Chat: `https://api.mistral.ai/v1/chat/completions`.
- Catalog: `GET /v1/models`; entries with `capabilities.completion_chat: false` and OCR / embedding / moderation models are dropped.
- Balance: none.

## Subscription Transport Status

N/A (pay-as-you-go API key). Tier A. Plan-specific keys that need another host are not covered by the default base URL; use `--base-url`.

## Terms-of-Service Risk

Low (official API with the user's own key).

## Implementation References

- Official documentation only; no code copied.
- https://docs.mistral.ai/api/endpoint/models
