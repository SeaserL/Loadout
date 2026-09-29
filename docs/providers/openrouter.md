# OpenRouter (`openrouter`)

- **Last Verified Date**: 2026-09-29
- **Module**: `crates/providers/src/openrouter.rs`

## Official Authentication Method

API key; `Authorization: Bearer <key>`.

## Official API Availability

- `https://openrouter.ai/api/v1/chat/completions`.
- `GET /models`: `context_length`, `pricing` (USD per token as strings → converted to USD / 1M), `top_provider.max_completion_tokens`, `architecture.input_modalities` (image → vision), `supported_parameters` (tools / reasoning).
- Usage: `GET /api/v1/key` → `limit`, `limit_remaining` (shown as balance), `usage_daily/weekly/monthly` (spend), `free_model_daily_requests`, `is_free_tier`.
- Model ids contain `/` (e.g. `anthropic/claude-x`); avoid account aliases equal to a vendor prefix (ADR-010 §3 — Loadout rejects them).

## Subscription Transport Status

N/A (pay-as-you-go API). Tier A.

## Terms-of-Service Risk

Low.

## Implementation References

- OpenRouter API reference (limits, models).
