# Codex / ChatGPT subscription (`codex`)

- **Last Verified Date**: 2026-09-29
- **Module**: `crates/providers/src/codex.rs`

## Subscription Transport Status

**NOT SUPPORTED in v1** (ADR-009). Loadout never sends prompts with ChatGPT subscription credentials.

## Official Authentication Method

Managed by the Codex CLI (`$CODEX_HOME` or `~/.codex/auth.json`). Loadout never reads the tokens; discovery reads only `auth_mode`.

## Usage Source

1. **Official app-server** (`OFFICIAL_CLI`): `codex app-server`, JSON-RPC 2.0 without the `jsonrpc` field, newline-delimited over stdio — `initialize` → `initialized` notification → `account/read` → `account/rateLimits/read` (`rateLimits.primary/secondary`: `usedPercent`, `windowDurationMins`, `resetsAt` in Unix seconds). Started in the background every 10 minutes with `CREATE_NO_WINDOW`, killed after the call (Harness §2.4: never per request).
2. **Fallback** (`LOCAL_FILES`, confidence `ESTIMATED`): the last `rate_limits` in `$CODEX_HOME/sessions/**/rollout-*.jsonl` (`used_percent`, `window_minutes`, `resets_in_seconds` relative to the event time, or `resets_at`).

## Terms-of-Service Risk

Low — official CLI interface and local files, read-only.

## Implementation References

- OpenAI Codex (Apache-2.0) app-server docs (learn.chatgpt.com/docs/app-server).
- CodexBar (MIT) `docs/codex.md` — data-source overview. Interface knowledge only.
