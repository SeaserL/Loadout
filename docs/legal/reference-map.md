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

| Project | License | Priority | Loadout feature to study | Agent should look for | Reuse mode |
|---|---|---:|---|---|---|
| **CLIProxyAPI** | MIT | **A** | Subscription/OAuth → unified API; multi-account providers; protocol translation; SSE; provider executors | `auths/`, `internal/`, `sdk/`, provider executors/translators, Responses/Claude/Gemini conversion | **Direct reference / port with notice** |
| **EasyCLIProxyAPI** | MIT | **A** | Tauri desktop lifecycle; OAuth UI; provider management; local core process management; installed-agent detection/configuration; tray behavior | `src-tauri/`, `src/`, `tests/`, agent config flows, OAuth screens, quota UI | **Direct reference / very high relevance** |
| **CodexBar** | MIT | **A** | Account discovery; usage/quota collection; local CLI credentials; browser/API/local fallback hierarchy; spend history | `docs/providers.md`, provider-specific docs, quota parsers, local usage scanners | **Reference algorithms/data sources; port selectively** |
| **Claude Code Router (CCR)** | MIT | **A/B** | Local control plane; agent profiles; provider presets; credential pools; condition routing; rewrites; retries; ordered fallbacks; local-login import | provider/config/routing code, desktop workflow, account import, logs | **Direct reference, but avoid duplicating its full product scope** |
| **LiteLLM OSS** | MIT outside restricted enterprise areas | **A/B** | Canonical API abstraction; provider normalization; routing; retry/fallback; virtual keys; spend tracking; model pricing database | gateway/router code, provider adapters, `model_prices_and_context_window.json`, endpoint support metadata | **Use OSS code only; never assume `enterprise/` is MIT** |
| **CliRelay** | MIT | **B** | Channel groups; path routing; failover; request logging; per-key quotas; health score; latency tracking; provider-channel management | `auths/`, `internal/`, `sdk/`, `sdkbridge/`, routing-group/channel-health code | **Study management enhancements; proxy core should usually be traced upstream to CLIProxyAPI** |
| **ccusage** | MIT | **B** | Local agent transcript parsing; token aggregation; API-equivalent cost calculation; cache/reasoning token accounting | local JSONL parsers, pricing snapshot/update logic, dedup strategy | **Direct reference for offline usage analytics** |
| **Codex Switch** | Apache-2.0 | **B** | Multiple Codex account management; auth profile switching; quota refresh; Tauri credential boundary | Tauri backend, account store, quota refresh, auth import/export safeguards | **Directly compatible; useful for Codex-specific implementation** |
| **OpenAI Codex** | Apache-2.0 | **A for Codex-specific work** | Official Codex configuration, app-server/RPC, authentication, local state conventions | official `codex-rs` code/docs; app-server/account behavior | **Prefer over community reverse engineering whenever possible** |
| **Gemini CLI** | Apache-2.0 | **A for Gemini implementation / C for unsupported service use** | Official config/auth storage and CLI architecture | `packages/core`, `packages/cli`, official auth/config code | **Code reusable; service/OAuth use must separately obey Google terms** |
| **New API** | AGPL-3.0 + additional attribution terms | **C** | Model/channel UX; weights/priorities; quota/cost management; management console data model; plugin concepts | `relay/`, `router/`, `oauth/`, `plugins/`, `model/`, `service/` | **Concept only — do not copy implementation into Apache Loadout** |

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
| Provider pricing/model metadata | **LiteLLM** | official provider docs |

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

> **Reuse permissive code deliberately, preserve provenance, treat AGPL as architecture-only, and always prefer official provider behavior over community reverse engineering.**
