# Loadout Development Harness v0.1

> **Loadout — All your AI, one endpoint.**

## 0. 文档定位

本文件是 Loadout 项目的最高级开发约束之一。

目标不是描述产品“是什么”，而是规定任何 AI coding agent 在该仓库中：

- 如何理解项目；
- 如何规划开发；
- 如何修改代码；
- 如何管理长期任务；
- 如何处理 Provider 差异；
- 如何保证兼容性；
- 如何验证性能；
- 如何避免为了快速交付而破坏架构。

本项目不以 MVP 为目标。

首个公开可用版本应当是一个结构完整、具备真实日常使用价值的 **v1**。允许分阶段实现，但不得为了“先跑起来”而永久引入明显错误的架构。

---

# 1. Project Mission

构建一个本地优先的个人 AI 账户与推理能力控制中心：

```text
AI Subscription / API / Local Models
                ↓
            Loadout
                ↓
    Unified Local AI Gateway
                ↓
 Codex / OpenCode / Claude Code
 Apps / Scripts / Trading System / IDE
```

Loadout 同时承担：

1. AI 账户管理
2. 本机已登录 Agent 扫描
3. API Key / OAuth / Subscription 导入
4. 模型目录维护
5. 用量、余额、限额、续费状态监控
6. Provider 健康检查
7. 延迟与吞吐统计
8. 本地统一 API Gateway
9. 动态/静态路由
10. Failover
11. 请求统计
12. 成本分析
13. 对客户端隐藏上游差异

项目的核心价值不是“又一个 API Proxy”。

核心价值是：

> 将用户已经拥有的 AI subscriptions、API balances、local models 和不同 coding-agent ecosystems 统一成为一个本机可观测、可调度的 AI compute pool。

---

# 2. Non-Negotiable Architecture Rules

以下规则不得随普通 feature request 修改。需要修改时必须建立 ADR。

## 2.1 Gateway 必须独立于 GUI 生命周期

错误：

```text
API Request
→ 启动桌面 GUI
→ GUI 初始化
→ Router
→ Provider
```

正确：

```text
OS Login
   ↓
Loadout Core / Daemon
   ├─ Gateway
   ├─ Router
   ├─ Provider Runtime
   └─ Database

Desktop UI
   ↓ IPC
Loadout Core
```

关闭主窗口不得中断 API 服务。

GUI 是控制面。

Gateway 是数据面。

---

## 2.2 热路径禁止网络式“动态决策”

任何进入：

```text
POST /v1/*
```

的请求，在选路之前不得执行：

- Provider quota API 查询
- Provider balance 查询
- HTTP benchmark
- DNS benchmark
- CLI startup
- browser automation
- 登录状态检测
- LLM router
- remote model classifier
- database heavy aggregation

这些操作全部必须在后台完成。

Router 使用后台提前生成的：

```text
RouteSnapshot
```

进行本地选择。

---

## 2.3 动态路由必须是 Fast Path

目标性能预算：

```text
Policy evaluation P95       < 1 ms
Route selection P95         < 2 ms
Total local router overhead < 5 ms P95

Gateway additional TTFT
excluding network/TLS:
target < 10 ms P95
```

具体数值可以根据 benchmark 调整，但：

> Router overhead must remain insignificant relative to upstream inference latency.

任何功能如果会将几十或几百毫秒加入正常请求路径，都需要重新设计。

---

## 2.4 不得每次请求启动 CLI

例如：

```text
kimi -p "..."
claude -p "..."
codex exec ...
```

不得作为标准 Transport 的 per-request 实现。

如果某 Provider 必须通过进程访问：

- 使用持久 worker；
- 建立进程池；
- 或使用 daemon/RPC；
- 或明确标为 compatibility transport。

---

# 3. Technology Baseline

默认技术栈：

```text
Desktop
Tauri 2
React
TypeScript

Core
Rust
Tokio
Axum
Tower

Storage
SQLite
WAL mode

Secrets
Windows Credential Manager
macOS Keychain
Linux Secret Service

Serialization
Serde

HTTP
Reqwest / Hyper
rustls

Observability
tracing
structured logs
```

除非 benchmark 或平台限制证明必须修改，否则不要擅自更换核心技术栈。

---

# 4. Repository Structure

推荐结构：

```text
/
├─ AGENTS.md
├─ README.md
├─ Cargo.toml
├─ package.json
│
├─ apps/
│  └─ desktop/
│
├─ crates/
│  ├─ core/
│  ├─ gateway/
│  ├─ router/
│  ├─ provider-sdk/
│  ├─ providers/
│  ├─ discovery/
│  ├─ usage/
│  ├─ benchmark/
│  ├─ storage/
│  ├─ secrets/
│  └─ protocol/
│
├─ docs/
│  ├─ product/
│  ├─ architecture/
│  ├─ providers/
│  ├─ security/
│  ├─ performance/
│  ├─ testing/
│  │
│  ├─ adr/
│  │
│  └─ exec-plans/
│     ├─ active/
│     └─ completed/
│
├─ schemas/
│  ├─ openapi/
│  └─ database/
│
├─ fixtures/
│  └─ providers/
│
├─ benchmarks/
│
└─ tests/
   ├─ contract/
   ├─ integration/
   ├─ e2e/
   └─ performance/
```

---

# 5. Required Root AGENTS.md Behaviour

Agent 每次开始工作必须按以下顺序进行。

## Boot Sequence

1. 读取 `AGENTS.md`
2. 读取当前相关产品文档
3. 读取相关 ADR
4. 检查 `docs/exec-plans/active`
5. 阅读相关代码
6. 阅读相关测试
7. 确认现有 abstraction
8. 开始修改

不得直接看到 issue 后立即写代码。

---

# 6. ExecPlan System

任何满足以下条件的任务都必须创建 ExecPlan：

- 跨越多个 crate/module
- 新增 Provider
- 新增协议
- 修改 Router
- 修改数据库 schema
- 修改 credentials
- 修改 daemon architecture
- 修改 request lifecycle
- 性能敏感修改
- 预计超过一个 coding session

路径：

```text
docs/exec-plans/active/NNN-title.md
```

完成后：

```text
docs/exec-plans/completed/
```

ExecPlan 至少包含：

```text
Goal
Context
Existing behavior
Constraints
Architecture impact
Files expected to change
Implementation stages
Tests
Performance validation
Security considerations
Migration considerations
Known risks
Definition of done
Progress
Decisions
```

Agent 必须持续更新 Progress，而不是任务结束后补写。

---

# 7. Architecture Decision Records

重大设计必须进入：

```text
docs/adr/
```

首批应该建立：

```text
ADR-001-tauri-rust-core.md
ADR-002-daemon-ui-separation.md
ADR-003-snapshot-routing.md
ADR-004-secret-storage.md
ADR-005-provider-capability-model.md
ADR-006-canonical-protocol.md
ADR-007-plugin-isolation.md
ADR-008-request-logging-privacy.md
```

ADR 必须记录：

```text
Context
Decision
Alternatives
Consequences
Status
```

---

# 8. Provider Development Contract

不要把一个 Provider 写成一个巨大的：

```text
provider.rs
```

Provider 必须按能力拆分。

逻辑模型：

```text
ProviderPlugin
 ├─ Discovery
 ├─ Authentication
 ├─ Usage
 ├─ Catalog
 ├─ Transport
 └─ Health
```

Provider 可以只实现其中一部分。

例如：

```text
Kimi Code
Discovery      YES
Authentication YES
Usage          MAYBE
Catalog        YES
Transport      CLI_BRIDGE / future API

OpenCode Go
Discovery      YES
Authentication YES
Usage          YES
Catalog        YES
Transport      HTTP

Command Code
Discovery      YES
Authentication YES
Usage          YES
Catalog        YES
Transport      HTTP
```

绝不能因为一个平台缺少 Usage API，就导致整个 Provider 无法接入。

---

# 9. Capability-First Design

不要写：

```text
if provider == "kimi"
```

而应写：

```text
if provider.capabilities.contains(...)
```

典型 capability：

```text
CHAT_COMPLETIONS
RESPONSES
ANTHROPIC_MESSAGES

STREAMING
TOOLS
VISION
REASONING

USAGE_QUERY
BALANCE_QUERY
RATE_LIMIT_QUERY

OAUTH
API_KEY
LOCAL_SESSION

MODEL_DISCOVERY

DIRECT_HTTP
PERSISTENT_BRIDGE

ZDR
```

任何 Router 行为都应尽可能依赖 capability，而不是 provider 名字。

---

# 10. Canonical Internal Protocol

Gateway 不应该让 Router 理解每一种协议。

建立：

```text
CanonicalRequest
CanonicalResponse
CanonicalStreamEvent
```

流程：

```text
OpenAI request
        ↓
Protocol Adapter
        ↓
CanonicalRequest
        ↓
Router
        ↓
Provider Transport
        ↓
Provider protocol
```

返回同理。

协议转换必须单独测试。

---

# 11. Provider Trust Level

每个 Transport 都必须标记：

```text
OFFICIAL_API
OFFICIAL_SUBSCRIPTION_API
OFFICIAL_CLI
CLI_BRIDGE
PRIVATE_ENDPOINT
WEB_SESSION
LOCAL
```

Router 默认优先稳定 transport。

UI 必须显示 transport 类型。

不得让用户误以为：

```text
Web Cookie Provider
```

与官方 API 具备相同稳定性。

---

# 12. Local Agent Discovery Rules

扫描必须由用户主动发起。

禁止：

- 静默上传配置；
- 自动复制所有 token；
- 扫描浏览器所有 cookie；
- 输出 token 到 log；
- 把 secret 写入 SQLite。

Discovery 可以：

```text
detect executable
read known config metadata
execute safe status/version commands
detect auth source
detect model/provider configuration
```

发现：

```text
Claude Code
Codex
OpenCode
Kimi Code
Command Code
Gemini CLI
```

后先展示：

```text
Detected
Authenticated
Credential type
Provider
Capabilities
Import method
```

用户选择 Import 后才建立 Account。

---

# 13. Secret Handling

SQLite 只允许保存：

```text
secret_ref
credential_type
created_at
updated_at
scope
```

不能保存：

```text
raw_api_key
refresh_token
access_token
cookie
password
```

Secret 应进入 OS credential storage。

如果 Provider credential 已由官方 CLI 管理：

优先保存：

```text
CredentialSource::ExternalManaged
```

而不是复制 token。

---

# 14. Routing Architecture

Router 分为两个循环。

## Control Plane

异步：

```text
health
usage
quota
balance
benchmark
price
models
error statistics
```

计算：

```text
RouteSnapshot
```

## Data Plane

同步：

```text
Request
 ↓
Resolve Profile
 ↓
Capability Filter
 ↓
Constraint Filter
 ↓
Snapshot Lookup
 ↓
Score / Pick
 ↓
Transport
```

不得混用。

---

# 15. Latency Benchmark Rules

记录至少：

```text
DNS
TCP
TLS
TTFT
total latency
output tokens/sec
stream interruption rate
request success rate
```

真实路由主要参考：

```text
TTFT
tokens/sec
error rate
```

测速由两种数据组成：

### Passive

真实请求自动生成统计。

### Active

低频 synthetic probe。

Active Probe 不得高频消耗昂贵 subscription quota。

---

# 16. Latency Tier

每个：

```text
Account + Model + Transport
```

组合称为：

```text
RouteTarget
```

Latency classification 属于 RouteTarget，而不是只属于 Model。

例如：

```text
Kimi K3 via OpenCode Go
```

和：

```text
Kimi K3 via Command Code
```

必须被视为两个独立 RouteTarget。

---

# 17. Routing Profiles

必须原生支持：

```text
realtime
balanced
quality
economy
coding
batch
manual
```

其中 `realtime`：

- 禁止调用 router LLM
- 禁止 remote classifier
- 优先低 TTFT
- 只使用已经 benchmark 的 targets
- 快速 failover
- 不做主动 hedging，除非用户开启

---

# 18. Circuit Breaker

每个 RouteTarget 必须独立维护：

```text
healthy
degraded
cooldown
rate_limited
auth_failed
disabled
```

429：

根据 reset / retry-after 进入 cooldown。

5xx：

指数退避。

401/403：

标记 authentication issue。

不能让一个坏账号拖慢整个 Provider。

---

# 19. Testing Pyramid

Provider 必须具备：

### Unit tests

协议转换、quota parsing、scoring。

### Fixture tests

将 Provider response 保存为 sanitized fixture。

不得保存真实 credentials。

### Contract tests

验证：

```text
model list
authentication
usage
streaming
errors
```

### Integration tests

Mock provider server。

### Live tests

显式 opt-in：

```text
LOADOUT_LIVE_TESTS=1
```

默认 CI 不消耗用户额度。

---

# 20. Performance Gates

任何 Router/Gateway 修改必须运行 benchmark。

至少维护：

```text
route_selection
canonical_conversion
stream_proxy
sqlite_usage_write
snapshot_swap
```

禁止因为增加统计功能明显提高 TTFT。

请求日志必须异步写入。

必要时通过 channel：

```text
request
 ↓
stream immediately
 ↓
async telemetry queue
 ↓
SQLite
```

---

# 21. Database Migration Rules

所有 schema 修改：

- migration only；
- 不得程序启动时“猜测修复”；
- migration 必须 reversible 或明确说明；
- migration 必须有测试；
- 旧版本数据库升级必须进入测试。

---

# 22. UI Development Rule

UI 不允许直接访问 Provider。

错误：

```text
React → OpenRouter API
```

正确：

```text
React
 ↓ IPC
Core
 ↓ Provider Runtime
```

UI 永远只是 control plane client。

---

# 23. Logging Privacy

默认 Request Log 保存：

```text
timestamp
client
virtual model
route
provider
model
latency
tokens
cost
status
error category
```

默认不保存：

```text
prompt
response
tool arguments
file contents
```

Payload logging 必须用户显式开启。

---

# 24. Agent Behaviour During Ambiguity

Agent 应优先：

1. 查看现有 abstraction；
2. 查看官方文档；
3. 查看项目 Provider notes；
4. 查看参考实现；
5. 建立兼容性最好的抽象。

不要因为一个 Provider 特例立即修改公共接口。

如果必须特例：

将其封装在该 Provider 内。

---

# 25. External Reference Policy

允许研究：

```text
LiteLLM
CLIProxyAPI
CliRelay
CodexBar
OpenCode
Kimi CLI
Command Code
官方 Provider SDK
```

但不得未经确认复制：

- GPL/AGPL 代码；
- 不兼容 license 代码；
- undocumented secrets；
- 敏感认证逻辑。

借鉴 architecture 可以。

复制代码必须确认 license。

---

# 26. First-Release Development Sequence

虽然不是 MVP，但开发顺序必须避免 Big Bang。

推荐：

```text
Phase 1
Core runtime
Database
Daemon
Secret store
Gateway skeleton

Phase 2
Canonical protocol
OpenAI compatible provider
Streaming
Usage logging

Phase 3
Provider SDK
Discovery
Account model
Usage model

Phase 4
OpenCode Go
Command Code
OpenRouter
generic OpenAI-compatible

Phase 5
Codex
Claude Code
Kimi Code
Gemini CLI

Phase 6
Benchmark engine
Health
Quota
RouteSnapshot

Phase 7
Realtime / Balanced / Economy / Quality routing

Phase 8
Desktop management UI

Phase 9
Client keys
Advanced logs
Statistics
Backup/export

Phase 10
E2E hardening
Performance optimization
installer/update
recovery
```

每个阶段必须保持软件可以运行。

---

# 27. Definition of Done

一个功能不是“代码写完”即完成。

必须满足：

```text
Implementation
Tests
Documentation
Error handling
Telemetry
Security review
Performance impact checked
Migration handled
UI state handled where applicable
```

Provider 完成还必须满足：

```text
Discovery tested
Auth tested
Catalog tested
Usage state known
Streaming tested
Failure state tested
Rate-limit handling tested
Credential redaction tested
```

---

# 28. Final Development Principle

本项目优化目标的排序：

```text
Correctness
↓
Security
↓
Reliability
↓
Latency
↓
Compatibility
↓
Observability
↓
Convenience
```

但是：

> 不允许用明显增加请求热路径延迟的方式实现 observability、quota tracking 或 dynamic routing。

Loadout 的核心技术特征应该是：

**后台高度动态，前台请求路径极其简单。**
