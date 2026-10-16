# StepFun 阶跃星辰 (`stepfun`)

- **Last Verified Date**: 2026-10-17
- **Module**: `crates/providers/src/presets.rs` (row `stepfun`, ExecPlan 018)

## Official Authentication Method

API key; `Authorization: Bearer <key>`.

## Official API Availability

- Chat: `https://api.stepfun.com/v1/chat/completions`.
- Messages: `https://api.stepfun.com/v1/messages`.
- Catalog: `GET /v1/models` (includes vision models, which are chat models and kept).
- Balance: none documented.

Notes: Step Plan keys use `/step_plan/v1`.

## Subscription Transport Status

N/A (pay-as-you-go API key). Tier A. Plan-specific keys that need another host are not covered by the default base URL; use `--base-url`.

## Terms-of-Service Risk

Low (official API with the user's own key).

## Implementation References

- Official documentation only; no code copied.
- https://platform.stepfun.com/docs/zh/api-reference/models/list
- https://platform.stepfun.com/docs/zh/api-reference/chat/messages-create
