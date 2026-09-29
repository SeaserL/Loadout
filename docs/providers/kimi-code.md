# Kimi Code (`kimi-code`)

- **Last Verified Date**: 2026-09-29
- **Module**: `crates/providers/src/kimi_code.rs`

## Official Authentication Method

- **API key** created by Kimi members in the Kimi Code console (up to 5 keys), officially meant for third-party tools (Claude Code, OpenCode, Codex guides in kimi.com/code/docs).
- **Kimi Code CLI login** (`~/.kimi-code/credentials/kimi-code.json`): importable with `loadout scan import kimi-code` for **usage only**; Loadout reads the current access token per refresh and never refreshes or rewrites the file.

## Official API Availability

- International `https://api.kimi.ai/coding/v1`, China `https://api.kimi.com/coding/v1` (provider setting `region = "china"`). Both Chat (`/chat/completions`) and Messages (`/messages`).
- Models: `k3`, `k3-256k`, `kimi-for-coding`, `kimi-for-coding-highspeed` (documented list used when `/models` is unavailable).
- Usage: `GET {base}/usages` → `usage` (weekly quota: `limit`, `used`, `remaining`, `resetTime`) and `limits[]` (rolling window, e.g. 300 minutes).

## Subscription Transport Status

API key: allowed (tier A). CLI login: usage only.

## Terms-of-Service Risk

API key: low. CLI-login usage reading: low (read-only, low frequency).

## Implementation References

- Kimi Code docs (kimi.com/code/docs).
- CodexBar (MIT) `docs/kimi.md`, `KimiUsageFetcher.swift` — usage endpoint, response fields, CLI credential path. Interface knowledge only.
