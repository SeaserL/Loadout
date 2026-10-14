# ADR-014: Opt-in Browser Session Transport (`WEB_SESSION`)

- **Status**: Accepted（2026-10-15，项目所有者确认）
- **Date**: 2026-10-15
- **Amends**：ADR-009 §1、§2（Transport 范围、Web session 禁令）；ADR-007 §2（受管浏览器进程）；ADR-004 §1（浏览器 profile 中的 session cookie）

## Context

项目所有者在 2026-10-15 提出设想：用浏览器控件操作 AI 网页端（以 ChatGPT 为例），把"在输入框输入内容 → 捕获回答"这一来回当作一次 API 调用，最好能在无头状态下运行。

同日补充：

- 如果 headless 的使用与开发风险更高，可以接受正常的浏览器窗口；
- 主要使用场景是把现有的手工流程自动化："顾问回路"（Advisor loop）：
  1. 把 coding agent 的输出复制给网页端；
  2. 等待网页端给出建议；
  3. 让网页端直接写出下一轮给 agent 的 prompt；
  4. 把该 prompt 交给 agent 继续工作。

这个场景的特点：

- 通常只有一段持续的网页对话；
- 请求频率低（每轮间隔为 agent 的工作时长）；
- 用户本来就在看这个对话；
- 对话保存在上游历史中正是用户想要的。

项目所有者同日确认：

- 顾问回路并入集成 API 的设计：本地程序通过 API 调用"顾问模型"，网页端的输出就等同于该模型生成并返回的内容；
- Loadout 只负责输入输出，执行与审核不属于它的分工；
- 目标站点为 ChatGPT 网页；做完风险确认之后允许接入。

### 社区做法（2026-10-15 调研，许可证均未逐个核实）

| 项目 | 做法 |
|---|---|
| PhantomAPI（mrshibly） | FastAPI + Playwright 无头操作 ChatGPT 免费网页版，提供 OpenAI 兼容 `/v1`，用提示词模拟 tool calling |
| mse_ai_api | Playwright 驱动独立 Chrome，注入提示词强制输出 JSON 以映射 function calling |
| unofficial-chatgpt-api（taranjeet） | 打开 Chromium，解析 HTML 获取回答；已停止维护 |
| AIstudioProxyAPI（CJackHwang） | Playwright + Camoufox 操作 Google AI Studio 网页，依靠指纹伪装规避反爬 |
| WebAI2API（foxhui，MIT） | Camoufox，把多个网页端转为 OpenAI 兼容接口 |
| gpt4free（GPL-3.0） | 用浏览器取得 cookie / HAR 与反爬令牌，之后改用 HTTP 直连 |
| chat2api（lanqian528） | 不用浏览器，逆向网页后端 HTTP 接口，多账号轮换 |

共同点：能长期可用的项目，几乎都依赖指纹伪装（Camoufox、nodriver 等）或逆向实现站点的反爬令牌。

### 约束与风险

- **服务条款**：OpenAI Terms of Use 在 "What you cannot do" 中禁止以自动化或程序化方式提取数据或 Output（2026-10-15 读取）。这比 ADR-012 的 Codex 情况更明确：Codex 走的是官方产品自己的后端接口，网页自动化则直接落在禁止条款里。其他站点需要逐个核对。
- **反爬**：ChatGPT 等站点使用 Cloudflare Turnstile、Sentinel（proof-of-work / chat-requirements 令牌）等机制，纯 headless 浏览器容易被识别。社区的应对方式正是 ADR-012 §2 拒绝的那类"规避访问控制"手段。
- **协议还原度**：网页端没有真正的 system prompt，也没有 sampling 参数、stop、token usage 和原生 tool calling。网页对话有状态，API 请求无状态。
- **性能**：TTFT 通常为数秒；一个标签页同一时间只能处理一个请求；每个浏览器实例占数百 MB 内存。
- **脆弱性**：前端改版就可能失效，需要持续维护。
- **Secret 与隐私**：浏览器 profile 中的 session cookie 本身就是凭证（ADR-004）；对话会留在上游的历史记录里。这对通用 API 用法与 ADR-008 的精神冲突，但在顾问回路场景下正是用户想要的。
- **已有替代的局限**：`codex-subscription`（ADR-012）已经能用 ChatGPT 套餐额度调用 Codex 模型，协议还原度和稳定性都更好。但顾问回路需要的是网页端本身：用户可见、可接管的对话历史，以及网页端提供的模型与对话上下文。这些 `codex-subscription` 提供不了。

## Decision

### 1. 范围：新的 Transport 类别，默认关闭

- 新增 Transport 类别，Trust Level 为 `WEB_SESSION`（Tier D，glossary §3）。这是 v1 中最低的可信等级。
- **两层开关**：
  1. 全局开关"启用 Web Session Transport"，默认关闭，关闭时不加载任何相关 Provider 的浏览器；
  2. 逐账号 opt-in，沿用 ADR-012 §1 的两步确认。
- **每个目标站点一个 Provider 模块**（slug 形如 `web-<site>`），站点特例只写在该模块内部（ADR-005）。公共部分（浏览器池、CDP 客户端、流解析框架）放在共享 crate 中，不出现 `if provider == …`。
- **逐站点准入**（见 §9）。首个站点为 ChatGPT 网页（Provider `web-chatgpt`），已由项目所有者确认。
- **主要场景**是顾问回路（§6）：低频、单段持续对话、用户可见。默认值都按这个场景设定。通用的无状态 API 用法同样支持，但不是优化重点。

### 2. 不做指纹伪装与反检测（硬性原则）

延续 ADR-012 §2：这些手段是在规避站点的访问控制，不是"警告 + 二次确认"能兜底的风险。Loadout **不实现**以下任何一项：

- 使用打过反检测补丁的浏览器或驱动（Camoufox、nodriver、undetected-chromedriver、puppeteer-extra-stealth 等）；
- 修改或隐藏自动化特征（`navigator.webdriver`、CDP 痕迹、headless 标识），或把 User-Agent 伪装成另一个浏览器；
- TLS / JA3 / HTTP2 指纹伪装；
- 自动解 CAPTCHA / Turnstile，或接入打码服务；
- 在页面之外自行生成站点的反爬令牌（Sentinel、proof-of-work、Arkose 等），或绕过浏览器直接调用网页后端接口（chat2api 的做法）；
- 为规避频率限制而轮换代理 IP、自动注册账号，或在账号之间轮换来绕过单账号额度。

允许：

- 使用**未修改的**浏览器（Windows 自带的 Microsoft Edge），由页面自己的脚本照常运行，包括它自己的反爬检测；
- **默认使用正常的可见浏览器窗口**（可最小化，可被用户随时查看和接管）。它与用户手动使用最接近：被反爬拦截的概率更低，也不需要处理 headless 特有的检测问题，开发与排错更简单；
- headless 模式：只作为可选配置，且不得掩盖 headless 特征。被站点拒绝即视为该站点在 headless 下不可用；
- 遇到挑战页或登录失效时：把账号标记为 `needs_attention`，停止路由，在 UI 中提示用户打开该账号的浏览器窗口手动处理。Loadout 不代为点击或填写。

**接受的后果**：一些站点在不伪装的前提下可能根本无法使用，或只能在有界面模式下使用。这是本原则的预期结果，不构成例外的理由。

### 3. 浏览器与进程模型

- **浏览器**：系统自带的 Microsoft Edge（Windows 10/11 预装，基于 Chromium）。daemon 通过 CDP（Chrome DevTools Protocol）直接驱动，不引入 Node.js / Python sidecar。CDP 客户端库的选型和许可证核对放在 ExecPlan 中完成。
- **窗口**：默认是普通的可见窗口。窗口最小化或被遮挡时，Chromium 默认会节流后台页面，导致流式输出停顿。因此启动时关闭后台节流（Chromium 标准启动参数）。这些参数不改变浏览器对站点呈现的特征，不属于 §2 禁止的范围。
- **专用 profile**：每个账号一个独立的 profile 目录（`%LOCALAPPDATA%\Loadout\browser-profiles\<account-id>`）。**不读取、不复用**用户日常浏览器的 profile 或 cookie（保持 ADR-009 §2 与 Harness §12 的禁令）。
- **常驻浏览器池**：每个已 opt-in 的账号对应一个常驻浏览器实例，由后台任务启动、健康检查，崩溃后按退避重启。禁止每个请求启动浏览器（Harness §2.4）。浏览器进程的生命周期与 daemon 绑定，daemon 退出时随之结束。具体的进程托管方式在 ExecPlan 中确定，并遵守 `loadout_platform::spawn_detached` 的约定。
- **热路径**：选路只读取 RouteSnapshot 中该 target 的原子状态（就绪 / 忙 / 需要处理），不启动浏览器，也不检测登录状态（Harness §2.2）。浏览器操作只发生在选路之后，作为上游 Transport 执行。
- **隔离**（修订 ADR-007 §2）：浏览器本身是独立进程，崩溃不会拖垮 daemon。CDP 连接断开时触发该 target 的 circuit breaker（ADR-003），并由 supervisor 重启浏览器。

### 4. 登录与 Secret

- 用户在该账号专用 profile 的可见窗口中**自己登录**。Loadout 不经手密码，不填写登录表单，不处理 OAuth 授权页。
- **ADR-004 的例外**：session cookie 保存在 profile 目录中，由 Chromium 使用 Windows DPAPI 加密，不进入 Credential Manager。约束如下：
  - Loadout 不读取、导出或复制 cookie 的值；
  - SQLite 只保存 profile 目录的引用；
  - profile 目录仅当前用户可访问；
  - 删除账号时删除整个 profile 目录。
- 登录状态检测由后台低频执行（读取页面状态，不发送 prompt）。

### 5. 请求与响应映射

- **输入**：通过 CDP 输入事件把内容填入页面输入框并提交，与用户手动操作等价。不调用页面内部的私有 JS 接口。
- **输出**：网页端的回答原样作为模型生成的内容返回，Loadout 不改写、不截取、不过滤。
  - 优先通过 CDP 网络事件截获页面**自己发起**的响应流（如 SSE），解析为 canonical 流事件（ADR-006）；
  - DOM 提取只作为后备，并在 Provider 文档中注明（DOM 中得到的是渲染后的内容，原始 Markdown 会丢失）。
- **模型**：寻址为 `web-chatgpt/<account>/<model>`（ADR-010），`<model>` 对应网页端模型选择器中的选项。模型目录由后台任务从页面读取，不在热路径上进行。
- **Capabilities**（ADR-005）：首版只声明文本输入、文本输出和流式输出。
  - 不声明 tool calling、vision、sampling 参数、stop、`RESPONSES_STATEFUL`；
  - 请求需要未声明的能力时，Router 跳过该 target；按精确模型名请求时返回明确错误，不静默丢弃参数；
  - system / instructions 只在新开一段网页对话时，以带明确标记的前缀并入首条输入，在 Provider 文档中说明这一降级。
- **对话映射**：提供两种模式，按账号配置。
  - **持续对话**（默认，服务顾问回路）：
    - 每次只把请求中最新一条 user 消息输入网页对话。请求里更早的历史不再发送，上下文以网页对话本身为准。这样调用方每次只发一条消息也能工作；
    - 使用哪段网页对话：
      1. 请求带有显式对话 ID（ADR-013 §2 的来源 1–2，如 `x-loadout-session-id`）：使用该 ID 对应的网页对话，首次出现时新开一段；
      2. 否则使用账号的默认对话：用户通过 CLI / UI 绑定的一段已有网页对话（沿用其中已经建立的上下文）；未绑定时，首次请求新开一段并记住。
    - 不使用 ADR-013 的兜底方式（哈希对话开头的文本）：调用方每次只发一条消息时，兜底值每次都不同，会导致每次都开新对话；
    - 账号的默认对话 URL 是用户配置，不是 secret，也不是请求正文，可以经 migration 存入 SQLite。请求级的"对话 ID → 对话 URL"映射只保存在内存中。
  - **无状态**：每个 API 请求开一个新对话，优先使用站点的临时 / 不保存历史模式，把完整消息历史展平后一次性输入，并设长度上限。
- **用量**：拿不到 token usage，用量数据来源标记为 `ESTIMATED`。

### 6. 顾问回路：通过标准 API 使用

- 对 Loadout 来说，顾问回路就是对某个 `web-chatgpt/...` 模型的**普通 API 调用**：
  - 请求中最新的 user 消息被输入网页对话；
  - 网页端的回答作为模型生成的内容返回；
  - Chat / Responses / Messages 三种入口都可用，协议转换照常进行（ADR-006）。
- 回路本身由调用方实现，例如本地程序、脚本或 agent 的 hook：
  - 调用方决定发送什么内容；
  - 调用方解析回答（例如从中提取"下一轮 prompt"）；
  - 调用方控制回路的轮数与停止条件。
- Loadout 不提供专门的回路命令或 agent 集成，不解析回答内容，也不对输出做审核或过滤：执行与审核不属于 Loadout 的分工。
- 多轮调用落在同一段网页对话，靠 §5 的持续对话模式实现。
- **隐私**：发送的内容会保存在上游的对话历史中。这是该场景的预期行为，在风险说明中写明。Loadout 本地仍然不落盘请求与回答正文（ADR-008）。

### 7. 路由护栏

- 不进入 `loadout/realtime|fast|default` 虚拟模型，只能通过精确名称或用户手动加入 Pool 使用（同 ADR-012）。
- 只允许同一模型在账号之间 failover（ADR-003 / ADR-010）。
- **每个账号同一时间只处理 1 个请求**；排队超过上限（默认很小）时立即返回可重试错误，让 Router 转向下一个 target。
- 每个账号设最小请求间隔和每日软上限（默认值在 ExecPlan 中确定，用户可调）。目的是控制负载与封号风险，不是模拟人类行为。
- 收到额度耗尽或频率限制信号时冷却到 reset。连续出现挑战页或解析失败时自动停用该 target，标记 `needs_attention`。
- **不做主动测速**：主动测速会消耗网页额度。延迟数据只来自真实请求；没有数据前归入"普通"档。

### 8. 风险确认与 UI

- 逐账号的两步确认沿用 ADR-012 §1：
  1. 勾选"我了解风险"；
  2. 输入账号别名确认。
- 风险说明必须包含：该站点相关条款的摘要与链接、封号可能、前端改版会导致失效、对话可能保存在上游、Loadout 不承担责任。
- 每次确认写入审计日志（不含 secret）。
- UI 在账号与路由处显示 Tier D / Web Session 标识，不得让用户误以为它与官方 API 一样稳定（Harness §11）。

### 9. 站点准入

为某个站点新增 `web-<site>` Provider 前，`docs/providers/web-<site>.md` 必须记录：

- 服务条款中与自动化访问相关的条款原文摘要、链接与读取日期；
- 实测观察到的反爬机制，以及**在不违反 §2 的前提下**能否稳定工作（headless / 有界面分别记录）。不能工作的站点不实现；
- 是否有临时对话模式；
- 响应流的格式与解析方式（网络截获或 DOM 后备）；
- Subscription Transport Status: OPT-IN, `WEB_SESSION`；
- 最后验证日期。

条款明确禁止自动化访问的站点（如 ChatGPT），在完成 §8 的风险确认之后允许接入（项目所有者 2026-10-15 确认）。这类站点的风险说明必须引用相关条款原文，并明确说明使用可见窗口只能降低被反爬拦截的概率，不改变条款本身的约束，封号风险由用户承担。

### 10. 测试

- 默认测试不访问真实站点：使用本地 mock 聊天页面（localhost 上提供输入框和 SSE 响应），驱动真实 Edge 做集成测试。机器上没有 Edge 时跳过并给出说明。
- 流解析、能力降级、排队与冷却逻辑做单元测试。
- Live test 需要 `LOADOUT_LIVE_TESTS=1`，由项目所有者用自己的账号执行。
- Router 选路不变，但 RouteSnapshot 增加了 target 状态，仍须跑 benchmark（Harness §20）。

## Alternatives

| 方案 | 否决原因 |
|---|---|
| 照搬社区做法，使用 Camoufox / nodriver 等指纹伪装 | 规避访问控制，与 ADR-012 §2 拒绝 Claude 伪装的理由相同 |
| 逆向网页后端接口，自行生成 Sentinel / PoW 令牌（chat2api 模式） | 同样是规避访问控制，且最脆弱：令牌算法一变就失效 |
| 读取用户日常浏览器的 cookie | ADR-009 §2 与 Harness §12 明确禁止；也会与用户自己的浏览器会话互相干扰 |
| Python / Node Playwright sidecar | Playwright 的等待与选择器更成熟，但需要随安装包附带额外运行时，增加打包与许可证负担；首版功能只需要 CDP 的少量能力 |
| 在 `loadout-app` 中用隐藏的 WebView2 | daemon 必须在没有 GUI 时独立工作（ADR-002） |
| 在 daemon 中嵌入 WebView2 | 需要窗口与消息循环，进程模型更复杂；崩溃隔离不如独立的 Edge 进程 |
| Loadout 提供回路专用功能（relay 命令、agent hook 集成、提取下一轮 prompt、停止条件） | Loadout 只负责输入输出；以标准 API 暴露后，任何本地程序都能自行组织回路，不需要为某个 agent 定制 |
| Loadout 直接编排 agent（启动或驱动 agent 进程，注入输入） | 越出网关的职责；各 agent 的机制不同 |
| 默认 headless | 更容易被识别，开发与排错更难；顾问回路场景下用户本来就要看这段对话 |
| 不做 | 保留为默认选项：本 ADR 不被接受时，维持 ADR-009 §2 的禁令 |

## Consequences

- `WEB_SESSION` 第一次用于推理 Transport。glossary §3 已同步更新（原句"v1 推理 Transport 只允许 Tier A"在 ADR-012 引入 Tier C 后已经过时）。
- 新增依赖：CDP 客户端库、Edge 安装检测；新增共享 crate（浏览器池与流解析）。
- 资源占用：每个已启用的账号常驻一个浏览器进程。
- 维护成本：每个站点都会随前端改版失效；依赖 Provider 文档的"最后验证日期"和自动停用机制来暴露问题。
- ADR-004 增加一个明确记录的例外（profile 中的 cookie）。
- 不伪装的前提下，部分站点可能无法接入，或只能在有界面模式下运行。
- 实施需要新建 ExecPlan（跨 crate、新增 Transport 类别、涉及 credentials 与 daemon 生命周期，Harness §6）。

## 待确认问题（项目所有者）

已确认（2026-10-15）：

- 默认使用正常的可见浏览器窗口；
- 主要场景为顾问回路，通过标准 API 使用，回路由调用方实现；
- Loadout 只负责输入输出，不做执行与审核；
- 首个站点为 ChatGPT 网页；条款禁止自动化的站点在风险确认后允许接入。

仍待确认：

1. §7 中最小请求间隔与每日软上限的默认值（也可以在 ExecPlan 中实测后再定）。
