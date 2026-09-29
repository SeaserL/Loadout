# Ollama (`ollama`)

- **Last Verified Date**: 2026-09-29
- **Module**: `crates/providers/src/official.rs`

Local server, default `http://127.0.0.1:11434/v1` (OpenAI-compatible Chat and `/models`). No key needed (`loadout account add ollama local --no-key`). Trust `LOCAL` on loopback; a remote Ollama base URL is treated as `OFFICIAL_API`. Loopback requests bypass the system proxy.
