# SenseNova 商汤日日新 (`sensenova`)

- **Last Verified Date**: 2026-10-17
- **Module**: `crates/providers/src/presets.rs` (row `sensenova`, ExecPlan 018)

## Official Authentication Method

API key from https://platform.sensenova.cn/console/keys (`sk-…`); `Authorization: Bearer <key>` on every endpoint. The legacy AK/SK → JWT scheme belongs to the old `api.sensenova.cn/v1/llm/*` API and does not apply.

## Official API Availability

- Chat: `POST https://token.sensenova.cn/v1/chat/completions` (`max_tokens`; `stream_options.include_usage` supported; `tools` / `tool_choice`).
- Messages: `POST https://token.sensenova.cn/v1/messages` (docs use `ANTHROPIC_AUTH_TOKEN`, i.e. Bearer; Loadout sends Bearer and `x-api-key`).
- Responses: `POST /v1/responses`, stateless only (`store` / `previous_response_id` unsupported). Not used by Loadout; Responses requests are converted to Chat.
- Catalog: `GET /v1/models`, OpenRouter-style entries (`context_length`, `output_modalities`, `pricing`). Image models (`sensenova-u1.5-*`, `output_modalities: ["image"]`) are dropped. Prices are not imported (currency undocumented).
- Fallback models: `sensenova-6.8-flash-lite`, `deepseek-v4-flash`, `deepseek-flash`, `glm-5.2`, `kimi-k3`.
- Usage: no API-key endpoint. Token Plan quota (5h / weekly points per pool) is console-only (SenseCore IAM login), so usage is estimated.
- Errors: documented `{"error":{"type","code","message"}}`; auth failures observed as 401 `{"error":{"code":16,"message":"Forbidden"}}`. 429 `quota_exceeded_error` covers both rate limits and points exhaustion.

Notes: Kimi K3 documents `max_completion_tokens` (Loadout renames `max_tokens` for `kimi-k3*` on Chat); the others use `max_tokens`. Per-model reasoning output field differs (`reasoning` vs `reasoning_content`).

## Subscription Transport Status

N/A (pay-as-you-go API key). Tier A. Plan-specific keys that need another host are not covered by the default base URL; use `--base-url`.

## Terms-of-Service Risk

Low (official API with the user's own key).

## Implementation References

- Official documentation only; no code copied.
- https://platform.sensenova.cn/docs (quickstart, auth, api-compatibility, list-models, errors, points)
