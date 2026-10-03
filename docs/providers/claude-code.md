# Claude Code / Claude subscription (`claude-code`)

- **Last Verified Date**: 2026-10-04
- **Module**: `crates/providers/src/claude_code.rs`

## Subscription Transport Status

**NOT SUPPORTED in v1** (ADR-009). Loadout never sends prompts with Claude subscription credentials.

## Discovery

Claude Code is found at any of:

- `claude` on PATH (npm install);
- `~/.local/bin/claude.exe` (native installer);
- the copy bundled with the **Claude desktop app**: `<desktop dir>\claude-code\<version>\<build>\claude.exe` (newest version wins).

**Desktop dir.** The desktop app is an MSIX package. Its `%APPDATA%\Claude` is redirected to `%LOCALAPPDATA%\Packages\Claude_<id>\LocalCache\Roaming\Claude`, and only processes inside the package see the redirected view. Loadout therefore checks both locations and uses the one whose usage history was written most recently.

Lesson (2026-10-04): a check run from a shell inside the Claude app saw the redirected path and passed, while the user's daemon did not.

Logged in means one of:

- the CLI credentials file exists (`auth = oauth`);
- the desktop app has recorded usage samples (`auth = desktop-app`).

Loadout never reads the desktop app's own login store.

## Official Authentication Method

Managed by Claude Code: `%CLAUDE_CONFIG_DIR%` or `~/.claude/.credentials.json` → `claudeAiOauth.{accessToken, expiresAt (ms), subscriptionType}`.

- Loadout reads the access token only when the `endpoint` usage source refreshes, and never stores or refreshes it.
- An expired token is reported as "run `claude` once".
- The desktop app keeps its login elsewhere, encrypted. Loadout does not touch it.

## Usage Sources (`usage_source` provider setting, ExecPlan 008)

Change it with `loadout provider set claude-code usage_source=<value>` or the "Usage source" selector on the Accounts page.

| Value | Source | Network | Token read | Trust |
|---|---|---|---|---|
| `auto` (default) | `endpoint` if the CLI credentials file exists, else / on failure `local` | maybe | maybe | — |
| `endpoint` | undocumented usage API | yes | yes | `PRIVATE_ENDPOINT` |
| `local` | desktop app usage samples | **no** | **no** | `LOCAL_FILES`, Estimated |
| `off` | not refreshed | no | no | — |

### `endpoint`

`GET https://api.anthropic.com/api/oauth/usage` with:

- headers `Authorization: Bearer <access token>` and `anthropic-beta: oauth-2025-04-20`;
- response fields `five_hour`, `seven_day`, `seven_day_opus`, `seven_day_sonnet`, each `{utilization (percent 0–100), resets_at (ISO 8601)}`.

Refreshed every 10 minutes with backoff. Loadout sends its own `User-Agent` (it does not impersonate Claude Code).

### `local`

The desktop app samples plan usage into `<desktop dir>\plan-usage-history.json`.

- Format: `{version: 2, samples: [{t: unix ms, org: <account uuid>, u: {fh: 5h %, sd: 7-day %}}]}`. Samples arrive about every 15 minutes while the app runs. There are no reset times.
- Loadout uses the newest sample.
- It drops a window once the sample is older than that window (5 h / 7 d), because the old percentage no longer says anything.
- `org` is never logged or shown.
- The format is undocumented and may change. A parse failure is reported as an error and does not cause any other access.

## Terms-of-Service Risk (re-checked 2026-10-04)

[Claude Code Legal and compliance](https://code.claude.com/docs/en/legal-and-compliance), section "Authentication and credential use":

- Third-party developers may not offer Claude.ai login or route requests through Free/Pro/Max credentials.
- They also may not "collect, store, or intermediate Claude.ai credentials or session tokens".

How each usage source fares against that:

- **`endpoint` — medium.** Reading Claude Code's token to call an undocumented endpoint is not inference. It is still close to the "collect / intermediate" wording, and the endpoint may change or be blocked.
- **`local` — low.** It reads only a file the official app writes for its own use. No token and no network.
- Users who want the lowest risk pin `local` or `off`.

## Implementation References

- CodexBar (MIT) `ClaudeOAuthUsageFetcher.swift` and `ClaudeOAuthCredentialModels.swift`: endpoint, headers and fields. Interface knowledge only.
- The desktop app's file format was observed on the developer's machine (2026-10-04). No third-party code was used.
