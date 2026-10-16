# Groq (`groq`)

- **Last Verified Date**: 2026-10-17
- **Module**: `crates/providers/src/presets.rs` (row `groq`, ExecPlan 018)

## Official Authentication Method

API key; `Authorization: Bearer <key>`.

## Official API Availability

- Chat: `https://api.groq.com/openai/v1/chat/completions`.
- Catalog: `GET /openai/v1/models`; Whisper, TTS, Guard and `active: false` entries are dropped.
- Balance: none.

Notes: `logprobs`, `logit_bias` and `n > 1` are rejected.

## Subscription Transport Status

N/A (pay-as-you-go API key). Tier A. Plan-specific keys that need another host are not covered by the default base URL; use `--base-url`.

## Terms-of-Service Risk

Low (official API with the user's own key).

## Implementation References

- Official documentation only; no code copied.
- https://console.groq.com/docs/openai
- https://console.groq.com/docs/api-reference
