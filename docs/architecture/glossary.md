# Loadout Glossary

本文件统一 Loadout 的术语、状态与分类。代码、文档、UI 必须使用这里的名称。产品设计文档与 Harness 中不一致的定义，以本文件为准。

---

## 1. Core Entities

| 术语 | 定义 | 备注 |
|---|---|---|
| **Provider** | 一个 AI 服务来源，由一个 Provider 模块实现（如 `opencode-go`、`commandcode`、`openrouter`、`codex`）。 | 中文 UI 可显示为"平台"。代码中统一使用 `Provider`。 |
| **Account** | 用户在某个 Provider 下实际拥有的一个账号或一把 API key。 | 每个 Account 有用户填写的 `alias`，见 ADR-010。 |
| **Credential** | Account 的认证来源：`API_KEY`、`OAUTH`、`EXTERNAL_MANAGED`（由官方 CLI 自行管理，Loadout 不复制）。 | 原文只存 OS secret store，见 ADR-004。 |
| **Model** | 逻辑模型（canonical），如 `kimi-k3`。 | 用于跨 Provider 识别"同一模型"。 |
| **AccountModel** | 某 Account 上可用的一个具体模型，带 `upstream_model_id`、协议、价格。 | |
| **Transport** | 真正发送请求的方式（如 OpenAI-compatible HTTP、Anthropic HTTP）。每个 Transport 有一个 Trust Level。 | |
| **RouteTarget** | `Account + Model + Transport`，路由选择的最小单位。 | 同一模型经不同 Account/Provider 是不同 RouteTarget。 |
| **Pool** | RouteTarget 的**有序**列表。 | 成员是 RouteTarget，不是 Provider。 |
| **Account Sub-pool（子号池）** | 账号的命名子集（代码中 `AccountSet`）。生效时路由表只包含其中账号，效果等同切换账户；未生效即**主号池**（全部账号）。 | 与 Pool 无关：成员是 Account。见 ExecPlan 019。 |
| **Virtual Model** | `loadout/<name>` 形式的模型名，Router 拥有选择权。 | 见 §4。 |
| **Exact Model** | `provider/account/model` 或 `provider/model` 形式的模型名。 | 见 ADR-010。 |
| **RouteSnapshot** | 后台编译出的只读路由表，热路径只读它。 | 见 ADR-003。 |
| **Client** | 调用 Loadout Gateway 的本地程序，持有一把 client key。 | 见 ADR-011。 |
| **Core / Daemon** | `loadoutd.exe`，承载 Gateway、Router、Provider Runtime、Storage。 | 见 ADR-002。 |
| **Control Plane** | 后台循环：health、usage、quota、benchmark、catalog → 生成 RouteSnapshot。 | |
| **Data Plane** | 请求路径：decode → resolve → select → transport → stream。 | |

---

## 2. Provider Drivers

一个 Provider 由最多 **6** 种 driver 组成（产品文档中的 5 种 + Health），均可选：

| Driver | 职责 |
|---|---|
| Discovery | 本机是否安装、是否登录、配置在哪 |
| Auth | API key / OAuth / External-managed session |
| Usage | balance、credits、quota windows、reset、renewal |
| Catalog | 模型列表、context、能力、协议、价格 |
| Transport | `send(request)` |
| Health | Provider 侧状态探测（低频 active probe、状态页等） |

---

## 3. Trust Level 与 Tier

**Trust Level** 是每个 Transport / 数据来源的规范字段（Harness §11）。**Tier** 只是 UI 分组，由 Trust Level 推导：

| Trust Level | Tier | UI 显示 |
|---|---|---|
| `OFFICIAL_API` | A | Official API |
| `OFFICIAL_SUBSCRIPTION_API` | A | Official Subscription API |
| `LOCAL` | A | Local |
| `OFFICIAL_CLI` | B | Official CLI |
| `CLI_BRIDGE` | C | Bridge |
| `PRIVATE_ENDPOINT` | C | Private Endpoint |
| `WEB_SESSION` | D | Web Session |

v1 推理 Transport 默认只使用 Tier A（见 ADR-009）。例外需要逐账号 opt-in：Tier C 的 `codex-subscription`（ADR-012）、Tier D 的 `WEB_SESSION`（ADR-014）。

---

## 4. Latency Tiers & Virtual Models (v1)

**Latency Tier** 属于 RouteTarget，按 TTFT 判定（阈值为默认值，可配置，见 ADR-003）：

| Tier | 中文 | 默认判定 |
|---|---|---|
| `ULTRA_FAST` | 超快 | TTFT P90 < 800 ms |
| `FAST` | 快 | TTFT P90 < 2000 ms |
| `NORMAL` | 普通 | 其余已测量的 target |
| `UNMEASURED` | 未测量 | 样本不足 |

同一 Tier 内按 **TPS**（output tokens/sec）从高到低排序，用户手动排序优先。

统计单位是 account model（账号 + 模型 + 渠道）；同一 account model 的不同协议变体共用统计（ExecPlan 004 §3.1）。P90 以近期样本加权（EWMA），至少 3 个样本才有 Tier；真实请求样本足够时不混用 probe 样本。Tier 有 ±10% 死区，TPS 排序变化需超过 20% 且持续 3 分钟，避免频繁切换。用户可用 `loadout model tier` 固定某个模型的 Tier。

v1 内置 Virtual Models：

| 名称 | 候选范围 |
|---|---|
| `loadout/realtime` | 仅 `ULTRA_FAST` |
| `loadout/fast` | `ULTRA_FAST` → `FAST` |
| `loadout/default` | 全部可用 target（含 `UNMEASURED`），按 Tier 顺序 |
| `loadout/<pool>` | 用户自定义 Pool：`manual` 按用户给定顺序，`latency` 按上面的 Tier / TPS 规则 |

保留名称（v1 不实现，不得被用户 Pool 占用）：`balanced`、`quality`、`economy`、`coding`、`batch`、`auto`、`manual`、`throughput`。

**Session affinity**：请求带 `X-Loadout-Session-ID` 时，同一会话优先继续使用上次的 account model，直到它不再 eligible（仅内存，2 小时过期）。

---

## 5. Status Model

状态分两层，避免产品文档与 Harness 中的两套定义混用。

### 5.1 Account Status（账号层，来自 Auth / Usage）

| 状态 | 含义 |
|---|---|
| `ACTIVE` | 认证有效 |
| `AUTH_REQUIRED` | 认证失效或未登录 |
| `QUOTA_EXHAUSTED` | 所有额度窗口均已耗尽，直到最早的 reset |
| `DISABLED` | 用户手动停用 |
| `UNKNOWN` | 尚未检查 |

### 5.2 RouteTarget Health（目标层，Circuit Breaker）

| 状态 | 进入条件 | 是否可选 |
|---|---|---|
| `HEALTHY` | 近期成功 | 是 |
| `DEGRADED` | 最近至少 4 次结果中失败 ≥ 25%（当前仍为 HEALTHY） | 是（同 Tier 内排在后面） |
| `COOLDOWN` | 5xx / timeout，指数退避中 | 否，到期自动半开 |
| `RATE_LIMITED` | 429，按 retry-after / reset 冷却 | 否，到期恢复 |
| `QUOTA_EXHAUSTED` | 上游返回额度耗尽，或 Account 为 `QUOTA_EXHAUSTED` | 否，到 reset 恢复 |
| `AUTH_FAILED` | 401 / 403，或 Account 为 `AUTH_REQUIRED`。首次 401/403 封锁 1 小时后半开重试；连续第二次被拒，或上游表明账号被停用 / 封禁 / 暂停，进入**终态**（无自动恢复，`unavailable_until_ms` 为空），直到 `account.check` 成功、更换凭证、重新登录或用户重新启用账号（ExecPlan 014） | 否，需重新认证 |
| `OFFLINE` | 连接失败 / DNS 失败 | 否，退避后探测 |
| `DISABLED` | 用户停用 target 或其 Account | 否 |
| `UNKNOWN` | 尚无数据 | 仅 `loadout/default` 与 exact model |

UI 标签映射：`HEALTHY`→Healthy，`DEGRADED`→Degraded，`RATE_LIMITED`/`COOLDOWN`→Rate Limited / Cooling Down，`AUTH_FAILED`→Auth Required，`OFFLINE`→Offline，`UNKNOWN`→Unknown。

---

## 6. Usage Data Confidence

所有 quota / balance 数据都带来源与可信度：

| 字段 | 取值 |
|---|---|
| `source` | `OFFICIAL_API`、`OFFICIAL_CLI`、`LOCAL_FILES`、`PRIVATE_ENDPOINT`、`ESTIMATED` |
| `confidence` | `EXACT`、`ESTIMATED`、`UNKNOWN` |
| `observed_at` | 观测时间；超过 staleness 阈值视为 `UNKNOWN` |

Router 对 `UNKNOWN` 额度不做预判，只依据上游实际返回（429 / quota 错误）触发切换。

---

## 7. Latency Metrics

| 指标 | 定义 |
|---|---|
| **TTFT** | Gateway 发出上游请求 → 收到第一个输出内容 token（不含纯元数据事件） |
| **TPS** | 首 token 之后的输出 token 数 ÷ 首 token 到结束的时间 |
| **Total latency** | 请求进入 Gateway → 响应结束 |
| **Gateway overhead** | 请求进入 Gateway → 发出上游请求（选路与转换开销） |

推理模型的隐藏 thinking 时间计入 TTFT。

---

## 8. Error Categories

上游错误统一归类（产品文档 §40）：

`AUTH` · `RATE_LIMIT` · `QUOTA` · `UPSTREAM_TIMEOUT` · `UPSTREAM_5XX` · `UNSUPPORTED_CAPABILITY` · `MODEL_NOT_FOUND` · `TRANSPORT_UNAVAILABLE` · `NO_ELIGIBLE_TARGET` · `INVALID_REQUEST`

同时保留脱敏后的上游原始错误。
