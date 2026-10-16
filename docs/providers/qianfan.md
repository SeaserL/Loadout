# Baidu Qianfan 百度千帆 (`qianfan`)

- **Last Verified Date**: 2026-10-17
- **Module**: `crates/providers/src/presets.rs` (row `qianfan`, ExecPlan 018)

## Official Authentication Method

API key; `Authorization: Bearer <key>` for Chat, `x-api-key` for Messages.

## Official API Availability

- Chat: `https://qianfan.baidubce.com/v2/chat/completions`.
- Messages: `https://qianfan.baidubce.com/anthropic/v1/messages`.
- Catalog: `GET /v2/models` (not documented, but the route exists: 403, not 404, without a valid key).
- Balance: none with the API key.

Notes: Coding Plan uses `/v2/coding` and `/anthropic/coding`.

## Subscription Transport Status

N/A (pay-as-you-go API key). Tier A. Plan-specific keys that need another host are not covered by the default base URL; use `--base-url`.

## Terms-of-Service Risk

Low (official API with the user's own key).

## Implementation References

- Official documentation only; no code copied.
- https://cloud.baidu.com/doc/qianfan/s/Hmh4suq26
- https://cloud.baidu.com/doc/qianfan-docs/s/6mh3e6gjp
