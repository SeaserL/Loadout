# OpenAI API (`openai`)

- **Last Verified Date**: 2026-09-29
- **Module**: `crates/providers/src/official.rs`

## Official Authentication Method

API key; Bearer.

## Official API Availability

- `https://api.openai.com/v1`: Chat Completions and Responses (native state: `previous_response_id`, ADR-006 §3).
- Chat requests use `max_completion_tokens` (reasoning models reject `max_tokens`).
- `/models` lists all model types; Loadout keeps chat-capable families (`gpt-`, `o1/o3/o4`, `chatgpt-`, `codex-`) and drops embeddings, audio, image, realtime, moderation and search models.
- Usage: regular keys have no usage endpoint → estimated from Loadout's records.

## Subscription Transport Status

N/A. A ChatGPT subscription is a different product: see [codex.md](codex.md).

## Terms-of-Service Risk

Low.
