# Zhipu BigModel 智谱 GLM (`zhipu`)

- **Last Verified Date**: 2026-10-17
- **Module**: `crates/providers/src/presets.rs` (row `zhipu`, ExecPlan 018)

## Official Authentication Method

API key; `Authorization: Bearer <key>` for Chat, `x-api-key` for Messages (Loadout sends both).

## Official API Availability

- Chat: `https://open.bigmodel.cn/api/paas/v4/chat/completions`. International (Z.ai): `--base-url https://api.z.ai/api/paas/v4`.
- Messages: `https://open.bigmodel.cn/api/anthropic/v1/messages`.
- Catalog: `GET /api/paas/v4/models` is not documented; on 404 Loadout uses `glm-5.3`, `glm-5.2`, `glm-5.1`, `glm-4.7-flash`.
- Responses: separate stateful API at `/api/v1/responses`; not used.
- Balance: none documented.

Notes: `temperature` range is (0, 1). GLM Coding Plan keys use their own host.

## Subscription Transport Status

N/A (pay-as-you-go API key). Tier A. Plan-specific keys that need another host are not covered by the default base URL; use `--base-url`.

## Terms-of-Service Risk

Low (official API with the user's own key).

## Implementation References

- Official documentation only; no code copied.
- https://docs.bigmodel.cn/cn/guide/develop/openai/introduction
- https://docs.bigmodel.cn/cn/guide/develop/claude/introduction
- https://docs.bigmodel.cn/cn/guide/start/model-overview
