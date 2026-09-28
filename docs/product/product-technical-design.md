# Loadout Product & Technical Design v0.1

> **Loadout — All your AI, one endpoint.**

## 1. Product Definition

产品名：

# Loadout

主 slogan：

> **All your AI, one endpoint.**

产品定位：

> 本地优先的 Personal AI Control Plane & Gateway。

Loadout 统一管理用户已有的：

- AI subscriptions
- coding-agent subscriptions
- API credits
- OAuth sessions
- API Keys
- local models
- compatible gateways

同时在：

```text
127.0.0.1
```

暴露统一 API，让其他软件无需理解不同 Provider。

“Loadout”这个名字表达的是：

> 用户已经拥有多种 AI 能力，Loadout 负责把这些能力组织成可选择、可组合、可动态调度的个人 AI 配装。

---

# 2. Core User Problem

当前 AI 使用环境越来越碎片化。

一个用户可能同时拥有：

```text
ChatGPT / Codex
Claude / Claude Code
Gemini
Kimi Code
OpenCode Go
Command Code
OpenRouter
DeepSeek API
local Ollama
```

每个平台存在不同：

```text
价格
余额
5h limit
weekly limit
monthly limit
reset time
模型
接口协议
速度
稳定性
登录方式
```

用户实际拥有大量 AI compute，但这些资源之间彼此隔离。

Loadout 的目标：

```text
Fragmented AI Resources
        ↓
Unified Compute Pool
        ↓
Policy-driven Routing
```

---

# 3. Product Principles

## 3.1 Local First

默认所有：

```text
credentials
routing
usage history
request metadata
```

只保存在本机。

---

## 3.2 Subscriptions Are First-Class

不能将 Subscription 当作劣化版 API Key。

数据模型必须直接理解：

```text
Subscription
Quota Window
Reset
Credits
Included Usage
```

---

## 3.3 Low Latency Is First-Class

不能实现：

```text
Request
 ↓
查询所有账号额度
 ↓
测速
 ↓
AI分析
 ↓
选择模型
```

必须实现：

```text
BACKGROUND
usage + latency + health + price
              ↓
         RouteSnapshot

REQUEST
              ↓
        local lookup
              ↓
           route
```

---

# 4. Current Ecosystem Compatibility

几个重要平台应作为首版核心 Provider：

## OpenCode Go

OpenCode Go 应作为 Direct Provider 接入。

```text
OpenCode Go
→ Direct Provider
→ High-confidence integration
```

---

## Command Code

Command Code 应优先通过其 OpenAI / Anthropic compatible Provider API 接入。

同时需要考虑它作为客户端连接 Loadout：

```text
Command Code
→ Loadout
```

以及 Loadout 将 Command Code 作为上游：

```text
Loadout
→ Command Code
```

---

## Kimi Code

Kimi Code 需要区分：

```text
Discovery
Authentication
Usage
Transport
```

支持扫描本机安装及登录状态。

认证发现与推理 Transport 不得耦合。

---

## Claude Code

支持：

```text
Claude CLI discovery
OAuth/account status
usage discovery
optional subscription transport
official API transport
```

---

## Codex

优先调用 Codex 官方已有的本机接口或 app-server/RPC 风格能力读取 account/rate-limit 信息。

Credential 优先保持由 Codex 自身管理，而不是复制 refresh token。

---

# 5. Overall Architecture

```text
                    ┌─────────────────────┐
                    │   Desktop React UI  │
                    └─────────┬───────────┘
                              │ IPC
                    ┌─────────▼───────────┐
                    │    Loadout Core     │
                    │       Rust          │
                    └─────────┬───────────┘
                              │
     ┌────────────────────────┼────────────────────────┐
     │                        │                        │
     ▼                        ▼                        ▼

Account Runtime          Router Engine          Gateway Server

     │                        │                        │
     ▼                        ▼                        ▼

Provider Drivers       RouteSnapshot        OpenAI / Anthropic
                                                compatible
     │
     ▼
┌──────────────────────────────────────────────────────┐
│                     Providers                        │
│ OpenCode Go | Command Code | Codex | Claude | Kimi │
│ Gemini | OpenRouter | API | Ollama | Custom        │
└──────────────────────────────────────────────────────┘
```

---

# 6. Core Domain Model

最重要的几个概念：

## Provider

例如：

```text
OpenAI
Anthropic
OpenCode Go
Command Code
Kimi
OpenRouter
```

---

## Account

用户实际拥有的一个账号。

例如：

```text
OpenCode Go / account A
```

---

## Credential

账号认证来源。

例如：

```text
API_KEY
OAUTH
EXTERNAL_CLI_SESSION
```

---

## Model

逻辑模型：

```text
kimi-k3
deepseek-v4-flash
gpt-5.6-sol
```

---

## RouteTarget

真正可以执行一个请求的对象：

```text
Account
+
Model
+
Transport
```

例如：

```text
Command Code Account
+ Kimi K3
+ OpenAI Compatible HTTP
```

和：

```text
OpenCode Go
+ Kimi K3
+ OpenAI Compatible HTTP
```

是两个 RouteTarget。

Router 实际选择的是 RouteTarget，而不是抽象 Model。

这是低延迟与多渠道路由能够成立的关键。

---

# 7. Provider Plugin / Driver Model

Provider Plugin 不应理解为用户手动安装的插件，而更接近：

> Windows 设备 Driver。

Loadout 定义一套标准接口，每个平台负责告诉系统：

```text
我怎么被发现
我怎么登录
怎么查用量
有什么模型
怎么发送请求
现在是否健康
```

一个 Provider 可以拆为五种 driver。

## Discovery Driver

负责：

```text
本机有没有安装？
当前有没有登录？
配置在哪里？
```

---

## Auth Driver

负责：

```text
OAuth
API Key
Existing Session
```

---

## Usage Driver

负责：

```text
balance
credits
5h
weekly
monthly
reset
renewal
```

---

## Catalog Driver

负责：

```text
models
context
vision
tools
protocol
price
```

---

## Transport Driver

负责真正：

```text
send(request)
```

所以：

```text
Kimi
```

即使暂时只有：

```text
Discovery ✓
Auth ✓
Usage ✓
Transport ?
```

账户管理仍然完全可以工作。

未来 Transport 更新即可。

---

# 8. Local Agent Scanner

设置页提供：

# Scan My AI Tools

扫描：

```text
Codex
Claude Code
Gemini CLI
OpenCode
Kimi Code
Command Code
```

输出：

| Tool | State | Auth | Import |
|---|---|---|---|
| Codex | Logged in | OAuth | Import |
| Claude Code | Logged in | OAuth | Import |
| Kimi Code | Logged in | OAuth | Import |
| OpenCode | Installed | Config | Inspect |
| Command Code | Logged in | API/session | Import |

点击：

```text
Import
```

后才真正创建账号。

---

# 9. Account Dashboard

首页核心卡片：

```text
Codex Pro

● Healthy

5h
████████░░ 78%
reset 01:44

Weekly
██████░░░░ 58%

Current latency
TTFT 820 ms

Routing
Realtime ✓
Balanced ✓
```

另一类：

```text
Command Code GOAT

Credits
$38.27 / $70

Renew
Oct 14

Kimi K3
TTFT 640 ms

DeepSeek V4 Flash
TTFT 310 ms
```

---

# 10. Gateway

默认：

```text
http://127.0.0.1:8317
```

支持：

```text
POST /v1/chat/completions
POST /v1/responses
POST /v1/messages

GET /v1/models
```

未来：

```text
embeddings
images
audio
```

独立扩展。

---

# 11. Client Authentication

即使 localhost 也使用：

```text
LOADOUT_API_KEY
```

支持创建：

```text
Trading App
OpenCode
VS Code
Personal Script
Testing
```

不同 local client keys。

每个 key 可以配置：

```text
allowed profiles
allowed models
daily spend
request limits
logging policy
```

---

# 12. Virtual Models

核心 UX：

```text
loadout/realtime
loadout/balanced
loadout/quality
loadout/economy
loadout/coding
loadout/batch
loadout/auto
```

此外支持 exact model：

```text
commandcode/kimi-k3
opencode-go/deepseek-v4-flash
openrouter/xxx
```

---

# 13. Latency Pools

这部分是首版核心，而不是优化项。

Router 自动将 RouteTarget 分类：

```text
Ultra Fast
Fast
Normal
Slow
Unknown
```

用户可以创建逻辑 Pool：

```text
Low Latency Pool

CommandCode / DeepSeek V4 Flash
OpenCode Go / MiMo
OpenRouter / Gemini Flash
```

注意：

> Pool 的成员应当是 RouteTarget，而不是 Provider。

因为同一个模型在两个不同渠道上的速度可能完全不同。

---

# 14. Benchmark System

## Passive Benchmark

真实请求自动记录：

```text
connection time
TTFT
total duration
input tokens
output tokens
output tokens/sec
status
```

Passive 数据权重最高。

---

## Active Benchmark

后台可定时发送非常小的标准 prompt。

例如：

```text
respond exactly with "OK"
```

用于判断：

```text
availability
TTFT
connection
```

频率需要低。

避免浪费额度。

---

# 15. Latency Statistics

不能只看平均值。

维护：

```text
P50 TTFT
P90 TTFT
P95 TTFT

P50 tokens/sec
P95 total latency

success rate
429 rate
5xx rate
```

近期数据通过：

```text
EWMA
```

增加权重。

---

# 16. Router Design

Router 分三层。

## Layer 1 — Eligibility

硬条件：

```text
context enough?
supports tools?
supports vision?
supports protocol?
account enabled?
quota available?
transport healthy?
```

不符合直接淘汰。

---

## Layer 2 — Pool

根据：

```text
loadout/realtime
```

进入 realtime candidate pool。

例如：

```text
12 RouteTargets
↓
4 realtime candidates
```

---

## Layer 3 — Score

从候选池选择。

基本：

```text
Score =
Quality
+ Latency
+ Reliability
+ Quota
+ Cost
+ Affinity
```

但不同 Profile 使用不同权重。

---

# 17. Realtime Routing

例如：

```text
Latency       55%
Reliability   20%
Quota         15%
Quality       10%
Cost           0%
```

这里只是默认模板。

核心不是具体数字，而是：

> realtime profile 不能为了省几分钱牺牲明显 TTFT。

---

# 18. Balanced Routing

例如：

```text
Quality       30%
Latency       25%
Quota         20%
Reliability   15%
Cost          10%
```

---

# 19. Economy Routing

例如：

```text
Cost          45%
Quota         25%
Latency       15%
Quality       10%
Reliability    5%
```

---

# 20. Quality Routing

例如：

```text
Quality       60%
Reliability   15%
Latency       10%
Quota         10%
Cost           5%
```

---

# 21. Quota-Aware Routing

不要只看：

```text
remaining = 50%
```

应该考虑：

```text
50% remaining
reset in 20 min
```

与：

```text
50% remaining
reset in 6 days
```

完全不同。

定义：

```text
remaining_budget
time_to_reset
recent_burn_rate
predicted_demand
```

估计：

```text
predicted_usage_until_reset
```

然后：

```text
pressure =
predicted_usage_until_reset
/
remaining_budget
```

pressure 越高，越应该保护该账号。

---

# 22. RouteSnapshot

所有后台状态最终编译成：

```text
RouteSnapshot
```

概念：

```text
realtime:
  coding:
    - target_31 score 0.94
    - target_18 score 0.91
    - target_44 score 0.84

balanced:
  coding:
    - target_18 score 0.95
    - target_31 score 0.89
```

请求热路径不需要查询数据库。

只需：

```text
snapshot.load()
```

选择。

可以使用：

```text
ArcSwap
```

实现近似 lock-free 热更新。

---

# 23. Request Lifecycle

```text
Client
 ↓
localhost:8317
 ↓
Auth
 ↓
Protocol Decode
 ↓
CanonicalRequest
 ↓
Resolve Virtual Model
 ↓
Eligibility Filter
 ↓
RouteSnapshot
 ↓
RouteTarget
 ↓
Provider Transport
 ↓
Streaming Response
```

同时：

```text
            └──── async telemetry
                     ↓
                  SQLite
```

---

# 24. Failover

请求失败：

```text
Target A
 ↓
429
 ↓
mark cooldown
 ↓
Target B
```

不要重新跑完整 dynamic routing。

Snapshot 中已经有：

```text
candidate[1]
candidate[2]
candidate[3]
```

直接选择下一个。

这样 Failover 本身仍然很快。

---

# 25. Session Affinity

Agent workloads 往往连续调用。

支持：

```text
X-Loadout-Session-ID
```

同一 session 尽量：

```text
stick to same RouteTarget
```

原因：

```text
provider prompt cache
latency stability
behavior consistency
```

除非：

```text
health failure
quota pressure
explicit re-route
```

---

# 26. Auto Routing

`loadout/auto` 与其他 profile 不同。

它可以做任务类型判断：

```text
coding
reasoning
simple
vision
long-context
tool-heavy
```

但必须有两种模式。

### Fast Auto

本地规则/轻量分类器。

目标：

```text
<1 ms ~ few ms
```

### Smart Auto

允许：

```text
local ML classifier
```

未来甚至允许独立 Router model。

但必须是用户主动开启。

`realtime` 永远不得依赖 Smart Auto。

---

# 27. Model Quality

Quality 不能假装可以完全自动获得。

首版来源：

```text
manual baseline
known benchmark metadata
user preference
task-specific rating
```

以后可以从用户自己的请求获得：

```text
retry rate
manual rating
provider switching
accepted outputs
```

形成个性化质量评分。

但首版不要搞复杂在线学习。

---

# 28. Database

建议 SQLite WAL。

## providers

```text
id
slug
name
trust_level
created_at
```

---

## accounts

```text
id
provider_id
display_name
account_type
status
enabled
subscription_plan
renew_at
created_at
```

---

## credentials

只保存 metadata：

```text
id
account_id
credential_type
secret_ref
source
expires_at
```

---

## quota_windows

```text
id
account_id
scope
limit_value
used_value
remaining_value
unit
window_seconds
reset_at
observed_at
```

---

## balances

```text
account_id
currency
balance
included_credit
topup_credit
observed_at
```

---

## models

```text
id
canonical_name
display_name
context_window
capabilities_json
```

---

## account_models

```text
id
account_id
model_id
upstream_model_id
protocol
input_price
output_price
enabled
```

---

## route_targets

```text
id
account_model_id
transport_id
status
priority
```

---

## latency_samples

```text
route_target_id
timestamp
ttft_ms
total_ms
output_tokens
tokens_per_second
success
```

---

## health_state

```text
route_target_id
status
failure_count
last_success_at
cooldown_until
last_error_type
```

---

## route_profiles

```text
id
name
policy_json
```

---

## request_events

```text
id
client_id
timestamp
requested_model
profile
route_target_id
status
input_tokens
output_tokens
ttft_ms
duration_ms
estimated_cost
```

---

## route_decisions

可选 debug：

```text
request_id
selected_target
candidate_summary
decision_reason
snapshot_version
```

默认只短期保留。

---

## clients

```text
id
name
secret_ref
enabled
policy_id
created_at
```

---

# 29. Secrets

真实 credential：

```text
API keys
OAuth tokens
refresh tokens
cookies
```

全部存在操作系统 secret storage。

SQLite 只：

```text
secret_ref
```

---

# 30. Management API

Desktop UI 通过 IPC 优先。

同时可以提供 localhost-only management API：

```text
GET /_loadout/accounts
GET /_loadout/routes
GET /_loadout/usage
GET /_loadout/health
GET /_loadout/benchmark
```

默认不暴露 LAN。

---

# 31. Compatibility Architecture

平台分四类。

## Tier A — Official API

例如：

```text
OpenAI API
Anthropic API
OpenRouter
Command Code Provider
OpenCode Go
```

优先级最高。

---

## Tier B — Official CLI / OAuth

例如：

```text
Codex
Claude Code
Kimi Code
Gemini CLI
```

可以扫描。

Transport 是否开放需要单独判断。

---

## Tier C — Bridge

例如通过：

```text
persistent CLI worker
CLIProxy-compatible transport
```

运行。

UI 显示：

```text
Bridge
```

---

## Tier D — Web Session

最低稳定等级。

首版可以设计接口，但不应该作为产品核心。

---

# 32. OpenCode Compatibility

需要同时理解两件不同的事。

### OpenCode 是客户端

它应该能够：

```text
OpenCode
 ↓
Loadout localhost API
```

所以 Loadout 是它的 Provider。

### OpenCode Go 是上游

```text
Loadout
 ↓
OpenCode Go
```

所以它同时又是 Loadout Provider。

这是 Loadout 很重要的产品特性：

> 一个 ecosystem 可以同时存在于 Gateway 的上下游。

---

# 33. Command Code Compatibility

同理：

```text
Command Code CLI
 ↓
Loadout
```

同时：

```text
Loadout
 ↓
Command Code Provider API
```

这应该成为首版完整验证的重点案例之一。

---

# 34. Usage Center

展示：

```text
Subscription Spend
Equivalent API Value
Actual API Spend
Included Usage Consumed
Requests
Tokens
```

尤其可以提供一个非常有趣的指标：

# Subscription Value Captured

例如：

```text
Command Code subscription
Monthly price      $10
Equivalent API use $46.32

Value captured
4.63×
```

帮助用户判断：

```text
这个订阅到底值不值得续。
```

---

# 35. Renewal Management

账户显示：

```text
price
billing cycle
next renewal
auto renew
credit reset
quota reset
```

注意：

```text
renewal
```

和：

```text
quota reset
```

必须是两个字段。

---

# 36. Dashboard

建议首屏：

```text
Loadout                              Gateway ●

All your AI, one endpoint.

Realtime Pool
P95 TTFT       720 ms
Success        99.3%
Targets        5

Accounts
─────────────────────────────────

Codex
5h       38%
Weekly   61%

Claude
5h       76%
Weekly   52%

OpenCode Go
5h       $8.30 / $12
Weekly   $17 / $30

Command Code
Credit   $41 / $70

─────────────────────────────────

Today

Requests     2,134
Tokens       18.2M
API Spend    $1.84
Sub Value    $17.91
```

---

# 37. Routes Page

用户可以看到：

```text
loadout/realtime

1 Command Code / DeepSeek V4 Flash
  TTFT 320ms
  success 99.8%

2 OpenCode Go / MiMo
  TTFT 410ms
  success 99.2%

3 OpenRouter / Gemini Flash
  TTFT 590ms
```

允许：

```text
drag priority
ban model
ban provider
minimum quality
maximum cost
maximum latency
```

---

# 38. Manual Pinning

任何应用可以被配置：

```text
Trading System
→ realtime

Codex
→ coding

Batch analyzer
→ economy
```

用户不需要每个 request 写复杂 header。

Client 可以绑定默认 profile。

---

# 39. Custom Headers

高级使用：

```text
X-Loadout-Profile: realtime
X-Loadout-Session-ID: ...
X-Loadout-No-Failover: true
X-Loadout-Target: ...
```

标准 SDK 不认识这些 header 也没有关系。

---

# 40. Error Model

需要将上游错误标准化：

```text
AUTH
RATE_LIMIT
QUOTA
UPSTREAM_TIMEOUT
UPSTREAM_5XX
UNSUPPORTED_CAPABILITY
MODEL_NOT_FOUND
TRANSPORT_UNAVAILABLE
```

同时保留 sanitized upstream error。

---

# 41. Health

Health 不是：

```text
ping 成功 = healthy
```

而是综合：

```text
authentication
recent requests
429
5xx
latency anomaly
stream interruption
```

状态：

```text
Healthy
Degraded
Rate Limited
Auth Required
Offline
Unknown
```

---

# 42. Smart Quota Preservation

一个很实用的策略：

如果：

```text
Claude
20% remaining
reset 4 days
```

而：

```text
Command Code
80%
reset 3 hours
```

Balanced Router 应更积极消耗后者。

即：

> 用掉快要刷新的额度，而保存长周期稀缺额度。

这应该成为 routing score 的核心功能之一。

---

# 43. No Hidden Downgrade

如果用户请求：

```text
exact model
```

默认不得偷偷换其他模型。

只有请求：

```text
loadout/*
```

这种 virtual model 时，Router 才拥有模型选择权。

Exact model failover 可以：

```text
same model
different provider
```

但：

```text
different model
```

需要用户开启。

---

# 44. Privacy

请求正文默认：

```text
not persisted
```

数据库保存 metadata。

用户显式开启 Debug Payload Logging 时：

必须显示警告。

日志支持：

```text
24h
7d
30d
custom
```

自动清理。

---

# 45. First Release Scope

首版不是 MVP。

首版目标至少包含：

### Runtime

```text
daemon
tray
desktop UI
auto start
local gateway
```

### Protocol

```text
OpenAI Chat
Responses
Anthropic Messages
streaming
tool calls
```

### Provider

至少：

```text
Generic OpenAI compatible
OpenRouter
OpenCode Go
Command Code
Codex
Claude Code
Kimi Code
Gemini
local compatible provider
```

并允许 Provider 在能力上不完全对称。

### Management

```text
account scan
manual import
usage
quota
balance
renewal
model catalog
health
```

### Routing

```text
manual
realtime
balanced
economy
quality
coding
failover
quota-aware routing
latency-aware routing
session affinity
```

### Observability

```text
request logs
latency
TTFT
TPS
cost
provider health
```

### Security

```text
OS secret store
client API keys
localhost management
log redaction
```

---

# 46. Explicit Non-Goals for v1

不优先：

```text
public SaaS
multi-user cloud gateway
selling API access
team billing
enterprise RBAC
remote hosted gateway
marketplace
```

这些会把项目带向传统 API 中转站/网关的竞争方向。

Loadout v1 保持：

> Personal Desktop AI Infrastructure。

---

# 47. Success Criteria

技术成功：

```text
Router P95 local overhead < 5ms target
Gateway stable streaming
No secret leakage
Provider failure isolation
Clean failover
Reliable usage reporting
```

用户成功：

```text
打开一个桌面软件
→ 看见自己所有 AI 资源

其他程序只配置一个 endpoint
→ 能使用全部 AI 资源

低延迟任务
→ 自动使用最快且健康的线路

一般任务
→ 自动兼顾额度、质量和价格

某个平台限流
→ 自动切换

月底
→ 清楚知道每个订阅到底用了多少、值多少钱
```

---

# 48. Product Thesis

Loadout 最终不是：

```text
AI API Proxy
```

而应该被理解为：

# Local AI Resource Scheduler

它管理的不是一堆 URL。

它管理的是：

```text
AI compute capacity
```

这些 capacity 恰好来自：

```text
subscriptions
API credits
local models
OAuth sessions
aggregators
```

一旦采用这个抽象，账户监控、额度保护、低延迟池、成本调度、动态路由都会成为同一个系统里的自然组成部分，而不是后期拼上的功能。

---

# 49. Brand Identity

## Name

**Loadout**

这个名字表达：

- 用户拥有自己的 AI 能力库存；
- 不同任务可以采用不同 AI 配装；
- Route Profile 本质上就是不同 Loadout；
- Provider、Model、Account、Transport 可以组合成不同执行路径；
- 软件不是购买或出售模型，而是在组织用户已经拥有的能力。

## Primary Slogan

> **All your AI, one endpoint.**

该 slogan 应优先用于：

- README
- 官网首屏
- About 页面
- 安装器/首次启动
- GitHub repository description

## Product Language

Loadout 内部推荐统一采用以下命名：

```text
Loadout
Route
RouteTarget
Profile
Pool
Provider
Account
Model
Transport
Gateway
```

虚拟模型命名：

```text
loadout/realtime
loadout/balanced
loadout/quality
loadout/economy
loadout/coding
loadout/batch
loadout/auto
```

这样产品名称本身和路由模型形成一致的语义体系。
