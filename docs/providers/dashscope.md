# Alibaba Bailian 阿里云百炼 (Qwen) (`dashscope`)

- **Last Verified Date**: 2026-10-17
- **Module**: `crates/providers/src/presets.rs` (row `dashscope`, ExecPlan 018)

## Official Authentication Method

API key (region-bound: a Beijing key fails elsewhere with 401 `invalid_api_key`); `Authorization: Bearer <key>`.

## Official API Availability

- Chat: `https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions` (Beijing). Workspace host `https://{WorkspaceId}.cn-beijing.maas.aliyuncs.com/compatible-mode/v1` also works via `--base-url`.
- Messages: `https://dashscope.aliyuncs.com/apps/anthropic/v1/messages`.
- Catalog: `/models` is not documented; on 404 Loadout uses `qwen3.8-max`, `qwen3.7-plus`, `qwen3.8-flash`. Speech / image ids (`paraformer`, `cosyvoice`, `wanx`…) are filtered.
- Responses: stateful; not used.
- Balance: none with the API key.

Notes: Only `messages[0]` may be system. Coding Plan keys use separate hosts.

## Subscription Transport Status

N/A (pay-as-you-go API key). Tier A. Plan-specific keys that need another host are not covered by the default base URL; use `--base-url`.

## Terms-of-Service Risk

Low (official API with the user's own key).

## Implementation References

- Official documentation only; no code copied.
- https://help.aliyun.com/zh/model-studio/compatibility-of-openai-with-dashscope
- https://help.aliyun.com/zh/model-studio/claude-code
- https://help.aliyun.com/zh/model-studio/models
