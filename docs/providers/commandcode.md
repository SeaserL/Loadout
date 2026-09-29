# Command Code (`commandcode`)

- **Last Verified Date**: 2026-09-29
- **Module**: `crates/providers/src/commandcode.rs`

## Official Authentication Method

Provider API key (Studio → API keys); `Authorization: Bearer <key>` (Messages also accepts `x-api-key`). Plans with API access: GOAT, Pro, Max, Team, Provider (not Go).

## Official API Availability

- Base URL `https://api.commandcode.ai/provider/v1`: `/chat/completions`, `/responses`, `/messages`, `/models` (commandcode.ai/docs/provider).
- `/models` lists each model's `supported_endpoints`; Loadout maps them to protocols.
- **No usage endpoint for API keys.** The web dashboard's 5-hour / weekly / monthly credit data is only available with browser session cookies, which Loadout does not read (Harness §12, ADR-009). Usage shown by Loadout is estimated from its own request records; record the plan with `loadout account plan`.

## Subscription Transport Status

Allowed (tier A, `OFFICIAL_SUBSCRIPTION_API`).

## Terms-of-Service Risk

Low — documented provider API.

## Implementation References

- Command Code Provider API docs.
- CodexBar (MIT) `docs/command-code.md` — confirms the cookie-only usage source (not used).
