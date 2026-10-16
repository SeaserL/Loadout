# Volcengine Ark 火山方舟 (Doubao) (`volcengine`)

- **Last Verified Date**: 2026-10-17
- **Module**: `crates/providers/src/presets.rs` (row `volcengine`, ExecPlan 018)

## Official Authentication Method

API key; `Authorization: Bearer <key>`.

## Official API Availability

- Chat: `https://ark.cn-beijing.volces.com/api/v3/chat/completions`.
- Messages: `https://ark.cn-beijing.volces.com/api/compatible/v1/messages`.
- Catalog: `GET /api/v3/models` is not documented (the server checks auth before routing). If it fails, add models by hand with `loadout model add volcengine <account> <model-id>`; plain model ids (`doubao-seed-…-YYMMDD`) and custom endpoint ids (`ep-…`) both work.
- Balance: none with the API key.

Notes: Agent / Coding Plan keys use `/api/plan` or `/api/coding` and do not work on the default base URL.

## Subscription Transport Status

N/A (pay-as-you-go API key). Tier A. Plan-specific keys that need another host are not covered by the default base URL; use `--base-url`.

## Terms-of-Service Risk

Low (official API with the user's own key).

## Implementation References

- Official documentation only; no code copied.
- https://ark.volcengine.com/region:cn-beijing/docs/82379/2160841
