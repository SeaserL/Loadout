# Loadout Open-Source License & Reference Map v0.1

> **Loadout — All your AI, one endpoint.**

## 0. Purpose

This document has two purposes:

1. Define the recommended open-source licensing strategy for Loadout.
2. Give coding agents a compact lookup table for existing projects so they can jump directly to relevant prior art instead of repeatedly researching the same ecosystem.

This is general software-licensing guidance, not legal advice.

---

# 1. Recommended License

## Decision

**Recommended project license: Apache License 2.0 (`Apache-2.0`).**

Use Apache-2.0 for all original Loadout code unless a file or vendored component is explicitly marked otherwise.

### Why Apache-2.0 fits Loadout

Loadout is infrastructure software:

- local AI gateway;
- provider integrations;
- authentication adapters;
- routing engine;
- desktop control plane;
- protocol translation;
- developer-facing APIs.

Apache-2.0 gives Loadout most of the openness of MIT while adding an explicit patent grant from contributors and a patent-termination provision.

It also permits:

- private use;
- commercial use;
- modification;
- redistribution;
- proprietary downstream products.

This keeps adoption friction low if Loadout later becomes useful to:

- individual users;
- open-source projects;
- companies;
- IDE/agent vendors;
- hosted products.

At the same time, Loadout can freely depend on or incorporate appropriately attributed MIT-licensed work.

---

# 2. Why Not the Main Alternatives?

| License | Fit for Loadout | Reason |
|---|---:|---|
| **Apache-2.0** | **Recommended** | Permissive, explicit patent grant, good for infrastructure and external contributors |
| MIT | Good | Extremely simple and compatible with most references, but lacks Apache-2.0's explicit patent grant |
| MPL-2.0 | Possible | File-level copyleft is useful if we specifically want modifications to Loadout files to stay open |
| GPL-3.0 | Not preferred | Strong copyleft creates more integration friction than this project currently needs |
| AGPL-3.0 | Not preferred | Extends source-availability obligations to remote network use; unnecessary for a local-first personal gateway |
| Proprietary / source-available | Not needed | Conflicts with the current goal of easy collaboration and reuse |

If the project philosophy later changes to:

> "Anyone can use Loadout, but modifications to Loadout itself should remain open, even inside larger proprietary products."

then **MPL-2.0** is the first alternative worth reconsidering.

If the philosophy changes to:

> "Hosted forks must publish their modifications."

then AGPL-3.0 can be reconsidered.

For the current project, neither restriction appears necessary.

---

# 3. Required Repository Licensing Files

The repository should contain:

```text
LICENSE
NOTICE
THIRD_PARTY_NOTICES.md
docs/legal/reference-map.md
docs/legal/source-reuse-policy.md
```

Recommended:

```text
LICENSE
→ canonical Apache License 2.0 text

NOTICE
→ Loadout copyright and required Apache NOTICE entries

THIRD_PARTY_NOTICES.md
→ every copied/derived third-party component and its license

docs/legal/reference-map.md
→ this document

docs/legal/source-reuse-policy.md
→ operational rules for AI agents and human contributors
```

Recommended SPDX identifier for original source files:

```text
SPDX-License-Identifier: Apache-2.0
```

---

# 4. Contribution Policy

For the first public version:

**Use Apache-2.0 + Developer Certificate of Origin (DCO), without a CLA.**

Reasons:

- low contribution friction;
- contributors keep their copyright;
- Apache-2.0 already defines contribution licensing behavior;
- DCO gives a lightweight record that contributors have the right to submit their work.

A CLA should only be introduced later if Loadout explicitly plans:

- commercial dual licensing;
- relicensing;
- copyright consolidation;
- enterprise licensing that requires rights beyond Apache-2.0.

Do not add a CLA pre-emptively.

---

# 5. Third-Party Code Reuse Rules

## 5.1 MIT

**Status: GREEN**

Code may generally be reused, modified, ported, and redistributed.

Requirements:

- preserve copyright notice;
- preserve MIT license text for copied/substantial portions;
- record source and provenance in `THIRD_PARTY_NOTICES.md`.

Loadout may remain Apache-2.0 overall while retaining MIT notices for imported portions.

---

## 5.2 Apache-2.0

**Status: GREEN**

Code is highly compatible with Loadout's proposed license.

Requirements include:

- preserve required notices;
- mark significant modifications where required;
- preserve any upstream `NOTICE` obligations;
- track provenance.

---

## 5.3 MPL-2.0

**Status: YELLOW**

Can coexist in a larger work, but MPL-covered files and modifications to those files remain under MPL.

Default Loadout policy:

> Do not directly copy MPL files unless there is a clear reason to keep the imported component isolated and MPL-licensed.

Architecture and behavior may still be studied.

---

## 5.4 GPL / AGPL

**Status: RED for direct copying**

Default policy:

> Do not copy GPL/AGPL source code into Loadout.

Agent may:

- study public behavior;
- study API shape;
- study architecture at a high level;
- independently implement equivalent functionality.

Agent must not:

- copy implementation code;
- translate code line-by-line into Rust/TypeScript;
- mechanically rewrite GPL/AGPL code;
- import GPL/AGPL files into the Loadout source tree.

If an AGPL/GPL component is ever proposed as an external process or dependency, perform a separate license review.

---

## 5.5 No License / Custom License / Unknown License

**Status: RED**

Do not copy code until the license is explicitly verified.

---

# 6. Separate "Code License" from "Provider Terms of Service"

Every Provider integration needs two independent checks:

```text
A. Can Loadout legally reuse this project's source code?
B. Can Loadout use the provider's authentication/service in the proposed way?
```

These are different questions.

Example:

```text
Gemini CLI source code
→ Apache-2.0
→ reusable under license conditions

Gemini CLI OAuth / backend service
→ governed by Google service terms
→ third-party routing may have additional restrictions
```

Therefore every Provider should have:

```text
docs/providers/<provider>.md
```

with:

```text
Implementation References
Source Licenses
Official Authentication Method
Official API Availability
Subscription Transport Status
Terms-of-Service Risk
Last Verified Date
```

---

# 7. Reference Priority System

Coding agents should use these labels.

## A — Primary Implementation Reference

Directly relevant and permissively licensed.

Agent may inspect targeted source files and port/reuse code when worthwhile, with attribution.

## B — Architecture / Secondary Reference

Useful, but different language/architecture or overlapping with a better primary source.

Prefer concepts over copying.

## C — Concept Only

License or architecture makes direct reuse undesirable.

Do not copy code.

---

# 8. Known Project → Borrowable Feature Map

## Master Table

OpenCode 官方 v2 API / Windows runtime：本次 OpenCode Free bridge 的 primary reference 为官方 `https://opencode.ai/v2/docs/api`，发行包 `@opencode/cli-windows-x64@2.0.32`；只调用原生服务，未复制源码。v1 与 v2 安装渠道独立，不以 opencode-ai/latest 判断 v2 版本。9router 的身份/工具指纹改写不采用。验证见 ExecPlan 026、ADR-017。

| Project | License | Priority | Loadout feature to study | Agent should look for | Reuse mode |
|---|---|---:|---|---|---|
| **CLIProxyAPI** | MIT | **A** | Subscription/OAuth → unified API; multi-account providers; protocol translation; SSE; provider executors | `auths/`, `internal/`, `sdk/`, provider executors/translators, Responses/Claude/Gemini conversion | **Direct reference / port with notice** |
| **EasyCLIProxyAPI** | MIT | **A** | Tauri desktop lifecycle; OAuth UI; provider management; local core process management; installed-agent detection/configuration; tray behavior | `src-tauri/`, `src/`, `tests/`, agent config flows, OAuth screens, quota UI | **Direct reference / very high relevance** |
| **CodexBar** | MIT | **A** | Account discovery; usage/quota collection; local CLI credentials; browser/API/local fallback hierarchy; spend history | `docs/providers.md`, provider-specific docs, quota parsers, local usage scanners | **Reference algorithms/data sources; port selectively** |
| **Claude Code Router (CCR)** | MIT | **A/B** | Local control plane; agent profiles; provider presets; credential pools; condition routing; rewrites; retries; ordered fallbacks; local-login import | provider/config/routing code, desktop workflow, account import, logs | **Direct reference, but avoid duplicating its full product scope** |
| **LiteLLM OSS** | MIT outside restricted enterprise areas | **A/B** | Canonical API abstraction; provider normalization; routing; retry/fallback; virtual keys; spend tracking; model pricing database | gateway/router code, provider adapters, `model_prices_and_context_window.json`, endpoint support metadata | **Use OSS code only; never assume `enterprise/` is MIT** |
| **CliRelay** | MIT | **B** | Channel groups; path routing; failover; request logging; per-key quotas; health score; latency tracking; provider-channel management | `auths/`, `internal/`, `sdk/`, `sdkbridge/`, routing-group/channel-health code | **Study management enhancements; proxy core should usually be traced upstream to CLIProxyAPI** |
| **ccusage** (moved to ccusage/ccusage, now Rust) | MIT for `apps/ccusage` (GitHub reports "other"; check per path) | **B** | Local agent transcript parsing; token aggregation; API-equivalent cost calculation; cache/reasoning token accounting | local JSONL parsers, pricing snapshot/update logic, dedup strategy | **Direct reference for offline usage analytics** |
| **Codex Switch** | Apache-2.0 | **B** | Multiple Codex account management; auth profile switching; quota refresh; Tauri credential boundary | Tauri backend, account store, quota refresh, auth import/export safeguards | **Directly compatible; useful for Codex-specific implementation** |
| **OpenAI Codex** | Apache-2.0 | **A for Codex-specific work** | Official Codex configuration, app-server/RPC, authentication, local state conventions | official `codex-rs` code/docs; app-server/account behavior | **Prefer over community reverse engineering whenever possible** |
| **Gemini CLI** | Apache-2.0 | **A for Gemini implementation / C for unsupported service use** | Official config/auth storage and CLI architecture | `packages/core`, `packages/cli`, official auth/config code | **Code reusable; service/OAuth use must separately obey Google terms** |
| **New API** | AGPL-3.0 + additional attribution terms | **C** | Model/channel UX; weights/priorities; quota/cost management; management console data model; plugin concepts | `relay/`, `router/`, `oauth/`, `plugins/`, `model/`, `service/` | **Concept only — do not copy implementation into Apache Loadout** |
| **Cli-Proxy-API-Management-Center** | MIT | **B** | Official CLIProxyAPI web panel: Dashboard / Config / Providers / Auth files / Quota / Logs IA | `src/pages`, quota cards, YAML diff-before-save | **Reference UX; port small components with notice** |
| **Quotio Desktop** (xiaocoss/quotio-desktop) | MIT | **B** | Quota cards: per-window bars, % remaining colour bands, reset countdown, health dot strip | `assets/*.png`, quota components | **Reference UX** |
| **cc-switch** (farion1231/cc-switch) | MIT (repo, 2026-10-04; check file headers) | **A** for client config writing | Client config writers with backup, owned-key floor, atomic `ReplaceFileW`, `ccswitch://` deep links, models.dev prices, usage templates | `src-tauri/src/config.rs`, `live/engine.rs`, `live/patch/*`, `live/floor.rs`, `deeplink/*` | **Port with notice** |
| **all-api-hub** | AGPL-3.0 | **C** | Browser extension: relay-site balances, check-ins, price comparison, export to clients | — | **Concept only** |
| **sub2api** | LGPL-3.0 | **C** | Operator-side subscription pooling and resale | — | **Out of scope (multi-tenant resale); concept only** |
| **OmniRoute** (diegosouzapw/OmniRoute) | MIT | **B** | Headroom / reset-aware / cache-affinity routing strategies; per-key cooldown and per-model lockout; decision header; webhooks | routing strategies, failure layers | **Concept first; port small parts with notice. Never its TLS stealth (ADR-012)** |
| **Claude-Code-Usage-Monitor** | MIT | **B** | Official rate limits via Claude Code statusline JSON; source labels; burn-rate forecasts | statusline capture, forecast | **Reference** |
| **models.dev** (anomalyco/models.dev) | MIT | **A** for price data | Price / limits / modality catalog with `canonical_model_id` | `api.json` | **Bundle a filtered snapshot with notice** |
| **pydantic genai-prices** | MIT | **B** | Provider `api_pattern` and model match rules; historic prices | `data.json` | **Reimplement matcher in Rust** |
| Others (2026-10-04 survey) | see docs/research/2026-10-04-feature-landscape.md §7 | **B/C** | 9router, ccs, claude-code-hub, bifrost, uni-api, axonhub, ccstatusline, tokscale, cc-switch-cli, api-key-tester | — | Check licence per path before porting |
| **chromedp** (chromedp/chromedp) | MIT (LICENSE verified 2026-10-15) | **B** for web-session transport | Windows CDP pipe launch (`--remote-debugging-io-pipes`, inherit only two handles) | `pipe_windows.go` | **Concept; reimplement in Rust (ExecPlan 017 D1)** |
| **devtools-protocol** (ChromeDevTools/devtools-protocol) | BSD-3-Clause | **A** for CDP | Authoritative CDP method / event definitions | `pdl/domains/*.pdl` (e.g. `Network.streamResourceContent`) | **Protocol spec; hand-written serde types** |
| Web-UI → API projects (2026-10-15) | MIT / AGPL-3.0 / GPL-3.0, see §10.12 | **C** | PhantomAPI, mse_ai_api, unofficial-chatgpt-api, AIstudioProxyAPI, WebAI2API, gpt4free, chat2api | — | **Concept only; their stealth / token-forging / cookie-reuse techniques are banned by ADR-014 §2** |
| **Antigravity Manager** (lbjlaq; Draculabo) | lbjlaq: verify; Draculabo: CC BY-NC-SA 4.0 | **C** | Account rotation on quota threshold; tray; local proxy | — | **Do not copy (non-commercial licence); web-session / OAuth piggybacking is banned by ADR-009/012** |

---

# 9. Feature → First Repository to Read

This is the fast lookup table agents should use before searching the web.

| Loadout task | Read first | Read second only if needed |
|---|---|---|
| OAuth/subscription account converted into compatible API | **CLIProxyAPI** | CliRelay |
| Kimi OAuth / subscription transport | **CLIProxyAPI** | CCR |
| Claude Code OAuth/subscription transport | **CLIProxyAPI** | CCR |
| Codex OAuth/subscription transport | **OpenAI Codex + CLIProxyAPI** | Codex Switch |
| Gemini authentication/config | **Gemini CLI official repo** | CodexBar for quota discovery |
| Unified protocol conversion | **CLIProxyAPI** | LiteLLM |
| OpenAI ↔ Anthropic ↔ Gemini canonical request model | **LiteLLM + CLIProxyAPI** | New API conceptually |
| Provider Adapter / Driver abstraction | **LiteLLM** | CodexBar |
| Usage/quota provider discovery | **CodexBar** | ccusage |
| OpenCode Go usage windows | **CodexBar** | provider official docs |
| Kimi usage windows | **CodexBar** | provider official docs |
| Command Code usage/billing discovery | **CodexBar** | provider official docs |
| Local JSONL/session usage parsing | **ccusage** | CodexBar |
| API-equivalent cost calculation | **ccusage / LiteLLM pricing data** | CodexBar |
| Desktop Tauri architecture | **EasyCLIProxyAPI** | Codex Switch |
| Agent installation/config scanning | **EasyCLIProxyAPI** | CCR |
| Tray + local daemon UX | **EasyCLIProxyAPI** | CCR |
| Route rules / retries / fallbacks | **CCR** | LiteLLM |
| Multi-account/key pool scheduling | **CLIProxyAPI** | CliRelay |
| Channel health / latency | **CliRelay** | LiteLLM |
| Route profile UX | **CCR** | New API conceptually |
| Client API keys / quotas | **LiteLLM** | CliRelay |
| Request logging / observability | **CliRelay** | LiteLLM |
| Complex admin-console patterns | **New API, concept only** | CliRelay |
| Codex multi-account UX | **Codex Switch** | EasyCLIProxyAPI |
| Provider pricing/model metadata | **models.dev `api.json` (MIT)** | LiteLLM JSON (MIT); pydantic genai-prices for matching rules |
| One-click client configuration (Claude Code / Codex / OpenCode) | **cc-switch (MIT)** | EasyCLIProxyAPI `agents/*` (MIT) |
| Claude Code statusline / official rate-limit capture | **Claude-Code-Usage-Monitor (MIT)** | ccstatusline (MIT) |
| Quota-aware / reset-aware / cache-affinity routing | **OmniRoute (MIT, concept)** | claude-code-hub (MIT) |
| Key validity / stream health checks | **cc-switch-cli (MIT)** | api-key-tester (MIT) |
| Price comparison UX (normalization, "never treat missing as 0") | **all-api-hub (AGPL, concept only)** | octopus (AGPL, concept only) |
| Web session transport (ADR-014): CDP launch / pipe / network capture | **devtools-protocol (BSD-3) + chromedp `pipe_windows.go` (MIT, concept)** | Playwright / Puppeteer (Apache-2.0) Chromium launchers; §10.12 projects for site behaviour only |

---

# 10. Project Notes

## 10.1 CLIProxyAPI

Repository:

```text
https://github.com/router-for-me/CLIProxyAPI
```

License:

```text
MIT
```

Why it matters:

This is currently the highest-value reference for the **Transport layer** of Loadout.

Relevant capabilities include:

- OpenAI-compatible API;
- Anthropic-compatible API;
- Gemini-compatible API;
- Codex/Responses compatibility;
- multiple CLI accounts;
- OAuth-backed providers;
- Kimi;
- Codex;
- Claude Code;
- Gemini;
- streaming;
- protocol bridging.

### Agent rule

When implementing:

```text
Transport Driver
OAuth Provider
Protocol Translator
Subscription Bridge
```

look here first.

**Scope limits (ADR-012, 2026-10-04).** Codex subscription transport may port the following:

- OAuth (device code / PKCE) and refresh;
- the Responses body normalization;
- error classification and quota-header parsing.

Never port any of these:

- the Claude "cloak" (system-prompt replacement, billing header, request signing, fake device ids, tool renaming);
- uTLS / TLS fingerprint spoofing;
- the Claude Code header impersonation profile.

Reviewed revision: `d7914afdedca7af95ee974a42453dc49fc1388ce` (2026-10-03).

Do **not** begin by reading CliRelay's entire repository; CliRelay is an enhanced fork of this project.

---

# 10.2 EasyCLIProxyAPI

Repository:

```text
https://github.com/router-for-me/EasyCLIProxyAPI
```

License:

```text
MIT
```

Tech:

```text
Tauri + React + Rust
```

This makes it unusually relevant to Loadout because the planned desktop stack is similar.

Study:

- process lifecycle;
- local core install/start/stop/update;
- OAuth authorization UI;
- auth file handling;
- provider workspace;
- quota display;
- installed agent detection;
- automatic agent configuration;
- config backup/restore;
- tray/menu-bar behavior.

### Agent rule

For desktop implementation, **read this before inventing a Tauri lifecycle design from scratch**.

---

# 10.3 CodexBar

Repository:

```text
https://github.com/steipete/CodexBar
```

License:

```text
MIT
```

Most important document:

```text
docs/providers.md
```

CodexBar currently documents dozens of provider IDs and multiple fetch strategies:

```text
OAuth
CLI
API
local files
browser sessions
web dashboards
```

This is the best known reference for Loadout's:

```text
Discovery Driver
Usage Driver
quota window normalization
usage fallback hierarchy
local usage scan
```

Particularly useful providers:

```text
Codex
Claude
Gemini
OpenCode
OpenCode Go
Kimi
Command Code
OpenRouter
DeepSeek
```

### Agent rule

Before reverse-engineering a usage endpoint, search CodexBar's provider documentation first.

---

# 10.4 Claude Code Router (CCR)

Repository:

```text
https://github.com/musistudio/claude-code-router
```

License:

```text
MIT
```

Study:

- one local endpoint for many coding agents;
- agent profiles;
- provider presets;
- local login import;
- credential pools;
- routing conditions;
- request rewrites;
- ordered fallbacks;
- logs showing resolved provider/model/account;
- desktop management flow.

CCR overlaps with the broad Loadout product vision more than most projects.

### Agent rule

Use CCR primarily for:

```text
control-plane UX
routing configuration UX
agent profiles
credential pools
```

Do not automatically copy its architectural boundaries because Loadout has stronger requirements around:

```text
subscription inventory
quota-aware scheduling
latency snapshot routing
resource-pool semantics
```

---

# 10.5 LiteLLM

Repository:

```text
https://github.com/BerriAI/litellm
```

License:

```text
MIT for normal OSS portions
separate restricted license for enterprise areas
```

Useful references:

- provider abstraction;
- OpenAI-compatible normalization;
- retry/fallback;
- load balancing;
- virtual keys;
- spend tracking;
- model metadata;
- context windows;
- model price database.

Important data file:

```text
model_prices_and_context_window.json
```

### Critical rule

Do not assume every file in the repository is MIT.

The repository explicitly separates restricted enterprise code.

Agent must verify the exact path/license before copying source.

---

# 10.6 CliRelay

Repository:

```text
https://github.com/kittors/CliRelay
```

License:

```text
MIT
```

CliRelay describes itself as a heavily enhanced fork of CLIProxyAPI.

Therefore use:

```text
CLIProxyAPI
→ proxy/auth/translation core reference

CliRelay
→ management/routing/operations enhancements
```

Study CliRelay for:

- routing groups;
- group/path routing;
- failover;
- account/channel management;
- per-key quotas;
- request logs;
- health scores;
- latency tracking;
- model exclusions;
- config snapshots;
- runtime monitoring.

### Agent rule

When identical code exists upstream, cite/derive from CLIProxyAPI rather than treating the fork as an independent origin.

---

# 10.7 ccusage

Repository:

```text
https://github.com/ccusage/ccusage
```

License:

```text
MIT
```

Study:

- Claude/Codex/OpenCode/Kimi/etc. local usage parsing;
- JSONL scanning;
- deduplication;
- session/day/month aggregation;
- input/output/cache/reasoning token breakdown;
- API-equivalent pricing;
- offline pricing snapshots.

Useful for:

```text
Usage Driver
Subscription Value Captured
local-only analytics
```

---

# 10.8 Codex Switch

Repository:

```text
https://github.com/piperhex/codex-switch
```

License:

```text
Apache-2.0
```

Study:

- multiple Codex accounts;
- `auth.json` profile management;
- real-time quota refresh;
- account switching;
- Tauri credential boundary;
- local-only proxy lifecycle.

Useful as a narrow Codex-specific reference rather than a general router reference.

---

# 10.9 OpenAI Codex

Repository:

```text
https://github.com/openai/codex
```

License:

```text
Apache-2.0
```

Use this as the primary authority for:

- Codex local file conventions;
- official app-server behavior;
- configuration;
- authentication architecture;
- Rust implementation patterns relevant to Codex.

### Agent rule

Official source beats community reverse engineering.

---

# 10.10 Gemini CLI

Repository:

```text
https://github.com/google-gemini/gemini-cli
```

License:

```text
Apache-2.0
```

Study:

- auth configuration;
- local credential handling;
- CLI architecture;
- policy/config design.

### Important

The software license does not automatically authorize using Gemini CLI's underlying service through third-party software.

Provider implementation must separately verify current Google terms.

---

# 10.11 New API

Repository:

```text
https://github.com/QuantumNous/new-api
```

License:

```text
AGPL-3.0
+ additional attribution requirements
```

Study only at the product/architecture level:

- channel model;
- priorities/weights;
- account management UX;
- quota/cost UI;
- provider catalog;
- plugin model;
- management console organization.

### Agent rule

**Do not copy New API implementation code into Loadout.**

Do not mechanically translate its Go/React code into Rust/TypeScript.

Use it only to understand product patterns and public behavior unless a future licensing decision explicitly changes.

---

# 10.12 Web-UI → API projects (ADR-014)

Surveyed for ADR-014 (2026-10-15). Licenses below were verified on 2026-10-15 by reading each repository's LICENSE file at the listed commit (ADR-014's table said "not verified").

| Project | Repository | License (file) | Commit checked | Approach | Notes |
|---|---|---|---|---|---|
| PhantomAPI | `mrshibly/PhantomAPI` | MIT (`LICENSE`) | `a48ba0d` | FastAPI + headless Playwright on free ChatGPT web; OpenAI-compatible `/v1`; prompt-simulated tool calling | Small (8★) |
| mse_ai_api | origin unclear — earliest found `shakahl/mse_ai_api` (created 2026-03-19) | MIT (`LICENSE`, "Copyright (c) 2026 Mohamed Elsayed") | `a08c6f3` | Playwright driving a standalone Chrome; prompt-forced JSON for function calling | Several same-named copies with MIT, MIT-0 or **no** license (e.g. `DjamelRebiai/mse_ai_api`, `alomda552005-lgtm/mse-ai-api`). Treat any copy without a LICENSE as "unknown: no reuse" |
| unofficial-chatgpt-api | `taranjeet/unofficial-chatgpt-api` | MIT (`License`) | `371abb7` | Opens Chromium, parses HTML for the answer | Unmaintained (last push 2023-07) |
| AIstudioProxyAPI | `CJackHwang/AIstudioProxyAPI` | **AGPL-3.0** (`LICENSE`) | `044c3db` | Playwright + Camoufox on Google AI Studio; fingerprint spoofing | Concept only (AGPL); Camoufox banned by ADR-014 §2 |
| WebAI2API | `foxhui/WebAI2API` | MIT (`LICENSE`) | `beac6c9` | Camoufox; several web UIs → OpenAI-compatible API | Camoufox banned; site selectors / flows may be read as behaviour notes |
| gpt4free | `xtekky/gpt4free` | **GPL-3.0** (`LICENSE`) | `74661d1` | Browser to obtain cookies / HAR / anti-bot tokens, then direct HTTP | Concept only (GPL); the technique itself is banned by ADR-014 §2 |
| chat2api | `lanqian528/chat2api` | MIT (`LICENSE`, "Copyright (c) 2024 aurora-develop") | `a63a90c` | No browser; reverse-engineered web backend, multi-account rotation | Backend calls and Sentinel/PoW generation banned; reading its notes on the SSE delta format is allowed as knowledge of what the page receives |

### Agent rule

- **Reuse mode for all of the above: concept only.** Even where the code is MIT, the core of each project is a technique ADR-014 §2 forbids (fingerprint spoofing, stealth drivers, cookie / HAR extraction, off-page token generation, direct backend calls, account rotation).
- What may be learned: which page request carries the answer stream, its event format, how the composer and model picker behave. Anything ported (e.g. a parsing table) needs the MIT notice in `THIRD_PARTY_NOTICES.md`; nothing may be ported from AIstudioProxyAPI or gpt4free.
- For the CDP mechanics themselves, read the official protocol (`ChromeDevTools/devtools-protocol`, BSD-3-Clause) first, then chromedp (MIT) or Playwright / Puppeteer (Apache-2.0). Do not use Camoufox, nodriver, undetected-chromedriver or puppeteer-extra-stealth as references for implementation.
- Rust CDP crates checked for ExecPlan 017 (crates.io, 2026-10-15): `chromiumoxide` 0.9.1 (MIT OR Apache-2.0), `chromey` 2.58.2 (MIT OR Apache-2.0), `headless_chrome` 1.0.22 (MIT). None adopted (ExecPlan 017 D1).

---

# 11. Agent Source-Reuse Workflow

When implementing a substantial feature:

## Step 1 — Identify feature

Example:

```text
Implement Kimi subscription usage
```

## Step 2 — Check this reference map

Expected lookup:

```text
Usage/quota provider discovery
→ CodexBar first
```

## Step 3 — Check official provider source/docs

Official behavior overrides community assumptions.

## Step 4 — Verify license of exact source path

Do not rely only on repository name.

Check:

```text
LICENSE
file header
directory-specific license
NOTICE
```

## Step 5 — Choose implementation strategy

One of:

```text
ORIGINAL
PORTED
DERIVED
COPIED
CONCEPT_ONLY
```

## Step 6 — Record provenance when code is reused

Example entry:

```text
Component:
Kimi quota parser

Source:
steipete/CodexBar

Upstream path:
...

Upstream revision:
<commit>

License:
MIT

Reuse:
Ported and modified

Local files:
crates/providers/kimi/usage.rs
```

Put this in:

```text
THIRD_PARTY_NOTICES.md
```

## Step 7 — Preserve notice/header where required

Never remove upstream copyright/license notices merely because Loadout uses Apache-2.0.

---

# 12. Token-Saving Rule for Agents

Do **not** browse every reference project for every task.

Use at most:

```text
1 primary reference
+
1 secondary reference
+
official provider docs/source
```

unless the first two are insufficient.

Recommended workflow:

```text
Feature
 ↓
reference-map.md
 ↓
primary project
 ↓
targeted repo/file search
 ↓
official provider source/docs
 ↓
implement
```

Avoid:

```text
Feature
 ↓
search web broadly
 ↓
clone six repos
 ↓
read six READMEs
 ↓
compare implementations
 ↓
finally code
```

---

# 13. Research Escalation Rules

Agent may expand beyond the table only when:

- primary reference lacks the provider;
- behavior changed upstream;
- implementation is broken/outdated;
- license is incompatible;
- official provider behavior contradicts community code;
- security-sensitive behavior needs confirmation.

When discovering a better reference, update this document instead of keeping the knowledge only inside an ExecPlan.

---

# 14. Recommended Initial Repository Policy

Adopt the following now:

```text
Project license:
Apache-2.0

Contribution:
DCO, no CLA initially

Permissive upstream:
reuse allowed with provenance and notices

MPL:
case-by-case isolated reuse

GPL/AGPL:
concept-only by default

Unknown/custom:
no code reuse

Provider service terms:
review separately from code license
```

---

# 15. One-Line Rule for the Agent

## WorkBuddy International（2026-10-17）

- Primary audit: [Zhengyuuuui/workbuddy2api](https://github.com/Zhengyuuuui/workbuddy2api), revision `1cc32c9a98abb53796d8d4e8d477736fe1f0c4cc`, root MIT. Checked `server/codebuddy_proxy.py`, `server/codebuddy_client_demo.py`, `server/workbuddy_ai_client_demo.py`, `server/test_nonstream_hardening.py`. No source copied. Reject stock deployment: cross-model retry, implicit conversation merge, plaintext session, no local API authentication.
- Secondary audit: [VentourF/WorkBuddy2api](https://github.com/VentourF/WorkBuddy2api), revision `ed6c2d9e380b53028a74a70da18d8f36b2fb8414`. Exact path `proxy/wbproxy.py` license not verified; no reuse. Capture writer includes request headers/body; unsuitable for stock deployment.
- Prefer installed official international client IPC: `resources/app.asar` entries `main/server.js` (`wb.request/http.fetch`, HMAC handshake) and `main/file-authentication-storage.js` (account metadata path). Interface/concept reference only; no official implementation copied. New scripts are original, use Node/Python standard libraries and keep tokens in the desktop client.
- [Official international pricing](https://www.workbuddy.ai/pricing/), [official FAQ](https://www.tencentcloud.com/document/product/1256/77297): Free 30 / Pro 50 daily active credits, qualifying client activity, next-day settlement. No claim that API bridge traffic qualifies.
- See [ADR-016](../adr/ADR-016-workbuddy-international-ipc-bridge.md) and [Provider notes](../providers/workbuddy.md).

> **Reuse permissive code deliberately, preserve provenance, treat AGPL as architecture-only, and always prefer official provider behavior over community reverse engineering.**


## AxonHub targeted reference (2026-10-18)
用户指定 [looplj/axonhub](https://github.com/looplj/axonhub)，固定研究 commit `a946fca495c52344ed073abb6c3562b9ff728b12`。作为 B 类协议状态机参考：`llm/pipeline/stream.go`、`llm/transformer/openai/inbound_stream.go`（取消、工具增量按 index 聚合、终止事件校验）。注意根 LICENSE 是 Apache-2.0，但根 NOTICE 与 `llm/LICENSE` 明确 **llm/ 为 LGPL-3.0**。本任务仅研究概念，不复制/翻译实现、不引入依赖。官方 OpenCode 源码与本机实测决定 bridge 能力。

## opencode2api targeted reference (2026-10-18)
用户指定 [jasonxu114514/opencode2api](https://github.com/jasonxu114514/opencode2api)，固定 commit `83d623d541c47a17cd832ee4f53bd7c95f3a18fa`。检查根 LICENSE（GPL-3.0）及 `internal/identity/request.go`、`internal/gateway/upstream.go`、`internal/gateway/gateway.go`、`internal/gateway/probe.go`、`internal/protocol/request.go`、`internal/protocol/stream.go`、`internal/httpx/client.go`；tree 未发现这些路径的嵌套 LICENSE。仅概念参考，无代码复制/翻译或依赖引入。
该项目直接调用上游，不使用官方 serve；生成官方形状 session、声明 CLI User-Agent，匿名路径补入/覆盖内置工具 schema，并在 legacy/new upstream 与代理间重试。该身份/工具模拟路线不符合 ADR-017；不作为现有官方桥接的直接修复。可参考 SSE 终止/取消、工具增量关联等一般协议概念。项目源码注释对上游校验规则的描述是作者推断，不是官方保证。
Issue [120](https://github.com/jasonxu114514/opencode2api/issues/120) 报告 FreeTierError，回复称当前无法解决并建议代理池；[109](https://github.com/jasonxu114514/opencode2api/issues/109) 报告尝试 IP 池仍失败。社区反馈不证明本机当前可用性或本次阻断原因。

## OpenCode freshness search (2026-10-18)
用户要求扩展到最新开源实现，故扩大候选元数据/README/issue 搜索范围；新增源码重点仅 primary KochC 与 secondary DESN131，未复制或引入代码。
- [KochC/opencode-llm-proxy](https://github.com/KochC/opencode-llm-proxy), `cc1ad58450ef15e5a1ecafa531a77a6dc4d22e0b`, default-branch commit 2026-10-18 08:39 UTC, v1.11.1, root MIT (路径许可证复用前需再核验)。检查 README、opencode-v2.js。今日修复 silent event stream；V2 context hook 删除非 proxy tools，替换 system；README 明确 max-token controls accepted but not enforced。可参考官方 plugin 生命周期，但不满足本任务严格输出预算，且删内置工具与本机此前失败实验相似。
- [DESN131/opencode-zen-proxy](https://github.com/DESN131/opencode-zen-proxy), `b48d93ef91a6eae3c71f04b99a49294e7af9d28a`, 2026-10-18 05:24 UTC。检查 README、opencode-zen-proxy.mjs；tree 仅 README 两份、mjs、package.json，没有 LICENSE，不复用。轻量 Node 网关，Chat/Responses/Anthropic、stream/tools，legacy Zen 请求形状和 read marker；未发现本机同一错误已修复的证据。作为隔离实测候选，不声称可用。
- [ebouther/opencode-unlimited-proxy](https://github.com/ebouther/opencode-unlimited-proxy), `67e3034945ab7083c210e6268f0e83fc214eb511`, 2026-10-18 09:40 UTC。README/commit 元数据审查；自有文件声明 MIT，推理引擎仍是 GPL opencode2api，新增 WARP/代理出口池而非独立协议引擎。未部署、未改变本机网络，不把 unlimited 命名作为容量证据。
- [Fast-Editor/OpenCode-Wrap](https://github.com/Fast-Editor/OpenCode-Wrap), `81aafcccba831f60375785026604f8ef5df1c1f2`, 2026-10-15；V2/Windows bootstrap 修复，license 未确认，无同错误修复证据。
- [FishBottle7/opencode2dsh](https://github.com/FishBottle7/opencode2dsh), `06a702931abcb7505147e848bf817f577f1cd577`, 2026-10-14, v0.3.12；9月及10月 session gate 修复是历史适配证据，不能推断10月18日可用。
- 已测 opencode2api 的 83d623d 仍为此次查询时最新 main（10月17日），含10月16日 legacy shape/read alias 修复；不应把本次失败归为使用旧版本。

证据索引：官方仓库 [#56756](https://github.com/anomalyco/opencode/issues/56756) 出现精确 Detect Abuse Traffic（10月18日），回复来自 opencode-agent bot，非人工服务端根因报告；[#56334](https://github.com/anomalyco/opencode/issues/56334) 包含同 DeepSeek 2.0.32 同机同账号 Wi-Fi 0/5、热点5/5用户报告，亦有间歇失败报告。均不能代替本机网络因果对照。历史 [opencode2dsh #9](https://github.com/FishBottle7/opencode2dsh/issues/9)、[#62](https://github.com/FishBottle7/opencode2dsh/issues/62) 支持门槛多次变化，但精确检查规则仍是社区逆向结论。

### Release updater reference (031)

- Primary: GitHub REST official releases API, https://docs.github.com/en/rest/releases/releases#get-the-latest-release (2026-10-18). Metadata and binary assets only; no third-party updater code copied.
- Installation uses the existing Windows spawn_detached abstraction and a locally embedded PowerShell helper. SHA-256 is integrity checking under repository/HTTPS trust, not an independent signature.
- Official GCM credential-store documentation: https://github.com/git-ecosystem/git-credential-manager/blob/main/docs/credstores.md and docs/environment.md. Windows default `git:https://github.com` entry is read-only; no GCM code copied. Private asset downloads use GitHub's releases/assets API, not anonymous browser URLs.
