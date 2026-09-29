# Claude Code / Claude subscription (`claude-code`)

- **Last Verified Date**: 2026-09-29
- **Module**: `crates/providers/src/claude_code.rs`

## Subscription Transport Status

**NOT SUPPORTED in v1** (ADR-009). Loadout never sends prompts with Claude subscription credentials.

## Official Authentication Method

Managed by Claude Code: `%CLAUDE_CONFIG_DIR%` or `~/.claude/.credentials.json` → `claudeAiOauth.{accessToken, expiresAt (ms), subscriptionType}`. Loadout reads the access token only when refreshing usage, never stores or refreshes it; an expired token is reported as "run `claude` once".

## Usage Source (`PRIVATE_ENDPOINT`)

`GET https://api.anthropic.com/api/oauth/usage` with `Authorization: Bearer <access token>` and `anthropic-beta: oauth-2025-04-20` → `five_hour`, `seven_day`, `seven_day_opus`, `seven_day_sonnet`, each `{utilization (percent 0–100), resets_at (ISO 8601)}`. Refreshed every 10 minutes with backoff. Loadout sends its own `User-Agent` (it does not impersonate Claude Code).

## Terms-of-Service Risk

Medium — undocumented endpoint (may change or be blocked). Read-only, low frequency, only for accounts the user imported. Can be removed with `loadout account remove claude-code main`.

## Implementation References

- CodexBar (MIT) `ClaudeOAuthUsageFetcher.swift`, `ClaudeOAuthCredentialModels.swift` — endpoint, headers and fields. Interface knowledge only.
