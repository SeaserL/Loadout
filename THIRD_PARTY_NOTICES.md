# Third-Party Notices

Every copied, ported or derived third-party component must be recorded here.
See [docs/legal/reference-map.md](docs/legal/reference-map.md) §11 for the workflow.

Entry template:

```text
Component:
Source:
Upstream path:
Upstream revision:
License:
Reuse: ORIGINAL | PORTED | DERIVED | COPIED
Local files:
```

## Components

No third-party source code has been copied or ported. The entries below record
interface knowledge (endpoint paths, header names, response field names, file
locations) learned from third-party projects and re-implemented as original code.

```text
Component: CodexBar provider usage research
Source: https://github.com/steipete/CodexBar
Upstream path: docs/providers.md, docs/{codex,claude,gemini,kimi,opencode,command-code}.md,
               Sources/CodexBarCore/Providers/** (usage fetchers, credential models)
Upstream revision: main as of 2026-09-29
License: MIT
Reuse: DERIVED (interface knowledge only; no code, comments or tests copied)
Local files: crates/providers/src/{codex,claude_code,gemini_cli,kimi_code,opencode_go}.rs
Notes: see docs/providers/<slug>.md "Implementation References"
```

```text
Component: OpenAI Codex app-server protocol
Source: https://github.com/openai/codex (app-server documentation)
Upstream revision: documentation as of 2026-09-29
License: Apache-2.0
Reuse: DERIVED (protocol method and field names only)
Local files: crates/providers/src/codex.rs
```
