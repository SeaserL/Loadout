# Anthropic API (`anthropic`)

- **Last Verified Date**: 2026-09-29
- **Module**: `crates/providers/src/official.rs`

## Official Authentication Method

API key; `x-api-key` + `anthropic-version: 2023-06-01`.

## Official API Availability

- `https://api.anthropic.com/v1/messages`, `/v1/models`.
- Converted requests default to `max_tokens = 16384` when the client sent none.
- Usage: regular keys have no usage endpoint → estimated.

## Subscription Transport Status

N/A. A Claude subscription is a different product: see [claude-code.md](claude-code.md).

## Terms-of-Service Risk

Low.
