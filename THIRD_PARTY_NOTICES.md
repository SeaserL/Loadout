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

### Loadout 1.0 runtime

```text
Component: OmniRoute
Source: https://github.com/cheaper-inference/OmniRoute
Upstream revision: v3.8.52 / ad6f33f66bda34eba98645d3bfdabbde294bc843
License: MIT, Copyright (c) 2026 diegosouzapw (backend/LICENSE-OmniRoute.txt)
Reuse: DERIVED. The repository records a patch on that revision and does not carry the
       upstream source; a built package contains the compiled upstream with its LICENSE
       and THIRD_PARTY_NOTICES.md.
Local files: backend/omniroute-v3.8.52.patch. backend/src/** is original Loadout code
             (Apache-2.0) written against OmniRoute's internal modules.
```

### Fusion desktop redesign (ExecPlan 065, development)

Original code in `apps/fusion-console` visually references yetone/magpie at
`fd80ed99c9a97a2f6f96ee62e36eba9eefb16036` (MIT), specifically the local reference
screenshots `site/public/img/{masking,routing,usage}-zh-light.png`. No source,
logo, screenshot or artwork from Magpie is shipped in the product. The ribbon
chart is original code over OmniRoute's existing aggregated usage records.
The separated management pages consume the already credited OmniRoute APIs.
The preview shell reuses existing Tauri/WebView2 and Lucide dependencies;
it does not introduce Electron or another gateway. `@types/node` is an MIT
build-time-only typing dependency, absent from browser runtime assets.

The early Rust implementation entries immediately below record interface
knowledge (endpoint paths, header names, response field names, file locations)
and visual references re-implemented as original code, without copied source.
Later isolated fusion sections separately record actual OmniRoute source and
selected WorkBuddy modules used in the candidate distribution; the early
no-copy description does not apply to those later components.

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

## OpenCode official runtime (optional local deployment)

Component: @opencode/cli-windows-x64 2.0.32, MIT, Copyright (c) 2025 opencode.
Source: https://registry.npmjs.org/@opencode/cli-windows-x64/-/cli-windows-x64-2.0.32.tgz
Reuse: unmodified external executable installed locally; no OpenCode or 9router source copied into Loadout.
The optional runtime is not included in the Loadout source tree or standard release archive.
The local deployment retains OPENCODE-LICENSE.txt. Integrity and verification: docs/research/opencode-official-server-20261018.json.

## CLIProxyAPI isolated evaluation (ExecPlan 036)

Source: https://github.com/router-for-me/CLIProxyAPI
Upstream revision: 90c447d3a8727d115aa7552ec93ce023736ed492
License: MIT; root LICENSE inspected; no nested LICENSE/NOTICE found under sdk/ or internal/.
Paths used: sdk/translator/**, sdk/cliproxy/auth/**, sdk/cliproxy/executor/**,
sdk/config/**, internal/runtime/executor/openai_compat_executor.go and its dependencies.
Interface knowledge: sdk/api/handlers/openai/openai_handlers.go (SSE framing and terminal contract).
Reuse: unmodified external dependency in a temporary evaluation checkout only.
Local files: experiments/cliproxy-eval/** (original probes; no upstream implementation copied).
No CPA runtime, dependency or credential was added to the production Loadout build or release.
The temporary checkout retains the full upstream LICENSE and all original source headers.

## Product fusion source references (ExecPlan 038)

These are unmodified, local-only source checkouts, excluded from the parent Git index
and production build/package. No upstream implementation has been ported into Loadout
product modules in this task. Future selective ports must retain the relevant copyright
and license notices and add their actual destination files here.

Component: OmniRoute source reference
Source: https://github.com/cheaper-inference/OmniRoute
Revision: ad6f33f66bda34eba98645d3bfdabbde294bc843 (v3.8.52)
Local path: development/upstreams/omniroute/
Root license: MIT, Copyright (c) 2026 diegosouzapw.
Preserved files: LICENSE, THIRD_PARTY_NOTICES.md, nested licenses, original source headers.
Selected inspection: src/lib/db/providers.ts, src/lib/db/encryption.ts,
src/lib/db/apiKeys/secretAtRest.ts, src/sse/services/auth.ts,
src/sse/services/keyConnectionScope.ts, src/lib/radar/index.ts,
src/lib/usage/routeExplain.ts, src/lib/usage/usageHistory.ts,
src/sse/handlers/chat.ts, open-sse/handlers/chatCore.ts, electron/main.js.

Component: Magpie source reference
Source: https://github.com/yetone/magpie
Revision: fd80ed99c9a97a2f6f96ee62e36eba9eefb16036 (v0.1.2026)
Local path: development/upstreams/magpie/
Root license: MIT, Copyright (c) 2026 yetone.
Preserved files: LICENSE, THIRD_PARTY_NOTICES, internal/qoder/LICENSE, original source headers.
Selected inspection: internal/gui/assets/plugins/routing/client.js,
internal/gui/assets/plugins/routing/routing.css, internal/gateway/trace.go.

No nested license overriding the root MIT license was found on the selected source
paths. This is not a license audit of either entire dependency closure or brand assets.
Immutable source and license hashes are recorded in development/upstreams.lock.json.

## OmniRoute isolated adaptation experiment (ExecPlan 039)

Component: OmniRoute v3.8.52, revision ad6f33f66bda34eba98645d3bfdabbde294bc843.
Experiment patch: experiments/omniroute-phase1/omniroute-phase1.patch.
Modified upstream paths: src/app/api/v1/chat/completions/route.ts;
src/lib/db/providers.ts; src/lib/db/encryption.ts; src/lib/db/apiKeys/secretAtRest.ts;
src/sse/handlers/chat.ts; src/sse/services/auth.ts;
src/sse/services/credentialSelectionOptions.ts.
These selected files inherit the upstream MIT license; no overriding ancestor
license was found on these paths. Original headers and patch context are retained.
License copy: experiments/omniroute-phase1/notices/OmniRoute-LICENSE.txt.

Component: keytar 7.9.0 (upstream-declared optional dependency).
Source: https://github.com/atom/node-keytar
License: MIT, Copyright (c) 2013 GitHub Inc.
License copy: experiments/omniroute-phase1/notices/keytar-LICENSE.md.
The experiment uses its Windows N-API binding; native SHA-256 and npm package
provenance are recorded in experiments/omniroute-phase1/evidence/toolchain.json.

The experiment's original adapter/test files carry Apache-2.0 SPDX identifiers.
Upstream code and dependencies retain their own licenses. No experiment dependency
has been added to Loadout's production build or release. This notice is scoped to
the selected code and does not claim an audit of the entire upstream dependency tree.

## OmniRoute release and upgrade experiment (ExecPlan 040)

Pinned upstreams: v3.8.51 c1e30b7676975feb298b49eff6ff58923c04b89e and
v3.8.52 ad6f33f66bda34eba98645d3bfdabbde294bc843 (MIT).
Frozen patches: experiments/omniroute-phase2/evidence/v51.patch and v52.patch.
Additional inspected/modified paths: src/instrumentation-node.ts,
src/lib/db/apiKeys.ts, src/lib/db/providers/lazyConnectionView.ts.
The v51 experiment backports the exact v52 file
src/lib/db/providers/providerSpecificDataSecrets.ts (181 lines; root MIT license,
Copyright (c) 2026 diegosouzapw; no nested overriding license found).
The backported file retains its original header and the repository LICENSE.
The v52 package uses the upstream implementation directly.
Standalone lab packages preserve upstream LICENSE and THIRD_PARTY_NOTICES.md;
keytar retains its LICENSE.md. Bundled Node retains its upstream distribution
licensing; lab archives are validation artifacts and are not a public release.
No phase2 dependency is added to Loadout's production Cargo/npm manifests.

## OmniRoute native SQLite lifecycle experiment (ExecPlan 042)

The same MIT upstream revisions and keytar binding listed above are reused in
isolated phase3 worktrees and standalone validation directories. Frozen patches:
experiments/omniroute-phase3/evidence/v51.patch and v52.patch.
Additional modified upstream paths: src/lib/db/providers/deletion.ts;
src/app/api/v1/messages/route.ts; src/app/api/v1/responses/route.ts;
src/app/api/keys/[id]/reveal/route.ts;
src/app/api/settings/export-json/route.ts; src/app/api/settings/import-json/route.ts.
These paths retain the upstream MIT licensing; the v51 metadata-secret helper
backport remains the same 181-line v52 upstream file noted above.
Original src/lib/loadoutPhase1/* and src/app/api/loadout*/route.ts experiment
files carry Apache-2.0 SPDX headers. Standard Node crypto supplies scrypt and
AES-256-GCM; no third-party vault implementation was copied.
Standalone directories include OmniRoute LICENSE/THIRD_PARTY_NOTICES.md,
keytar LICENSE.md, Node-v24.19.0 licensing and the Loadout adapter license.
No phase3 dependency or code is added to the production Loadout runtime/build.
This remains a selected-file experiment notice, not a complete dependency audit.

### Fixed-baseline real-client trial (ExecPlan 043)

The phase4 standalone trial reuses the v3.8.52 MIT source, Node and keytar
dependencies and licensing recorded above. The only additional runtime change
is the original Loadout adapter's prefix/nodeName metadata allowlist. No new
upstream source is copied for protocol implementation. Phase4 lifecycle and
acceptance scripts are original Apache-2.0 code and reuse existing repository
bridge modules. Installed Codex, Claude Code and OpenCode executables are used
in isolated processes; they are not bundled or relicensed by this experiment.

## Unified isolated distribution (ExecPlan 050)

The portable headless candidate reuses the pinned OmniRoute v3.8.52 MIT source,
Node v24.19.0 distribution and keytar 7.9.0 listed below. It carries their license
files, upstream notices and this notice. The selected MIT WorkBuddy connector
bundle from AidenOrz/dsh-workbuddy-connect 25680ca7f7ff5d7b84bb35f45de766519fccbc9d
is shipped under tools/vendor with its LICENSE and THIRD_PARTY_NOTICES.txt.
The exact first-line literal derives from the already recorded OmniRoute rule.
Loadout-owned portable tooling remains Apache-2.0. No Magpie UI code is shipped.

## Narrow pacing and WorkBuddy connector trial (ExecPlan 046)

OmniRoute v3.8.52 remains the pinned MIT base. The isolated candidate modifies
only open-sse/services/rateLimitManager.ts beyond the frozen phase4 adapter,
and reuses its existing open-sse/services/slidingWindowLimiter.ts unchanged.
The source patch is experiments/omniroute-phase7/evidence/429-floor.patch.

The user-requested AidenOrz/dsh-workbuddy-connect reference is pinned at
25680ca7f7ff5d7b84bb35f45de766519fccbc9d (version 0.7.15, MIT,
Copyright (c) 2026 Corrine Hu). Selected functions are bundled for an isolated
test from src/upstream.ts (chatHeaders), src/client-identity.ts (chatUserAgent),
src/auth.ts (parseWorkBuddyAuth), src/desktop-credential-protection.ts,
src/variants.ts, src/status-paths.ts and src/wsl.ts. No overriding nested
license was found for these files. The reference checkout and generated lab
module retain LICENSE and THIRD_PARTY_NOTICES.txt. DSH host write/home functions
are replaced with failing lab guards; no DSH plugin registration, refresh store,
or prompt-fingerprint rewriting is used. No new dependency enters Loadout's
production build. This notice does not cover a future distributed connector.

ExecPlan 047 reuses the same pinned, unmodified WorkBuddy connector functions
through the phase7 lab bundle. Its new memory credential reader and thin relay
are original Apache-2.0 experiment code. No additional upstream protocol parser,
prompt rewriting, token-refresh implementation, or production dependency is copied.

ExecPlan 048 adds diagnostic-only lab bundles. The same pinned MIT WorkBuddy
reference additionally exports prepareChatBody/prepareInternationalChatBody;
its built-in preparation is compared in memory, with external rules disabled.
Pinned OmniRoute ad6f33f66bda34eba98645d3bfdabbde294bc843 additionally supplies
open-sse/config/providers/registry/workbuddy/index.ts and the replaceBodyStrings
helper from open-sse/utils/strictChatHistory.ts. The generated diagnostic bundle
retains OmniRoute's MIT LICENSE (Copyright (c) 2026 diegosouzapw) and disables
registry runtime access. These diagnostic comparisons do not install prompt
rewriting in the relay, candidate backend, or Loadout production build.

ExecPlan 049 implements an explicitly enabled exact-first-line compatibility
function in the isolated WorkBuddy relay. The replacement literal is the
verified output of the pinned OmniRoute WorkBuddy registry rules cited above;
an offline test compares it to those unmodified MIT rules. The new boundary
function and test harness are original Apache-2.0 code. No recursive upstream
rewriting helper or new dependency is loaded by the relay, and no production
build is changed. Generated reference bundles retain their original notices.
# Fusion 053 implementation provenance

The subscription scan and controlled-login adaptation is a behavior port of Loadout's
Apache-2.0 sources `crates/providers/src/codex.rs`,
`crates/providers/src/codex_subscription.rs`, and `crates/core/src/oauth.rs`.
The Node/TypeScript runtime binding uses OmniRoute v3.8.52's existing provider,
Responses executor, model-combo mapping, and SQLite/vault integration.
No additional external OAuth implementation or client-token import was copied.
Existing upstream notices and the WorkBuddy module attribution remain applicable.

### 054 native read-only management
OmniRoute v3.8.52 (MIT, ad6f33f66bda34eba98645d3bfdabbde294bc843): directly enable GET handlers at src/app/api/models, provider-models, usage/history, usage/model-latency-stats, usage/call-logs, usage/route-explain/[id], usage/combo-trace/[id]. Reuse existing usage/DB and combo trace implementations; no copied second statistics store or UI. Native license/notice remain bundled. Loadout only narrows the existing proxy allowlist and documents data semantics.

### OmniRoute native free catalog and Radar (055)

The isolated candidate uses the existing MIT-licensed OmniRoute v3.8.52 implementations at `open-sse/config/freeModelCatalog{,.data}.ts`, `src/app/api/free-models/route.ts`, `src/app/api/radar/{catalog,status,settings,sync}/route.ts`, `src/app/api/radar/syncRequest.ts`, `src/lib/radar/`, and `src/lib/db/radar.ts`. Upstream copyright: Copyright (c) 2026 diegosouzapw. The root MIT license is retained in the distribution. No independent catalog, signature implementation, or feed scheduler was copied into Loadout. Validation reads the upstream MIT fixture `tests/fixtures/radar-feed-canonical.json` and runs existing Radar unit tests with isolated data and synthetic signing keys.

### OmniRoute native pricing (056)

Fixed v3.8.52, MIT, Copyright (c) 2026 diegosouzapw. Reuses `src/app/api/pricing/`, `src/lib/pricingSync.ts`, `src/lib/db/settings/pricing.ts`, `src/shared/constants/pricing/`, and `src/lib/usage/costCalculator.ts` in the existing backend. Original copyright and license remain bundled. No separate price dataset is copied or maintained by Loadout; validation uses synthetic public metadata and isolated mock traffic. External LiteLLM fetching remains upstream behavior, not a claim of independently verified vendor prices.


## OmniRoute native quota, budget and model discovery (ExecPlan 057)

Same MIT source and pinned revision as ExecPlan 056. Reuses existing bundled
`src/app/api/usage/{quota,provider-limits,budget}/route.ts`,
`src/app/api/keys/[id]/usage-limits/route.ts`,
`src/app/api/providers/[id]/{models,sync-models}/route.ts`,
`src/app/api/synced-available-models/route.ts`, native `domain/costRules`,
`lib/usage/apiKeyUsageLimits` and `lib/providerModels/managedModelImport`.
No additional pricing, quota or model database implementation is copied.
Distribution changes only expose bounded native management routes and retain
existing vault and account policy adapters. Original OmniRoute MIT notices remain.

## Release consolidation (ExecPlan 058)

## Fusion console (ExecPlan 064, development)

The original React console in `apps/fusion-console` takes visual inspiration
from Magpie's top navigation, grouped usage presentation and routing flows.
No Magpie source, logos, artwork, Go runtime or plugin kernel is copied.
The native OmniRoute v3.8.52 MIT function
`src/lib/radar/autoSync.ts:shouldAutoSyncOnOpen` is imported directly for cache
freshness display (Copyright (c) 2026 diegosouzapw); its original MIT license
must accompany the console bundle. Native management APIs remain the data source.
React/React DOM use the MIT License (Meta Platforms, Inc. and affiliates),
and lucide-react uses ISC (2026 Lucide Icons and Contributors). Distribution
must include these complete license texts alongside static assets.

### Historical 058 details

No new upstream runtime code is introduced. The active selected WorkBuddy bundle
is now `fusion/vendor/connector.mjs`, accompanied by its unchanged MIT LICENSE
and THIRD_PARTY_NOTICES.txt. Original provenance remains ExecPlan 046 above.
The pinned Node v24.22.1 license is at
`development/fusion/notices/Node-v24.22.1-LICENSE.txt`; build dependency locks are
at `development/fusion/tools/`. Active build/acceptance tools are under
`scripts/fusion/` and `tests/fusion/`. The sanitized legacy pool fixture is an
existing Loadout metadata projection, not a credential database or evidence of
current account availability. No Magpie UI code or new external library is added.

Correction: the 057 manifest already identified Node v24.22.1, despite older
24.19.0 prose. 058 reuses that binary and ships the matching official v24.22.1
LICENSE, with URL and SHA-256 recorded alongside the build notice.
