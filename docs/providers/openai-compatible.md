# Generic OpenAI-compatible (`openai-compatible`)

- **Module**: `crates/providers/src/openai_compatible.rs`

Any server exposing `/chat/completions` (and optionally `/responses`) under a base URL you provide, e.g. vLLM, LM Studio, LiteLLM, SiliconFlow, a company gateway.

```text
loadout provider add my-llm --kind openai-compatible --base-url https://llm.example.com/v1
```

Settings (`--setting key=value`):

| Key | Meaning |
|---|---|
| `max_tokens_field` | `max_tokens` (default) or `max_completion_tokens` |
| `allow_insecure_http` | Permit plain http to a non-loopback host (or `--allow-insecure-http`) |

Synced models are assumed to speak Chat. If the server also implements `/responses`, add the model with both protocols: `loadout model add my-llm main <model> --protocol chat --protocol responses`.

Trust `OFFICIAL_API` (you vouch for the endpoint), `LOCAL` on loopback.
