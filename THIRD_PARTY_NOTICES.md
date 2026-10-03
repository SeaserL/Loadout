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

```text
Component: Codex subscription transport protocol knowledge (ExecPlan 010)
Source: https://github.com/router-for-me/CLIProxyAPI
Upstream path: sdk/auth/codex_device.go, internal/auth/codex/{openai_auth.go,jwt_parser.go},
               internal/runtime/executor/codex_executor_*.go, internal/runtime/executor/helps/codex_quota.go,
               internal/registry/models/codex_client_models.json
Upstream revision: d7914afdedca7af95ee974a42453dc49fc1388ce (2026-10-03)
License: MIT
Reuse: DERIVED (interface knowledge only: endpoints, field names, error types; no code copied)
Local files: crates/providers/src/codex_subscription.rs
```

```text
Component: EasyCLIProxyAPI desktop UI design
Source: https://github.com/router-for-me/EasyCLIProxyAPI
Upstream path: src/styles.css, src/theme.ts, src/App.tsx (design tokens, layout, theme switching);
               screenshots of the released app
Upstream revision: main as of 2026-10-04
License: MIT
Reuse: DERIVED (visual design only: colour and spacing values, sidebar / card / quota-bar
       layout patterns; no code, styles, logos or text copied)
Local files: apps/desktop/src/{styles.css,theme.ts,components.tsx,App.tsx,pages/*}
Notes: ExecPlan 009
```
