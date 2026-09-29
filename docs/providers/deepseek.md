# DeepSeek (`deepseek`)

- **Last Verified Date**: 2026-09-29
- **Module**: `crates/providers/src/deepseek.rs`

## Official Authentication Method

API key; Bearer for Chat, `x-api-key` for the Anthropic-compatible endpoint.

## Official API Availability

- Chat: `https://api.deepseek.com/v1/chat/completions`.
- Messages: `https://api.deepseek.com/anthropic/v1/messages`.
- Balance: `GET https://api.deepseek.com/user/balance` → `balance_infos[]` (`currency`, `total_balance`, `granted_balance`, `topped_up_balance`).

## Subscription Transport Status

N/A. Tier A.

## Terms-of-Service Risk

Low.
