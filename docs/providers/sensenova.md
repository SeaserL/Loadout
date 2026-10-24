# SenseNova 商汤日日新 (`sensenova`)

- **Last Verified Date**: 2026-10-24
- **Module**: `crates/providers/src/presets.rs` (row `sensenova`, ExecPlan 018)

## Official Authentication Method

API key from https://platform.sensenova.cn/console/keys (`sk-…`); `Authorization: Bearer <key>` on every endpoint. The legacy AK/SK → JWT scheme belongs to the old `api.sensenova.cn/v1/llm/*` API and does not apply.

## Official API Availability

- Chat: `POST https://token.sensenova.cn/v1/chat/completions` (`max_tokens`; `stream_options.include_usage` supported; `tools` / `tool_choice`).
- Messages: `POST https://token.sensenova.cn/v1/messages` (docs use `ANTHROPIC_AUTH_TOKEN`, i.e. Bearer; Loadout sends Bearer and `x-api-key`).
- Responses: `POST /v1/responses`, stateless only (`store` / `previous_response_id` unsupported). Not used by Loadout; Responses requests are converted to Chat.
- Catalog: `GET /v1/models`, OpenRouter-style entries (`context_length`, `output_modalities`, `pricing`). Image models (`sensenova-u1.5-*`, `output_modalities: ["image"]`) are dropped. Prices are not imported (currency undocumented).
- Fallback models: `sensenova-6.8-flash-lite`, `deepseek-v4-flash`, `deepseek-flash`, `glm-5.2`, `kimi-k3`.
- Usage: no API-key endpoint. Token Plan quota (5h / weekly points per pool) is console-only (SenseCore IAM login), so usage is estimated. Points page (checked 2026-10-24): a general pool and a Flash-Lite pool, each 60,000 points per rolling 5 hours and 600,000 per rolling week; the page notes that some models are under pressure at peak hours.
- Errors: documented `{"error":{"type","code","message"}}`; auth failures observed as 401 `{"error":{"code":16,"message":"Forbidden"}}`. 429 `quota_exceeded_error` covers both rate limits and points exhaustion (official error codes: "rate/quota exceeded, retry with exponential backoff"); there is no separate points-exhausted error, and errors inside a stream are not documented.

Rate limits (measured 2026-10-17 with `loadout bench`, three Token Plan accounts):

- The docs state no per-minute limit; only the points quota (5h / weekly). The community claim "about one call per minute" is not accurate.
- `deepseek-v4-flash`: three requests about 2 s apart succeeded; the next ones returned 429 `inference exceeds tpm/rpm limit` and recovered after about 20 s. A second burst of three on the same account later succeeded, so the window is short (seconds) and the limit is a few requests, not one per minute. Other accounts were unaffected at the same moment, so the limit is per key (or per key and model).
- `sensenova-6.8-flash-lite`: four back-to-back requests succeeded, but each took 16–27 s to the first token.
- Loadout classifies every 429 as `rate_limit` (20 s), never as a one-hour quota block: the `ambiguous_429` preset flag (ExecPlan 034). Before that, bodies that mentioned quota without "rate" locked a working key for an hour. Stream error events whose message contains `tpm/rpm limit`, or that look like quota, are rate limits too. A rate limit blocks that key and model on every protocol (chat and messages, ADR-003 §7), then the request fails over to the same model on another account (ADR-003 §8), so several keys absorb bursts.
- Peak-hour throttling (export of 2026-10-19 – 10-23 from a second device, 7–12 Token Plan keys, one sequential agent client, about 31k input tokens per request): first attempts were rate limited 0–7 % of the time between 02:00 and 08:00 (UTC+8) and 33–50 % between 09:00 and 01:00 with the same request pattern. Overflow keys were often rate limited after minutes without local traffic. The limit therefore depends on platform load, not only on one key's own TPM/RPM. Connectivity and a 24-route probe from another network were normal on 2026-10-24, so this is not an IP block.

Usage (checked 2026-10-17): not readable with an API key. The API host has no usage, quota or balance route (unknown paths answer 404 before authentication; only real routes such as `/v1/models` answer 401). The console's usage page (two point pools with 5-hour and weekly quotas, plus the per-model TPM/RPM table) reads them through the SenseCore IAM sign-in, not the key. Loadout shows usage estimated from its own request records.

Unified API (ADR-015): `loadout endpoint enable sensenova`, or Accounts → SenseNova → 总 API. Clients use `http://127.0.0.1:8317/e/sensenova/v1` (Anthropic SDKs: `http://127.0.0.1:8317/e/sensenova`) with a client key and bare model names such as `deepseek-v4-flash`. Each request tries every SenseNova account in the current pool once, in account order. A 429 cools that account and model down for 20 s. If every account is cooling down, the request waits up to 30 s for the first one to recover.

Rotation (ADR-015 §5, ExecPlan 035): `loadout endpoint rotate sensenova on`, or the "每次请求轮换账号" switch in the 总 API dialog. Each request then starts at the next account instead of the first healthy one, spreading traffic over every key; rate-limited accounts are still skipped. Off by default. Trade-offs: consecutive requests of one conversation land on different keys, so upstream prompt caching mostly stops (26 % of input tokens were cache hits in the 2026-10-23 export, with little TTFT difference), and the total traffic from one IP is unchanged.

Notes: Kimi K3 documents `max_completion_tokens` (Loadout renames `max_tokens` for `kimi-k3*` on Chat); the others use `max_tokens`. Per-model reasoning output field differs (`reasoning` vs `reasoning_content`).

## Subscription Transport Status

N/A (pay-as-you-go API key). Tier A. Plan-specific keys that need another host are not covered by the default base URL; use `--base-url`.

## Terms-of-Service Risk

Low (official API with the user's own key).

## Implementation References

- Official documentation only; no code copied.
- https://platform.sensenova.cn/docs (quickstart, auth, api-compatibility, list-models, errors, points)
