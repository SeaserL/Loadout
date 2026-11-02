# Loadout Open-Source License & Reference Map v0.1

> **Loadout — All your AI, one endpoint.**

## 0. Purpose

This document has two purposes:

1. Define the recommended open-source licensing strategy for Loadout.
2. Give coding agents a compact lookup table for existing projects so they can jump directly to relevant prior art instead of repeatedly researching the same ecosystem.

This is general software-licensing guidance, not legal advice.

## 当前采用状态速查

065 修正：用户未接受064的产品呈现；064的后台测试不能证明桌面设计完成。新源码直接参照本地Magpie `fd80ed99c9a97a2f6f96ee62e36eba9eefb16036` 的 `site/public/img/masking-zh-light.png`（细线束/中心节点）、`routing-zh-light.png`（路由组层级）、`usage-zh-light.png`（用量分组），原创实现流向和交互，不复制其源代码、图标或截图作为产品资源。OmniRoute继续承担所有管理API；价格/配额/预算页复用既有接口，侧栏按其管理职责分区。Tauri复用Loadout现有依赖，独立入口不调用生产Bridge。详见[065记录](../research/2026-11-01-fusion-desktop-redesign.md)。

064 补充（隔离候选已验收，未正式部署）：`apps/fusion-console` 原创 React 页面借鉴 Magpie 的顶部导航、用量分组和流向表达；不复制其 DOM/插件内核。数据直接读取 OmniRoute 管理 API。直接导入固定 v3.8.52 的 `src/lib/radar/autoSync.ts` 原生过期判据，仅显示待更新，不自动同步。React/React DOM/MIT、Lucide/ISC 与 OmniRoute/MIT 完整许可证随静态资源打包。实际浏览器审查、同包247项及058→064升级通过；最新范围见[064报告](../research/2026-11-01-fusion-console-progress.md)，下文旧阶段候选为历史记录。

更新于2026-10-31。入口见 [README功能来源表](../../README.md#功能与开源来源速查)。本节描述实际采用状态；下文早期的“可借鉴功能”和优先级表是研究导航，不代表代码已经移植。详细固定revision、源文件和证据见各项目原有条目，避免重复立项调研。

后续后台融合先查[功能清单与原生源码入口](../architecture/fusion-backend-status.md)。模型到combo映射、目录、用量/价格/配额、Radar同步和时间戳均已有上游实现，先验证复用，避免另造同类模块。旧Codex订阅扫描及受控登录已在053按现有Loadout行为移植；原生model-combo-mappings与key权限承担别名和旧池接线，证据见[053报告](../research/2026-10-31-subscription-native-migration.md)；商汤、CommandCode、WorkBuddy外部账号问题按用户指示延期，不作为本阶段完成条件。

| 项目 | 已采用或已研究的内容 | 当前状态与边界 |
|---|---|---|
| [OmniRoute](https://github.com/cheaper-inference/OmniRoute) | 协议、Provider接入、多账号、SQLite、管理API、等待重试、免费目录/Radar | 隔离融合候选的实际源码底座，v3.8.52；凭据、账号限制与429节流有窄适配。054接通只读目录/统计/诊断；055接通原生社区免费目录、日期、缓存和同步并通过隔离专项；056接通原生价格目录、来源、同步与成本衔接；管理UI未融合。早期B类概念参考已由039–050升级为候选主底座，正式Rust版未替换。 |
| [dsh-workbuddy-connect](https://github.com/AidenOrz/dsh-workbuddy-connect) | 内存读取官方当前凭证、请求头、客户端身份及错误分类 | 046–050选择性复用0.7.15 MIT模块；不采用DSH宿主、刷新落盘和全正文提示词改写。 |
| [yetone/magpie](https://github.com/yetone/magpie) | 流向UI与trace语义；另研究过agent配置编辑、粘性、用量提示 | 设计/源码研究，尚无Magpie代码或UI进入当前候选包。不是采用它的完整网关或插件内核。 |
| [CodexBar](https://github.com/steipete/CodexBar) | 订阅凭据发现、额度接口与字段 | 现有Rust provider实现借鉴接口知识；原创实现，具体条目见notices。 |
| [EasyCLIProxyAPI](https://github.com/router-for-me/EasyCLIProxyAPI) | 桌面主题、侧栏、卡片、额度条 | 现有Rust/Tauri桌面端视觉借鉴，未复制代码。 |
| [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) | 旧Codex订阅接口知识；协议/执行器替换实验 | 接口知识已借鉴；036整层替换未采用，不是当前候选的第二个运行时。 |
| [OpenCode](https://github.com/anomalyco/opencode) | 原生服务接口和Windows官方运行时 | 旧版可选本地桥接调用外部官方程序；没有复制其源码，未将其全部能力带入融合包。 |
| [AxonHub](https://github.com/looplj/axonhub) | 协议状态机、工具增量、取消/终止、路由与限流设计 | 概念/源码研究，无代码移植；尤其llm子树LGPL不按根Apache许可证误用。 |
| [Ponytail](https://github.com/DietrichGebert/ponytail) | Agent兼容矩阵、A/B验收、版本一致性检查思路 | 已研究，未复制插件或代码，也未作为网关底座。 |
| [CC Switch](https://github.com/farion1231/cc-switch) | 客户端配置与桌面工具管理 | 037完整产品选型评估过，未选为多账号网关底座；不是本项目的agent协同引擎。 |
| [Blinue/Magpie](https://github.com/Blinue/Magpie) | Windows常驻工具的版本化配置、托盘/自启工程思路 | 与yetone/magpie不同的窗口缩放项目；仅概念参考，GPL代码未移植，部分做法原出处为PowerToys。 |

广筛中的其他产品（ProxyPal、New API、Bifrost、Relay AI、lmgw等）保留在037选型记录，不因为“看过”就列为实现来源。免费目录可用性、上游功能存在、代码已复用、真实账号已验收也必须分别记录。

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

**全面对比（2026-10-19，同一 commit）**：结论与逐项分析见 [docs/research/2026-10-19-axonhub-review.md](../research/2026-10-19-axonhub-review.md)。按主题的参考路径如下；`internal/` 为 Apache-2.0（移植前仍须核对文件头），`llm/` 为 LGPL-3.0，只能参考概念：
- 会话识别 / 路由粘性：`internal/server/middleware/trace.go`、`internal/server/orchestrator/candidates.go`、`internal/server/biz/request.go`、`internal/server/biz/channel_apikey_provider.go`；LGPL：`llm/transformer/anthropic/claudecode/userid.go`、`llm/transformer/openai/codex/headers.go`。
- Prompt cache 断点 / 缓存 key（全部 LGPL）：`llm/transformer/anthropic/ensure_cache_control.go`、`llm/transformer/openai/responses/cache_anchor.go`、`llm/pipeline/cc/system_messages.go`。
- 跨 Provider reasoning 签名识别（LGPL）：`llm/transformer/shared/signature.go`。
- 并发 / RPM / 429 冷却：`internal/server/orchestrator/channel_limiter.go`、`channel_limiter_manager.go`、`channel_request_tracker.go`、`rate_limit_tracking.go`。
- 负载均衡 / 熔断 / 自动停用：`internal/server/orchestrator/load_balancer.go`、`lb_strategy_*.go`、`internal/server/biz/model_circuit_breaker.go`、`channel_metrics.go`、`channel_auto_disable.go`。
- 额度节奏 / 订阅额度折算：`internal/server/biz/provider_quota/routing.go`、`internal/server/orchestrator/candidates_quota.go`、`internal/server/biz/provider_quota_cost.go`。
- 模型同步 / 协议探测 / 批量添加：`internal/server/biz/channel_model_sync.go`、`channel_endpoint_detect.go`、`channel_bulk.go`。
- 价格模型（阶梯、分时段、5m/1h 缓存写入）：`internal/objects/price.go`、`internal/server/biz/cost_calc.go`。
- Responses 续接与恢复错误：`internal/server/orchestrator/responses_session.go`、`responses_attempt.go`；LGPL：`llm/transformer/openai/responses/websocket_executor.go`。
- 提示词保护：`internal/server/biz/prompt_protection_rule.go`、`internal/server/orchestrator/prompt_protection.go`。
- 反面参考（不照搬）：正文默认落库、明文 key 与明文备份、热路径查库、任何错误都换渠道、身份伪装类订阅 transport。AxonHub `docs/` 有多处与代码不一致，以代码为准。

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

## Ponytail reference (2026-10-23)

用户指定 [DietrichGebert/ponytail](https://github.com/DietrichGebert/ponytail)，固定 commit `09c434ab73ca43761c5d8b0c5d341926c2fba710`，根 LICENSE 为 MIT。它是 coding agent 的提示词插件，不是网关。结论见 [docs/research/2026-10-23-ponytail-review.md](../research/2026-10-23-ponytail-review.md)。只借鉴概念，未复制代码。B 类参考：
- Agent 级 A/B 测评方法：`benchmarks/agentic/`（先自检再花钱、各组交替运行、隐藏检查、离线重算）。
- 多 agent 接入矩阵的维护方式：`docs/agent-portability.md`（每个宿主写明"已验证版本"）。
- Claude Code band 模块与本地账本：`hooks/band/`。
- 版本号一致性检查：`scripts/check-versions.js`。

## Magpie reference (2026-10-23)

用户指定 [Blinue/Magpie](https://github.com/Blinue/Magpie)，默认分支 `dev`，固定 commit `9e8e90274ba07f555bf87f09321dabdac97b1b4f`。根 LICENSE 为 **GPL-3.0**，只能参考概念，不得复制或翻译。它是 Windows 窗口超分辨率工具，与 LLM 无关。结论见 [docs/research/2026-10-23-magpie-review.md](../research/2026-10-23-magpie-review.md)。C 类参考，只看 Windows 常驻工具的工程做法：
- 版本化配置目录（升级不破坏旧格式，可以降级）：`src/Magpie/AppSettings.cpp`。
- 独立更新器与按旧版本清单清理文件：`src/Updater/`。
- 托盘在资源管理器重启后的恢复：`src/Magpie/NotifyIconService.cpp`。Loadout 用的 tray-icon 0.25.1 已经处理，无需再做。
- 自启与托盘补救的原始出处是 **PowerToys（MIT）**：`src/runner/auto_start_helper.cpp`、PR #789。需要实现时读 PowerToys，不读 Magpie。
- 反面参考：触控支持会把自签名证书写入本机受信任根，并把程序复制到 System32。

## yetone/magpie reference (2026-10-23)

用户指定 [yetone/magpie](https://github.com/yetone/magpie)，固定 commit `3dc25a61681fac109c12f4b065146b9204cdc37e`（v0.1.1333）。根 LICENSE 为 MIT，按文件核对：`internal/qoder/endpoints.go` 带 CLIProxyAPI 的版权头。这是 Loadout 最直接的竞品（本地网关 + agent 配置写入 + 订阅 + 路由组）。结论见 [docs/research/2026-10-23-yetone-magpie-review.md](../research/2026-10-23-yetone-magpie-review.md)。
- **A 类**：Agent 接入与保格式编辑。`internal/agent/`（`claude.go`、`codex.go`、`stash.go`、`applied.go`）、`internal/edit/`（JSONC 偏移拼接、TOML 写后校验、保留 BOM / CRLF）。
- **B 类**：
  - 缓存感知粘性与 failover 细节：`internal/gateway/affinity.go`、`fallback.go`、`routing.go`（`probeOne`、两层冷却）；
  - 计价与 models.dev 目录：`internal/catalog/`；
  - 额度提醒：`internal/provider/quota_alert.go`；
  - client key 限额：`internal/access/limit.go`；
  - 集成契约：`docs/integrating.md`；
  - 导入链接校验：`internal/provider/importurl.go`。
- **反面参考，不照搬**：
  - Claude CLI 常驻桥接（`--dangerously-skip-permissions`）；
  - 读写其他工具的凭证；
  - 冒充 Codex / Antigravity / OpenCode 的 UA；
  - 预热刷窗口；
  - 明文密钥；
  - loopback 接受任意 token；
  - 无沙箱的 Bun 插件与 JS 中间件；
  - 意图路由（选路前调用 LLM）。

## CLIProxyAPI isolated replacement evaluation (2026-10-29)

固定 `90c447d3a8727d115aa7552ec93ce023736ed492`（module v8），根 MIT；sdk/internal 范围未找到嵌套 LICENSE/NOTICE。详情见 [隔离验证报告](../research/2026-10-29-cliproxy-isolated-evaluation.md) 与 `development/experiments/cliproxy-eval/`。

- **本轮结论：保留 Loadout 协议/接入主链路，不启动整体迁移。** CPA 单账号执行可行，但不能据此移交凭证、安全、状态路由和错误分类责任。
- Public SDK：`sdk/translator/**`（独立 module 实测可用）；`sdk/cliproxy/auth/**`（pin、retry、Store）。内置 executor 在 `internal/runtime/executor/**`，独立 module 直接导入被编译器拒绝。
- 需要保留调用方检查：缺失转换边的 fallback、有状态 Responses、Codex 特有正规化、敏感错误/Auth Debug、流终止。CPA 自身 HTTP handler 也承担部分终止保护，不能把执行器级失败泛化为整个服务失败。
- 实验：55个叶级组件检查46通过9不满足直接替换契约；现有 Gateway 经最小 Chat bridge 串联14项通过。后者保留 Loadout 协议转换，不是完整替换通过。
- 没有复制上游实现或引入生产依赖。临时 checkout 保留上游许可证，实验依赖和接口知识记录在 `THIRD_PARTY_NOTICES.md`。

## Complete product foundation research (2026-10-29)

用户明确要求从完整产品重新选底座，并接受更换UI与操作方式、优先少维护，因此本轮扩大候选范围。报告见 [完整产品选型](../research/2026-10-29-product-foundation-review.md)，需求见 [可迁移需求包](../research/loadout-portable-requirements.md)，版本与来源见 [证据快照](../research/2026-10-29-product-foundation-evidence.json)。

- **整产品迁移首选试点**：[cheaper-inference/OmniRoute](https://github.com/cheaper-inference/OmniRoute)，MIT；发布基准 v3.8.52 / `ad6f33f66bda34eba98645d3bfdabbde294bc843`。原生账号范围、优先级与轮转可承接部分需求；`src/lib/db/encryption.ts` 的无密钥/异常明文回退、日志覆盖、Windows运行必须先验收。不能将默认分支的新功能当作该发布版能力。
- **备选完整产品**：[yetone/magpie](https://github.com/yetone/magpie)，当前源码 `fd80ed99c9a97a2f6f96ee62e36eba9eefb16036`。产品贴近个人需求，但当前不接收外部PR，凭证保护需要核心适配。更新旧条目的时效边界：中间件现为Go进程内JS，Provider插件仍另有执行机制；策略失败退回原顺序，不能当作强制安全控制。
- **桌面工具管理候选**：[farion1231/cc-switch](https://github.com/farion1231/cc-switch)，MIT，当前源码 `ddcb111765be66ed9e7e43e240497fd2c9576688`；聚合模式不提供failover，不作为多账号总API的首选底座。
- 广筛还包括 EasyCLIProxyAPI、ProxyPal、AxonHub、New API、Bifrost、Relay AI、lmgw。New API为AGPL；AxonHub有LGPL子树；整产品采用与向Apache项目复制代码是不同决策。
- 此处的迁移建议是选型结论，未替换正式环境或修订现行ADR。上一节CPA“保留主链路”是036组件替换实验结论，不构成否决整产品迁移的永久决策。
- 本轮未移植代码或引入生产依赖；第三方来源仅作研究链接，未新增需进入THIRD_PARTY_NOTICES的代码副本。

## Fusion architecture and local sources (038, 2026-10-29)

用户要求先出三项目解耦文档，并将上游源码放入当前项目。设计见 [融合架构](../architecture/loadout-fusion-design.md)，决策为 [ADR-019 Proposed](../adr/ADR-019-product-fusion-boundaries.md)，尚未修改正式运行时。

- Primary：OmniRoute v3.8.52 / `ad6f33f66bda34eba98645d3bfdabbde294bc843`，本地 `development/upstreams/omniroute/`；网关/Provider/管理界面/免费目录主体。
- Secondary：Magpie `fd80ed99c9a97a2f6f96ee62e36eba9eefb16036`，检出时tag为v0.1.2026，本地 `development/upstreams/magpie/`；流向视觉与真实trace语义。
- 定点文件：OmniRoute `src/lib/db/{providers,encryption}.ts`、`apiKeys/secretAtRest.ts`、`src/sse/services/{auth,keyConnectionScope}.ts`、`src/lib/{radar,usage}/`；Magpie `internal/gui/assets/plugins/routing/{client.js,routing.css}`、`internal/gateway/trace.go`。根MIT与祖先许可证检查覆盖这些选读路径，完整依赖许可另审。
- 重要边界：Magpie脚本依赖DOM插件/trace API，不能当作React组件直接导入；OmniRoute可逆client key保存、quota preflight、Electron quit联动需适配；Loadout点名账号默认允许同模型failover，不等于stateful固定。
- checkout保留Git历史、LICENSE与notices，被父仓库忽略，不进入构建；来源锁定见 [development/upstreams.lock.json](../../development/upstreams.lock.json)。本轮新增的是本地原样参考副本，无产品代码移植。

## OmniRoute bounded native wait (045, 2026-10-30)

- [045验证](../research/2026-10-30-omniroute-bounded-wait-provider-validation.md)：接受全局/累计原生等待，最多2次wait和30秒sleep预算；核心17项、编译器16项通过，新增runtime patch0。
- 固定v3.8.52 `open-sse/services/rateLimitManager.ts` 的429 eviction会丢失前次排队时间记录；20秒普通请求节流不能保证429后的20秒间隔，追加反例2127ms，保留FAIL作为商汤迁移门槛。
- CommandCode官方直连401/EXPIRED；WorkBuddy官方IPC恢复，但推理400/code11128，继续隔离候选、不切正式环境。

## OmniRoute isolated phase 1 (039, 2026-10-30)

- 固定v3.8.52；[运行验证报告](../research/2026-10-30-omniroute-phase1-validation.md)、可重放实验。正式环境未修改。
- 7个MIT原文件接缝、2个原创模块：Provider secret_ref/Windows Credential Manager、client key hash-only、空集合约束、首选账号及SSE提前keepalive限制；没有第二套router。
- 复用上游已声明的MIT keytar 7.9.0。相应license副本、npm包来源/integrity和native模块hash在实验目录；完整依赖闭包尚未发布审计。
- 主要实验18/18、focused回归62/62；原生headless包启动成功。带补丁完整包与跨版本运行升级未完成；相邻旧标签静态试套4个文件不兼容，不能据代码量小断言低维护。

## OmniRoute package and upgrade evaluation (040, 2026-10-30)

- [完整后台包与升级验证](../research/2026-10-30-omniroute-package-upgrade-validation.md)：v3.8.51→v3.8.52实跑升级，16项migration、原key/ref、账号规则及旧数据副本回滚通过；不涉及正式Loadout迁移。
- 最终旧版补丁前向应用有6个原文件冲突及1个已被上游提供的新文件；新版8个原文件接缝，核心自有模块格式化后201行。区别机械重定位、上游接管与真正行为变化。
- SQLite并非查询速度瓶颈，但500ms写锁可阻塞热路径；去重熔断器写入后，选账号中的同步租约清理仍阻塞。请求内memo不能满足现有RouteSnapshot边界。
- **当前约束下不建议继续作为整产品替换底座推进深度融合**，不进入UI/正式迁移。039/038的“首选试点”是当时的试验选择，已由040运行证据更新；仍可定点参考协议与策略实现。

## Native SQLite reassessment (041, 2026-10-30)

- 用户重申少维护优先，要求接受原生实现的路线评估。040停止建议只适用于保留原内部契约的前提，不能无条件用于新路线。
- [041评估](../research/2026-10-30-omniroute-native-sqlite-assessment.md)推荐OmniRoute原生SQLite/状态模型加最小凭证与账号适配，作为后续首选验证方向；不重建RouteSnapshot、不退回全量自研，Magpie视图可选。
- 当时剩余风险是同步锁影响、NORMAL异常断电耐久性、已认证明文JSON导出、凭证生命周期及账号规则全入口覆盖。041只读评估没有新的runtime PASS；以下042提供后续实跑证据。

## Native SQLite validation (042)

- [042实跑](../research/2026-10-30-omniroute-native-sqlite-validation.md)：两版完整后台目录132/132通过，原生队列配置对照28/28通过；凭证生命周期、三个协议账号规则、193→209 migration及回滚可复核。
- 继续以OmniRoute为主底座，接受原生SQLite；新版430行自有适配，14个上游文件+91/-17。不是零补丁插件，升级必须审查接入点并回归。v51旧key撤销缓存及v52写后返回差异均有真实反例和修复。
- 新增定点参考：`src/lib/db/providers/deletion.ts`、`src/app/api/{keys/[id]/reveal,settings/export-json,settings/import-json,resilience}/route.ts`、`open-sse/services/rateLimitManager.ts`；实验代码和notice见phase3。API key受限profile通过不等于所有OAuth/免费平台或生产安全入口已验收。

## Fixed-baseline real clients and migration (043)

- [043实跑](../research/2026-10-30-omniroute-real-client-migration-validation.md)：固定v3.8.52；原生自定义Provider要求适配白名单新增prefix/nodeName两行，原有14个上游hook不增加。原生SQLite继续保留。
- 官方客户端文本与Claude/OpenCode工具通过合成上游；OpenCode免费桥接真实文本、加密恢复后真实文本通过。CommandCode 401、商汤429、WorkBuddy官方IPC缺失仍阻塞正式验收，不当作底座必须重写的证据。
- 新增定点参考：`src/lib/providers/compatibleNodeCreation.ts`、`src/app/api/provider-nodes/route.ts`、`open-sse/executors/base.ts`。后者的同URL429重试独立于requestRetry；测试出口已补拒绝后停止，但产品行为仍需对齐。
- 迁移工具为显式partial；原gpt池、WorkBuddy池、商汤端点等待语义尚未全部映射。继续最小适配候选，暂不迁移正式环境。

## Native pool and endpoint mapping (044)

- [044隔离验证](../research/2026-10-30-omniroute-routing-mapping-validation.md)：旧gpt/WorkBuddy池映射为原生priority combo；failoverBeforeRetry=true与零target/set重试可消除同账号先重试，三协议及12账号链通过。新增运行时补丁0。
- 原生`comboCooldownWait`对全部combo生效，且30秒budget不是旧端点的请求起点deadline；两个真实后台mock反例确认不等价。编译器拒绝启用非零等待端点，不静默改全局规则。
- 本轮定点参考：`src/app/api/combos/route.ts`、`src/shared/validation/schemas/combo.ts`、`open-sse/services/comboConfig.ts`、`open-sse/services/combo/{comboAttemptLoop,comboCooldownRetry}.ts`。用户外部待办见报告，未再次调用真实Provider。

## Narrow pacing and user-selected WorkBuddy reference (046)

- OmniRoute primary仍为固定v3.8.52。main `55ccad495c4a73eb64deaf094829fe5b90ded80c` 的 `open-sse/services/rateLimitManager.ts` 与该基线相同；[PR11413](https://github.com/cheaper-inference/OmniRoute/pull/11413)已关闭未合并，不能当作可采用修复。实验复用原生`SlidingWindowLimiter`保存独立dispatch floor，不重写路由。
- 用户追加指定的secondary：[AidenOrz/dsh-workbuddy-connect](https://github.com/AidenOrz/dsh-workbuddy-connect)，固定`25680ca7f7ff5d7b84bb35f45de766519fccbc9d`、实际package版本0.7.15、MIT，参考副本`development/upstreams/dsh-workbuddy-connect/`。精读`src/{upstream,auth,desktop-credential-protection,client-identity,variants}.ts`和`docs/channel-check-fix.md`；具体复用见THIRD_PARTY_NOTICES。
- 该项目11128解释涵盖缺会话头、首条system要求及渠道提示词检查，错误码不能唯一定位原因。原生插件会写刷新凭证副本，并可改写提示词；本轮仅复用内存凭证读取和连接头，不接入这两种行为。
- 本机隔离冻结OmniRoute→桥接→指定项目直连能力已返回200/预期合成文本，原账号/模型保持；正式IPC路径未修改，生产与长会话/工具兼容尚未验收。

## Daily-use verification (047)

- 继续使用046固定来源和后台包，新增后台hook0。参考`src/upstream.ts`中强制SSE的既有语义，窄relay只设置stream=true，不复刻协议转换或提示词指纹规则。
- Claude Code/OpenCode受限真实文本/只读工具与149KB三轮history通过；Codex仍真实HTTP400渠道拒绝，同形mock可成功，role已经system，无需新增developer映射。
- 原生key provider支持内存cache；17项离线relay/轮换检查通过，真实token轮换未发生。商汤一次真实429后停止，不能据mock结论宣称真实风控已解除。见[047报告](../research/2026-10-30-daily-client-credential-provider-validation.md)。

## Codex channel diagnosis (048)

- [048 隔离诊断](../research/2026-10-30-workbuddy-codex-channel-diagnosis.md)：15 次固定账号/模型对照，将当前 Codex 11128 定位至首条 system 的身份开头；工具字段、text 数组、其他消息均有成功对照。只对第一行应用 OmniRoute 既有规则，实际客户端生成的完整请求重放成功；不是完整 Codex 工具闭环已修复。
- **固定 v3.8.52 已有处理**：`open-sse/config/providers/registry/workbuddy/index.ts` 的 `bodyStringReplacements` 与 `open-sse/utils/strictChatHistory.ts` 的 `replaceBodyStrings/applyRegistryBodyRepairs`。实验走自定义 Provider，未继承内建 workbuddy registry 规则。当前 main `55ccad495c4a73eb64deaf094829fe5b90ded80c` 的规则一致。
- 原生 helper 作用于所有字符串，会改写合成用户文字/路径/工具 arguments；不直接开启全文替换。建议只在连接器匹配已验证 system 首行，范围外逐字保留；本轮后台及 relay 新补丁均为 0。
- 指定 AidenOrz 0.7.15 的准备函数实测对当前 Codex body 无改动，内置 DSH 规则不命中。历史 corrinehu 原仓库当前 404；GitHub 当前 fork parent 为 jojoman2024（`cc97452665eb87e7798581bdf9c80a52e54bae5b`），不混同这两个来源。

## Exact first-line adapter (049)

- [049实现与验证](../research/2026-10-30-workbuddy-first-line-client-validation.md)：仅首条system首个文本的完整已知首行，固定输出为048已验证原生规则结果；27个非空行纯函数、relay默认关闭的显式入口，无新增后台hook或runtime依赖。原生helper仅用于离线等价检查，不带入全body替换。
- 22项范围/loopback/工具验收负例与原12项凭证relay回归通过；真实Codex文本、持久会话get_goal、同session三轮204686 B长会话通过。文件读取仍因隔离Codex的本机执行策略blocked by policy失败，零Provider调用mock复现同hash错误；未放宽权限或替换范围。

## Unified isolated distribution (050)

- [050统一包](../research/2026-10-31-isolated-fusion-package.md)：继续使用固定OmniRoute v3.8.52和AidenOrz 0.7.15，不扩展参考项目。随包复用Node/keytar、原生API管理、窄连接器及既有首行函数，许可证与来源清单纳入归档。
- 定点参考新增 `src/lib/db/settings.ts` 与 `src/proxy.ts`，用于不可关闭的安全默认值和本地管理入口；17个上游运行时接缝+128/-19，未改原生SQLite/schema/Router。自有工具和适配模块不计入该差异行数。
- 同一最终ZIP解包后120/120通过，来源与清理可复核。实际旧池绑定、包含全部增量的下一版升级和外部Provider可用性仍待验收；不将打包通过当作正式迁移批准。具体复用与许可证见THIRD_PARTY_NOTICES的050条目。

## OmniRoute native read-only management (054, 2026-10-31)

- Primary仍为固定v3.8.52；复用`src/app/api/{models,provider-models}/route.ts`及`usage/{history,model-latency-stats,call-logs,route-explain/[id],combo-trace/[id]}/route.ts`，原生`src/lib/usage/`、`src/lib/db/`和`open-sse/services/combo/decisionTrace.ts`。核对根MIT及祖先目录，无另行许可声明；对应来源已补入THIRD_PARTY_NOTICES。
- 仅扩展现有proxy的GET/HEAD管理白名单；普通推理key拒绝全局管理读取。未另造统计/目录/路由服务，未改SQL/schema。
- [054报告](../research/2026-10-31-native-readonly-observability.md)记录最终同包验收与原生限制：目录非真实探测、成本非账单、route-explain含重算、combo trace内存TTL30分钟。免费目录/Radar同步仍未开放。

## OmniRoute native free catalog and Radar (055, 2026-10-31)

- Primary仍为固定v3.8.52；`open-sse/config/freeModelCatalog{,.data}.ts`、`src/app/api/free-models/route.ts`、`src/app/api/radar/{catalog,status,settings,sync}/route.ts`、`src/lib/radar/`、`src/lib/db/radar.ts`。所用文件继承根MIT，保留上游notice；不维护第二名单。
- 原生签名、schema、版本floor、缓存与scheduler直接复用。generatedAt与fetchedAt分开；后台catalog为24h到期/每小时检查，页面6h规则不在无UI后台执行。stale为版本未更新，旧缓存仍可读；社区feed可能延迟，见固定上游docs/frameworks/RADAR.md。
- 本轮社区目录，拒绝supporterKey输入，保留账号/路由和出站边界；[055报告](../research/2026-10-31-native-free-catalog-radar.md)区分原生测试、最终包签名mock与真实外部服务未验收。

## OmniRoute native pricing catalog and sync (056, 2026-10-31)

- Primary仍为固定v3.8.52；`src/app/api/pricing/{route,defaults/route,models/route,sync/route}.ts`、`src/lib/pricingSync.ts`、`src/lib/db/settings/pricing.ts`、`src/shared/constants/pricing/`、`src/lib/usage/{costCalculator,aggregateHistory}.ts`，相关文件继承根MIT。
- 直接复用四层价格及sourceMap、原生LiteLLM同步和计价器；无第二价格表。lastSync不是上游生成日期，未知价格/订阅等价成本/历史重算需如实说明。真实远程源未验证。


### 057 原生配额、预算与模型同步来源

Primary 仍为固定 OmniRoute v3.8.52（MIT）。复用 `src/app/api/usage/{quota,provider-limits,budget}`、`keys/[id]/usage-limits`、`providers/[id]/{models,sync-models}`、`synced-available-models` 与原生 `costRules/apiKeyUsageLimits/managedModelImport`。没有新增 secondary 项目，也没有重建余额、价格或模型库；具体状态见[057报告](../research/2026-10-31-native-quota-model-sync.md)。

### 058 发布收口与预检

没有新增外部功能参考或运行时补丁。活动构建/测试统一到 `scripts/fusion-release.ps1`、`scripts/fusion/`、`tests/fusion/`；原有WorkBuddy MIT选定模块放到 `fusion/vendor/`，依赖锁和Node匹配许可证放到 `development/fusion/`。旧实验evidence保留溯源，不再作为活动源码入口。预检复用Loadout已有snapshot/inventory/compile，不另建路由或凭据系统。详见[发布操作说明](../architecture/fusion-release-operations.md)。
