# SiliconFlow 硅基流动 (`siliconflow`)

- **Last Verified Date**: 2026-10-17
- **Module**: `crates/providers/src/presets.rs` (row `siliconflow`, ExecPlan 018)

## Official Authentication Method

API key; `Authorization: Bearer <key>` (Messages also accepts `x-api-key`).

## Official API Availability

- Chat: `https://api.siliconflow.cn/v1/chat/completions`. International: `--base-url https://api.siliconflow.com/v1`.
- Messages: `https://api.siliconflow.cn/v1/messages`.
- Catalog: `GET /v1/models?sub_type=chat` (server-side filter).
- Balance: `GET /v1/user/info` was retired on 2026-08-14 with no replacement yet, so usage is estimated.

Notes: Model ids contain `/` (`Qwen/Qwen3-…`).

## Subscription Transport Status

N/A (pay-as-you-go API key). Tier A. Plan-specific keys that need another host are not covered by the default base URL; use `--base-url`.

## Terms-of-Service Risk

Low (official API with the user's own key).

## Implementation References

- Official documentation only; no code copied.
- https://docs.siliconflow.com/cn/api-reference/models/get-model-list
- https://docs.siliconflow.com/en/api-reference/chat-completions/messages
- https://docs.siliconflow.cn/docs/release-notes/overview
