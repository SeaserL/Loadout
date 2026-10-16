# Google Gemini API (`gemini`)

- **Last Verified Date**: 2026-10-17
- **Module**: `crates/providers/src/presets.rs` (row `gemini`, ExecPlan 018)

## Official Authentication Method

Gemini API key; `Authorization: Bearer <key>` on the OpenAI-compatible endpoint.

## Official API Availability

- Chat: `https://generativelanguage.googleapis.com/v1beta/openai/chat/completions` (beta).
- Catalog: `GET /v1beta/openai/models`; the `models/` prefix is stripped; embedding / Imagen / Veo / AQA models are filtered.
- Balance: none.

Notes: `reasoning_effort` and `thinking_*` cannot be combined. This is the API-key product, unrelated to the `gemini-cli` subscription.

## Subscription Transport Status

N/A (pay-as-you-go API key). Tier A. Plan-specific keys that need another host are not covered by the default base URL; use `--base-url`.

## Terms-of-Service Risk

Low (official API with the user's own key).

## Implementation References

- Official documentation only; no code copied.
- https://ai.google.dev/gemini-api/docs/openai
