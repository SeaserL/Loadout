# Gemini CLI / Google login (`gemini-cli`)

- **Last Verified Date**: 2026-09-29
- **Module**: `crates/providers/src/gemini_cli.rs`

## Subscription Transport Status

**NOT SUPPORTED in v1** (ADR-009). Google's documentation warns that using the Gemini CLI backend through third-party software may violate its terms. Use a Gemini API key through an OpenAI-compatible provider for inference instead.

## Official Authentication Method

Managed by Gemini CLI: `~/.gemini/oauth_creds.json` (`access_token`, `expiry_date` in ms); `~/.gemini/settings.json` → `security.auth.selectedType`. Read-only; never refreshed by Loadout.

## Usage Source (`PRIVATE_ENDPOINT`)

1. `POST https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist` → `cloudaicompanionProject`, `currentTier` / `paidTier`.
2. `POST …/v1internal:retrieveUserQuota` `{"project": …}` → `buckets[]` (`remainingFraction`, `resetTime`, `modelId`). Loadout shows one daily window per model (the most constrained bucket).

## Terms-of-Service Risk

Medium — undocumented endpoints; read-only, every 10 minutes, only for imported accounts.

## Implementation References

- Gemini CLI (Apache-2.0) auth/config conventions.
- CodexBar (MIT) `GeminiStatusProbe.swift` — endpoints and fields. Interface knowledge only.
