# OpenCode Go (`opencode-go`)

- **Last Verified Date**: 2026-09-29
- **Module**: `crates/providers/src/opencode_go.rs`

## Official Authentication Method

API key created in the OpenCode console; `Authorization: Bearer <key>`. Messages requests additionally send `x-api-key` and `anthropic-version`.

## Official API Availability

- Base URL `https://opencode.ai/zen/go/v1` with `/chat/completions`, `/responses`, `/messages`, `/models` (opencode.ai/docs/go).
- Each model is served over one protocol, by family: MiniMax / Qwen → Messages, Grok → Responses, others → Chat. Loadout applies this mapping when syncing models; override with `loadout model add <provider> <alias> <model> --protocol …`.
- Usage: `GET /zen/go/v1/usage` → `usage.{rolling, weekly, monthly}` with `usagePercent` (or `used`/`limit`) and `resetInSec`; limits are 20 % (5 h) / 50 % (weekly) / 100 % (monthly) of the plan's monthly allowance.

## Subscription Transport Status

Allowed (tier A, `OFFICIAL_SUBSCRIPTION_API`): the Go plan officially issues API keys for third-party tools.

## Terms-of-Service Risk

Low — documented API with user-issued keys.

## Implementation References

- OpenCode docs (opencode.ai/docs/go) — endpoints and model families.
- CodexBar (MIT) `docs/opencode.md`, `OpenCodeGoUsageFetcher.swift` — usage endpoint and field names. Interface knowledge only; no code copied.
