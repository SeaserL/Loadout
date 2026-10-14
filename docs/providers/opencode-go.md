# OpenCode Go (`opencode-go`)

- **Last Verified Date**: 2026-10-15
- **Module**: `crates/providers/src/opencode_go.rs`

## Official Authentication Method

API key created in the OpenCode console; `Authorization: Bearer <key>`. Messages requests additionally send `x-api-key` and `anthropic-version`.

## Official API Availability

- Base URL `https://opencode.ai/zen/go/v1` with `/chat/completions`, `/responses`, `/messages`, `/models` (opencode.ai/docs/go).
  - 2026-10-15 的文档改用 `https://opencode.ai/inference/go/v1`；旧路径当天仍可用。默认值暂不变（ExecPlan 016 §12），需要时可用 `--base-url` 覆盖。
- Each model is served over one protocol, by family (docs model table, 2026-10-15). Loadout applies this mapping when syncing models; override with `loadout model add <provider> <alias> <model> --protocol …`.
  - MiniMax / Qwen / Claude → Messages
  - Grok / GPT / Muse Spark → Responses
  - others → Chat
  - Gemini models use Google's native `/models/<id>` endpoint, which Loadout does not speak; they are mapped to Chat and will fail.
- **Session header (required since 2026-10)**: every request must carry a stable per-conversation id in `x-opencode-session`, otherwise the upstream answers `HTTP 400: Request is missing x-opencode-session`. The module declares it through `session_header()`; the gateway derives the value (ADR-013), probes use a random id.
- Clients should identify with their own User-Agent (Loadout sends `Loadout/<version>`).
- Usage: `GET /zen/go/v1/usage` → `usage.{rolling, weekly, monthly}` with `usagePercent` (or `used`/`limit`) and `resetInSec`; limits are 20 % (5 h) / 50 % (weekly) / 100 % (monthly) of the plan's monthly allowance.

## Subscription Transport Status

Allowed (tier A, `OFFICIAL_SUBSCRIPTION_API`): the Go plan officially issues API keys for third-party tools. The docs say traffic is monitored for abuse and validated clients may stop working in the future.

## Terms-of-Service Risk

Low — documented API with user-issued keys.

## Implementation References

- OpenCode docs (opencode.ai/docs/go) — endpoints, model families, session header requirement.
- CodexBar (MIT) `docs/opencode.md`, `OpenCodeGoUsageFetcher.swift` — usage endpoint and field names. Interface knowledge only; no code copied.
