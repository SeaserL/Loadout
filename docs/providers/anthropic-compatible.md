# Generic Anthropic-compatible (`anthropic-compatible`)

- **Module**: `crates/providers/src/anthropic_compatible.rs`

Any server exposing Anthropic `/messages` under a base URL you provide.

| Setting | Meaning |
|---|---|
| `auth_style` | `x-api-key` (default) or `bearer` |
| `anthropic_version` | default `2023-06-01` |
| `default_max_tokens` | used when a converted request has none (default 8192) |
| `allow_insecure_http` | as for openai-compatible |
