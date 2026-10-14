# ExecPlan 017: Web Session Transport — `web-chatgpt`（opt-in）

- **Status**: Active（实施中）
- **Created**: 2026-10-15
- **Decision**: [ADR-014](../../adr/ADR-014-browser-session-transport.md)（修订 ADR-009 §1–2、ADR-007 §2、ADR-004 §1）
- **Related**: [ADR-012](../../adr/ADR-012-subscription-transport-opt-in.md)（两步 opt-in）、[ADR-013](../../adr/ADR-013-upstream-conversation-session.md)（对话 ID 来源）、[ExecPlan 010](010-codex-subscription-transport.md)（opt-in / AccountSlots 的现有实现）

---

## 1. Goal

用户在 Loadout 专用的 Microsoft Edge 窗口中自己登录 ChatGPT 网页。经全局开关与逐账号两步风险确认后，本地程序可以用普通 API 调用 `web-chatgpt/<account>/<model>`：

- 请求中最新一条 user 消息通过 CDP 输入事件填入网页对话并提交；
- 页面自己发起的响应流经 CDP 网络事件被动截获，原样作为模型输出返回（Chat / Responses / Messages 三个入口均可）；
- 默认"持续对话"模式，多轮调用落在同一段网页对话中（顾问回路，ADR-014 §6）。

Loadout 只负责输入输出；不做回路控制、不解析、不审核回答。

## 2. Context

- ADR-014 已于 2026-10-15 由项目所有者接受（提交 e73c597），本计划是其实施计划。
- ChatGPT 条款（OpenAI Terms of Use，"What you cannot do"）禁止以自动化或程序化方式提取数据或 Output。项目所有者确认：完成风险确认后允许接入，风险由用户承担（ADR-014 §9）。
- 硬性原则（ADR-014 §2）：不做指纹伪装、不隐藏自动化特征、不解 CAPTCHA、不在页面外生成反爬令牌、不绕过浏览器直连网页后端。社区项目几乎都依赖这些手段，因此只能作概念参考（见 reference-map §10.12）。
- 现有可复用的部分：
  - ExecPlan 010 的 `accounts.transport_opt_in_at`、`account.set_transport`（别名二次确认 + 审计日志）、`RouteTarget.opted_in`、虚拟模型排除、`AccountSlots`（`ProviderModule::account_concurrency`）；
  - ADR-013 的对话 ID 来源（`gateway/src/proxy/session.rs` 的 `CLIENT_HEADERS` 与 `prompt_cache_key`）；
  - `meter.rs` 在上游不给 usage 时已自动按约 4 字符 / token 估算并标记 `ESTIMATED`；
  - `loadout_platform::process`（无句柄继承的 `CreateProcessW`）、`platform::sid`（当前用户 SID）。

## 3. Existing behavior

- 所有推理 Transport 都是 HTTP：`ProviderModule::endpoint()` 产出 `Endpoint`（URL + headers），`RouteTarget.endpoint` 预先构建，gateway 用 reqwest 发送，再用 `decoder_for(target.protocol)` 把上游 SSE 解码为 `CanonicalEvent`。
- `TrustLevel::WebSession`（Tier D）已在 provider-sdk 中定义，但没有任何模块使用；`allowed_for_inference()` 只放行 Tier A，非 Tier A 目标须 `opted_in`。
- 主动测速（`core/src/bench.rs`）对所有可路由目标发 HTTP 探测请求；auto-probe 由 `benchmark.auto_probe` 控制。
- `spawn_detached` 使用 `CREATE_BREAKAWAY_FROM_JOB`：子进程有意比调用方活得久（daemon 本身就是这样启动的）。没有"随 daemon 退出而结束"的子进程启动方式。
- `credentials.credential_type` 的 CHECK 只允许 `api_key` / `oauth` / `external_managed`。

## 4. Constraints

- 热路径（选路前）不得有网络、进程启动、DB 聚合、浏览器操作（AGENTS 规则 1，Harness §2.2）。选路只读 `RouteTarget` 上的原子状态。
- 不得每个请求启动浏览器（Harness §2.4）：每个已启用账号一个常驻 Edge 进程。
- Capability-first（ADR-005）：gateway / router / core 中不得出现 `if provider == "web-chatgpt"`；站点知识只在 `providers/src/web_chatgpt.rs`；公共机制（浏览器池、CDP、流截获）在共享 crate。执行路径按 **transport 类别**（HTTP / 浏览器会话）分派，而不是按 provider。
- Secret（ADR-004 例外）：session cookie 只存在于 profile 目录（Chromium DPAPI 加密）。Loadout 永不读取、导出、复制 cookie；SQLite 只存 profile 目录引用。
- 请求正文不落盘（ADR-008）：CDP 消息中含 prompt 与回答，任何日志级别都不得输出 CDP payload。
- 不得让 UI 直连 Provider / 浏览器（ADR-002）：窗口操作经 IPC 由 daemon 执行。
- 进程启动遵守 AGENTS §10：不用 `std::process::Command`；不泄漏句柄（ExecPlan 001 Stage 7 的教训）。
- 测试不得触碰真实 ChatGPT、真实安装或用户日常 Edge profile；live test 需 `LOADOUT_LIVE_TESTS=1`。
- 不偷偷换模型：只允许同模型跨账号 failover（ADR-003 / ADR-010）。

## 5. Architecture impact

```text
client ─HTTP─▶ gateway ─select (atomic gate check)─▶ RouteTarget
                                   │
                ┌──────────────────┴───────────────────┐
         Upstream::Http(Endpoint)          Upstream::Session(Arc<dyn SessionUpstream>)
         (现有 reqwest 路径)                 │  canonical request ─▶ Stream<CanonicalEvent>
                                            ▼
                                 loadout-browser::Pool ── per-account BrowserHandle
                                            │   CDP over pipe (--remote-debugging-io-pipes)
                                            ▼
                                 msedge.exe (visible, dedicated --user-data-dir,
                                             in daemon-owned Job: KILL_ON_JOB_CLOSE)
                                            ▲
                     site knowledge: providers::web_chatgpt (impl WebSite)
```

### 5.1 新 crate `crates/browser`（`loadout-browser`）

站点无关的浏览器机制：

- **Edge 检测**：`HKLM` / `HKCU` 的 `SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\msedge.exe`，回退到 `%ProgramFiles(x86)%` / `%ProgramFiles%\Microsoft\Edge\Application\msedge.exe`；版本取自文件版本信息（不启动进程）。低于最低版本（Stage 0 确定，取决于 `Network.streamResourceContent` 的可用版本）时报"不支持"。
- **启动**：通过新的 `loadout_platform::spawn_owned`（§5.4）。参数固定为：
  - `--user-data-dir=<profile>`、`--no-first-run`、`--no-default-browser-check`；
  - `--remote-debugging-pipe --remote-debugging-io-pipes=<r>,<w>`（Windows 上 Chromium 从该开关读取管道句柄，参考 chromedp `pipe_windows.go`，MIT）；
  - 后台节流：`--disable-background-timer-throttling`、`--disable-backgrounding-occluded-windows`、`--disable-renderer-backgrounding`（ADR-014 §3 允许）；
  - headless 仅在 `web_session.headless=true` 时加 `--headless=new`；
  - 起始 URL 为站点 origin。
  - **禁止**出现在参数中的（代码中常量列表 + 单元测试断言）：`--disable-blink-features=AutomationControlled`、任何 `--user-agent=`、代理参数、扩展加载参数。
- **CDP 客户端（自写最小实现）**：JSON-RPC over pipe（`\0` 分隔消息），`serde_json` 编解码，tokio 任务做读写分派（命令 id → oneshot，事件 → 按 sessionId 广播）。
  - **方法白名单**：只允许 `Target.*`（attach / create / activate）、`Page.navigate` / `Page.enable` / 生命周期事件、`Runtime.evaluate`（仅限站点模块提供的固定只读表达式）、`Input.insertText` / `Input.dispatchKeyEvent` / `Input.dispatchMouseEvent`、`Network.enable` / `Network.streamResourceContent` / `Network.getResponseBody` 及其事件、`Browser.close` / `Browser.getWindowForTarget` / `Browser.setWindowBounds`。白名单之外的调用在客户端内直接返回错误；`Network.getCookies`、`Storage.*`、`Network.getAllCookies`、`Fetch.*`（会改变页面请求）明确禁止，并有单元测试。
- **流截获（`NetworkTap`）**：`Network.requestWillBeSent` 按站点给出的匹配规则（方法 + URL 前缀）选中页面自己发起的请求 → `responseReceived` 时调用 `Network.streamResourceContent(requestId)`（返回已缓冲数据，之后 `dataReceived.data` 携带增量）→ 字节流交给站点解析器。WebSocket 帧（`Network.webSocketFrameReceived`）作为第二来源保留接口，Stage 0 确认 ChatGPT 是否使用。
- **浏览器池（`Pool`）**：每个已启用账号一个 `BrowserHandle`（进程、CDP 连接、主 tab、状态原子量）。由 daemon 的 restartable 后台任务（ADR-007 supervisor）管理：启动、健康检查、崩溃 / CDP 断开后指数退避重启（1 s → 5 min 上限）。
- **关闭顺序**：daemon 正常退出时先发 `Browser.close`（让 Edge 刷写 cookie 与 profile），等待最多 5 s，再由 Job 结束剩余进程；daemon 崩溃时 Job 句柄关闭即结束 Edge。

### 5.2 provider-sdk 扩展（ADR-005，均带默认值）

- `TransportKind { Http, BrowserSession }`：`TransportSpec` 增加 `kind` 字段（现有模块为 `Http`）。route_build 与 gateway 按类别分派，不看 slug。`BrowserSession` 的 `protocol` 只作为名义值（取 `Chat`），不参与编解码：这类目标永远走转换路径（§5.3）。
- `trait WebSite: Send + Sync`（站点知识，由 Provider 模块实现）：
  - `origin()`、`new_conversation_url(temporary: bool)`、`conversation_url_pattern()`；
  - `page_state(&dyn PageOps) -> PageState`（`Ready | LoggedOut | Challenge | Busy | Unknown`），只用固定只读表达式；
  - `submit(&dyn PageOps, text)`：聚焦输入框 → `Input.insertText` → 提交（Enter 或点击发送按钮）；
  - `select_model(&dyn PageOps, model)`：通过输入事件操作模型选择器；
  - `stream_match()`：响应流请求的匹配规则；`parser() -> Box<dyn StreamParser>`：把页面流字节解析为 `CanonicalEvent`；
  - `classify_stream(...)`：额度 / 频率限制 / 内容错误 → `UpstreamFailure`；
  - `read_models(&dyn PageOps) -> Vec<CatalogModel>`；
  - `max_input_chars()`。
- `trait PageOps`（由 `loadout-browser` 实现，provider-sdk 不依赖 browser crate）：`eval_readonly(expr)`、`insert_text`、`press_key`、`click(selector)`、`navigate(url)`、`current_url()`、`wait_network(match)`。
- `ProviderModule::web_site() -> Option<&dyn WebSite>`（默认 `None`）。
- `ProviderModule::first_token_timeout() -> Option<Duration>`（默认 `None` = 全局设置）：网页端推理模型首字可达数分钟。
- `ProviderModule::active_probe() -> bool`（默认 `true`）：`web-chatgpt` 返回 `false`，bench / auto-probe 跳过（ADR-014 §7）。
- `SessionGate`（放在 router crate，挂在 `RouteTarget` 上，`Option<Arc<SessionGate>>`，HTTP 目标为 `None`）：
  - `state: AtomicU8`（`Ready / Busy / NeedsAttention / Starting / Cooling`）、`next_allowed_ms: AtomicI64`（最小间隔与冷却）、`today_count: AtomicU32` + `day_key: AtomicU32`（每日软上限）；
  - 只由浏览器池与 gateway 写入，选路只读。

### 5.3 Gateway / Router

- `RouteTarget.endpoint` 改为 `upstream: Upstream`（`Http(Endpoint) | Session(Arc<dyn SessionUpstream>)`）。`SessionUpstream::execute(SessionRequest) -> Result<EventStream, UpstreamFailure>`，由 core 用池句柄构建。
- 浏览器会话目标永远走"转换"路径：解码客户端请求为 `CanonicalRequest` → `SessionUpstream` → `CanonicalEvent` 流 → 现有 `Renderer::converted` / `Aggregator`。透传路径不适用。
- 选路（`router/src/select.rs`）：gate 存在时检查 `state == Ready && now >= next_allowed_ms && today_count < cap`，否则跳过并记录原因（`web session busy` / `needs attention` / `min interval` / `daily soft cap`）。只读原子量。
- 并发：`account_concurrency() = Some(1)`，复用 `AccountSlots`；拿不到许可即跳过（排队上限默认 0）。没有其他目标时返回可重试的 429（`Retry-After` 取 gate 的 `next_allowed_ms`）。
- 能力检查：请求含图像、tools、`tool_choice`、sampling 参数、stop、`previous_response_id` 时跳过该目标；精确模型名请求返回 `InvalidRequest` 并说明哪个参数不受支持。
- 客户端断开：会话执行器点击页面的"停止生成"（输入事件），等待页面回到 Ready 后才释放许可。

### 5.4 Platform

- `spawn_owned(program, args, inherit: &[HANDLE], job: &OwnedJob)`：
  - `CreateProcessW` + `CREATE_SUSPENDED`，`bInheritHandles = TRUE` 但用 `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` 只继承两个管道句柄（父端句柄设为不可继承）；
  - `AssignProcessToJobObject` 后 `ResumeThread`；
  - `OwnedJob`：daemon 启动时创建，`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`，句柄只在 daemon 进程内；Edge 的子进程（renderer / GPU）随之进入该 Job（嵌套 Job，Win8+）。
- `restrict_dir_to_current_user(path)`：profile 目录设受保护 DACL（当前用户 SID + SYSTEM，完全控制；不继承父目录 ACE）。
- `remove_dir_all_retry(path)`：删除账号时等待 Edge 释放文件锁后删除 profile；失败则记入"待删除"列表，下次 daemon 启动时重试。

### 5.5 Core

- 全局设置 `web_session.enabled`（bool，默认 `false`）、`web_session.headless`（bool，默认 `false`）。关闭时：池不启动任何浏览器、route_build 不生成 `BrowserSession` 目标；已运行的浏览器正常关闭。
- 账号流程（provider `web-chatgpt` 在首次添加账号时自动创建，类似 codex-sub）：
  - `account.web_add {provider, alias, confirm}`：要求全局开关已开；`confirm` 必须等于 alias（第二步，第一步"我了解风险"在 UI / CLI 侧）。创建账号（`account_type = 'subscription'`、`transport_opt_in_at = now`）、profile 目录、ACL，然后启动可见窗口打开登录页。写审计日志（只含 provider / alias）。
  - `account.set_transport`（现有）：关闭 → 停止路由并关闭该账号浏览器（profile 保留）；重新开启 → 再次两步确认。
  - `account.web_show {provider, alias}`：把窗口带到前台（未运行则启动）。
  - `account.web_bind {provider, alias, conversation_url | null}`：绑定 / 解绑默认对话；URL 必须匹配站点的 `conversation_url_pattern()`。
  - `account.web_config {provider, alias, mode, min_interval_secs, daily_soft_cap}`。
  - `account.web_status` → `{state, logged_in, last_check_ms, models, default_conversation_bound}`。
  - 删除账号：关闭浏览器 → 删除 profile 目录。
- 后台任务（不在热路径上）：
  - 登录 / 挑战页检查：每 10 分钟、每次请求失败后，以及用户在 UI 点"我已处理"时，调用 `page_state`（不发送 prompt）。
  - 模型目录：浏览器 Ready 后读取一次，此后每 6 小时；结果写入 `account_models`，变化时重建路由。
  - 每日计数：daemon 启动时从 `request_events` 统计当天该账号的请求数初始化 `today_count`（后台，一次）；本地时间零点重置。
- 状态转移：连续 3 次挑战页 / 解析失败 / 登录失效 → `NeedsAttention`（终止状态，直到检查通过），通过现有 `notify.rs` 发桌面通知；检测到额度或频率限制 → `Cooling` 到解析出的 reset 时间，无法解析时 1 小时。
- route_build：`BrowserSession` 目标只在全局开关开 + 账号已 opt-in 时生成；不进入 `loadout/realtime|fast|default`；无测速数据时归入"普通"档。

### 5.6 `web-chatgpt` Provider 模块

- descriptor：slug `web-chatgpt`、`AuthMethod::BrowserLogin`（新增）、`TransportKind::BrowserSession`、`TrustLevel::WebSession`；capabilities 只有文本输入、文本输出、流式。
- **对话映射**（ADR-014 §5）：
  - **持续对话**（默认）：只输入最新一条 user 消息（最后一条不是 user 时返回 `InvalidRequest`）。对话选择：
    1. 请求带 ADR-013 来源 1–2 的显式 ID（客户端 session 头或 `prompt_cache_key`）→ 内存映射 `(client, account, id) → conversation URL`，首次出现时新开对话，提交后从页面导航事件取得 `/c/<id>` URL；
    2. 否则用账号默认对话（`web_session_json.conversation_url`）；未绑定时首次请求新开一段并写回（标记 `bound_by = "auto"`）。
    - 不使用 ADR-013 的"对话开头哈希"兜底。
    - 内存映射在 daemon 重启后丢失：显式 ID 会开新对话（文档说明）。
  - **无状态**：每个请求开临时对话（`new_conversation_url(temporary = true)`），把完整消息历史按固定标记展平后一次性输入；超过 `max_input_chars()` 返回 `InvalidRequest`。
  - **system / instructions**：只在新开对话的首条输入中，以 `[System instructions from the API caller]` … `[End of system instructions]` 标记作前缀；已有对话中忽略，并在请求决策记录（ExecPlan 015）中写入"system ignored"（不含正文）。
- **输出**：只取 assistant 回复消息的正文文本部分，原样作为 `TextDelta` 输出；不改写、不截取、不过滤。搜索 / 工具中间消息和思考过程不属于回答，首版不输出（文档说明）。网络截获不可用时的 DOM 后备只在 Stage 0 证明必要时实现，并在 Provider 文档中注明会丢失原始 Markdown。
- **用户正在使用同一窗口**：提交前若输入框非空（用户正在起草），不覆盖，返回可重试的 busy 错误；页面正在生成（用户刚发了消息）同样视为 busy。
- 每账号持久化配置：`accounts.web_session_json`（§9）。

## 6. Files expected to change

| 区域 | 文件 |
|---|---|
| 新 crate | `crates/browser/{Cargo.toml, src/lib.rs, edge.rs, launch.rs, cdp.rs, allowlist.rs, tap.rs, page.rs, pool.rs}`；`Cargo.toml`（workspace member + `loadout-browser`） |
| platform | `crates/platform/src/process.rs`（`spawn_owned`）、新 `job.rs`、`acl.rs`、`lib.rs` |
| provider-sdk | `descriptor.rs`（`TransportKind`、`AuthMethod::BrowserLogin`）、`module.rs`（`web_site`、`first_token_timeout`、`active_probe`）、新 `web.rs`（`WebSite`、`PageOps`、`PageState`、`StreamParser`） |
| providers | 新 `web_chatgpt.rs`、`registry.rs`、`tests/fixtures/web_chatgpt/*.sse`（脱敏） |
| router | `target.rs`（`Upstream`、`SessionGate`）、`select.rs`（gate 检查） |
| gateway | `proxy/mod.rs`（Session 执行分支、取消处理）、`plane.rs`（每目标 first-token timeout） |
| storage | `migrations/0010_web_session/{up,down}.sql`、`migrations.rs`、`accounts.rs`、`routing.rs` |
| core | `daemon.rs`（Job、池启动 / 关闭）、新 `web_session.rs`（池接线、检查、目录、计数）、`route_build.rs`、`bench.rs`、`settings.rs`、`dispatcher.rs`、`control.rs` |
| ipc | `api.rs`（新方法与类型） |
| CLI | `apps/loadout/src/bin/loadout/account_cmd.rs`（`account add web-chatgpt`、`account web show/bind/config/status`） |
| desktop | `apps/desktop/src/pages/Accounts.tsx`、`Settings.tsx`、`api.ts`、`i18n.tsx`；`src-tauri` bridge 白名单 |
| benchmarks | `benches/route_selection.rs`（含 gate 的目标）、`benches/stream_proxy.rs`（不变，回归对照） |
| docs | `docs/providers/web-chatgpt.md`、`docs/providers/README.md`、README、`docs/architecture/glossary.md`（如需）、system-test-plan、`THIRD_PARTY_NOTICES.md`（如有移植） |

## 7. Implementation stages

每个 Stage 结束时 `cargo fmt`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace` 通过，并更新 §15 Progress。

### Stage 0 — 站点准入与可行性验证（门槛，ADR-014 §9）

不写产品代码。spike 代码放在仓库外的临时目录，不提交。需要项目所有者用自己的 ChatGPT 账号登录专用测试 profile。

1. 写 `docs/providers/web-chatgpt.md`：
   - OpenAI Terms of Use 相关条款原文摘要、链接、读取日期（2026-10-15 已读）；
   - Subscription Transport Status: `OPT-IN`, `WEB_SESSION`；
   - 是否有临时对话模式及入口；
   - 最后验证日期。
2. 用未修改的 Edge（可见窗口）+ pipe 模式验证：
   - `--remote-debugging-io-pipes` 在当前 Edge 版本可用；`navigator.webdriver` 的实际值（只记录，不修改）；
   - 登录后能正常对话，是否出现 Turnstile / 挑战页；分别记录可见窗口与 headless；
   - 响应流的传输方式（fetch SSE / WebSocket）、URL、格式，`Network.streamResourceContent` 能否拿到增量；
   - 窗口最小化 / 被遮挡时，带节流参数的流式输出是否停顿（含 Edge 的效率模式 / 睡眠标签页）；
   - `Input.insertText` 对多段文本（含换行、约 50 KB）的表现；
   - 模型选择器与模型列表的读取方式（被动截获页面自己的模型列表响应，或读 DOM）。
3. 记录到 Provider 文档并更新本计划 Decisions。**任一结论为"不违反 ADR-014 §2 就无法稳定工作"时停止本计划**，向所有者报告。

### Stage 1 — platform：Job、受控启动、ACL、Edge 检测

- `OwnedJob`、`spawn_owned`（handle list）、`restrict_dir_to_current_user`、`remove_dir_all_retry`。
- Edge 检测与版本读取（放 `loadout-browser::edge`，注册表读取用 windows-sys）。
- 测试：子进程只继承指定句柄（用测试辅助 exe 枚举自身句柄或尝试写入未传入的句柄）；Job 句柄关闭后子进程退出；ACL 只含当前用户与 SYSTEM。

### Stage 2 — `loadout-browser`：CDP、NetworkTap、Pool

- CDP 客户端（pipe 传输、命令 / 事件分派、超时、方法白名单）。
- `PageOps` 实现、`NetworkTap`、`Pool` + supervisor 接口（启动、健康检查、退避重启、有序关闭）。
- 本地 mock 聊天页（测试辅助，axum 绑定 `127.0.0.1:0`）：一个输入框 + 发送按钮，提交后用 `fetch` 请求本地 SSE 端点并渲染；可配置为返回挑战页、登录页、429、断流。
- 集成测试驱动真实 Edge（默认 headless，临时 profile，测试专用 Job）；机器上没有 Edge 时 `eprintln!` 说明并跳过。可见窗口测试（含最小化节流验证）需 `LOADOUT_BROWSER_VISIBLE_TESTS=1`。

### Stage 3 — SDK 扩展 + `web-chatgpt` 模块

- §5.2 的 trait 与类型；`web_chatgpt.rs`：descriptor、`WebSite` 实现、流解析器、错误分类、模型目录解析、首条输入组装（system 标记、无状态展平、长度上限）。
- 单元测试 + fixture 测试：Stage 0 采集的 SSE 样本**脱敏后**入库（替换对话 / 消息 / 账号 ID、邮箱，正文替换为合成文本），覆盖增量、结束、额度耗尽、频率限制、错误事件。

### Stage 4 — storage + core

- Migration 0010 + 升级测试（从 0009 的数据库升级，旧数据不受影响；down 可逆）。
- 设置项、`web_session.rs`（池接线、后台检查、模型目录、每日计数、状态转移、通知）、IPC 方法 + dispatcher + bridge 白名单、route_build（gate、虚拟模型排除、无主动测速）、`bench.rs` 跳过 `active_probe() == false`、daemon 启动 / 关闭顺序。
- 测试（mock 页 + headless Edge）：开关关闭时不启动浏览器且无目标；opt-in 流程与别名校验；删除账号删除 profile；挑战页连续出现 → NeedsAttention；额度信号 → Cooling。

### Stage 5 — router / gateway

- `Upstream` 枚举、Session 执行分支、取消处理、每目标 first-token timeout、选路 gate 检查、能力跳过与精确名错误、可重试 busy 错误。
- 对话映射：持续（显式 ID / 默认对话 / 自动绑定）与无状态。
- 端到端测试：经 `POST /v1/chat/completions`、`/v1/responses`、`/v1/messages` 调用 mock 页，验证流式与非流式（聚合）输出与 mock 页发出的文本逐字节一致；同一 session ID 两次请求落在同一 mock 对话；第二个并发请求得到 busy / failover；最小间隔生效。
- Benchmark（§11）。

### Stage 6 — CLI

- `loadout account add web-chatgpt <alias>`：显示风险说明 → 确认 "I understand" → 输入别名；脚本用 `--i-understand-the-risk --confirm <alias>`。
- `loadout account web show|bind|config|status <alias>`；`loadout config set web_session.enabled true` 时打印风险提示。
- `account list` / `route status` 显示 `WEB_SESSION` 与 gate 状态。

### Stage 7 — 桌面端

- 设置页：全局开关（开启时显示风险说明）。
- 账号页：ChatGPT 网页卡片；添加对话框（风险列表 → 勾选 → 输入别名）；账号卡片显示 Tier D / Web Session 标识、状态（就绪 / 忙 / 需要处理 / 冷却至 HH:MM）、"打开窗口"、"我已处理"、绑定默认对话、模式与限额设置。
- 路由页 / 模型页对 Tier D 目标显示标识（Harness §11）。
- 用 ExecPlan 009 的 mock IPC 在浏览器预览中走通添加与状态展示。

### Stage 8 — 文档与 live test

- 完成 `docs/providers/web-chatgpt.md`（Stage 0 结论 + 用法 + 降级说明 + 重启后显式 ID 映射丢失 + 隐私：对话保存在上游）、providers 索引、README、system-test-plan。
- 项目所有者执行 live test（`LOADOUT_LIVE_TESTS=1`）：登录 → 启用 → 用同一 session ID 连续 3 轮调用 → 确认网页中为同一段对话、返回内容与网页显示一致 → 最小化窗口再调用一次。结果记入 Progress。

## 8. Tests

| 层级 | 内容 |
|---|---|
| Unit | CDP 帧编解码与分派；方法白名单（禁止方法被拒）；启动参数不含禁止参数；流解析器（增量、结束、错误）；错误分类；system 前缀与无状态展平；长度上限；gate 状态转移与每日重置；能力跳过 |
| Fixture | 脱敏的 ChatGPT SSE 样本（Stage 0 采集）；模型目录样本 |
| Integration（真实 Edge + mock 页，默认运行，无 Edge 时跳过） | 启动 / 关闭 / 崩溃重启；Job 随测试结束清理；NetworkTap 逐字节还原；输入框非空时 busy；挑战页 / 登录页检测；客户端断开时点击停止 |
| Integration（可见窗口，`LOADOUT_BROWSER_VISIBLE_TESTS=1`） | 最小化与遮挡时流式不停顿 |
| E2E（daemon + CLI + mock 页） | 三个入口；持续 / 无状态；并发与最小间隔；开关关闭；opt-in 与删除 |
| Migration | 0009 → 0010 升级；down 可逆 |
| Live（`LOADOUT_LIVE_TESTS=1`，所有者执行） | 见 Stage 8 |

测试隔离：临时 data dir、端口 0、唯一 pipe 名与 instance id、`--secret-prefix loadout-e2e-<id>:`；浏览器 profile 在临时目录，测试专用 Job 结束时清理全部 Edge 进程。测试**不得**使用 `%LOCALAPPDATA%\Loadout\browser-profiles` 或用户日常 Edge profile。可见窗口测试运行前确认用户没有在使用桌面（见 memory：GUI 自动化）。

## 9. Migration considerations

- Migration `0010_web_session`：

  ```sql
  -- up
  ALTER TABLE accounts ADD COLUMN web_session_json TEXT;
  -- down
  ALTER TABLE accounts DROP COLUMN web_session_json;
  ```

  JSON 内容（均非 secret、非请求正文）：`{"profile":"browser-profiles/<account-id>","mode":"persistent|stateless","conversation_url":null|"https://chatgpt.com/c/…","bound_by":"user|auto","min_interval_secs":10,"daily_soft_cap":150}`。沿用 0008 `quota_reserve_json` 的做法。
- 不新增 `credentials` 行：profile 不是 Credential Manager secret，不改 `credential_type` 的 CHECK（避免重建表）。
- 设置项 `web_session.enabled` / `web_session.headless` 走现有 settings 表，无 schema 变更。
- 旧版本降级：down 后 `web-chatgpt` 账号仍在 `accounts` 中，但旧版本没有该 Provider 模块，会按"未知 kind"处理（与现有行为一致，测试覆盖）。profile 目录不会被 down 删除。

## 10. Security considerations

- **CDP 通道 = 已登录浏览器的完全控制权**：只用 pipe，不开 TCP 调试端口，其他本地进程 / 用户无法连接。父端管道句柄不可继承；子进程只继承两个指定句柄。
- **不接触 cookie**：方法白名单禁止 `Network.getCookies`、`Network.getAllCookies`、`Storage.*`、`Fetch.*`；代码中不出现读取 profile 目录内文件的逻辑（只创建、设 ACL、删除）。
- **Profile 目录**：受保护 DACL（当前用户 + SYSTEM）；删除账号时删除整个目录；不读取用户日常浏览器 profile。
- **日志**：CDP 消息（含 prompt、回答、请求 postData）在任何日志级别都不输出；只记录方法名、耗时、错误类别。增加一条测试：trace 级日志中不出现 mock 页返回的正文。
- **页面内容不可信**：`Runtime.evaluate` 只执行站点模块中的固定表达式，不拼接页面数据或请求内容；回答原样返回，Loadout 不解释其中的指令。
- **导航限制**：`navigate` 只允许站点 origin 下的 URL；`web_bind` 校验 URL 模式。
- **不规避访问控制**（ADR-014 §2）：启动参数禁止列表 + 单元测试；遇到挑战页只标记 NeedsAttention，不点击、不填写。
- **审计**：opt-in / 关闭 / 全局开关变化写 warn 级审计日志（只含 provider / alias）。
- **隐私**：风险说明写明对话会保存在 ChatGPT 历史中；Loadout 本地不落盘正文（ADR-008）。

## 11. Performance validation

- Router / Gateway 改动（Harness §20）：背靠背 A/B 对比改动前提交与改动后：
  - `route_selection/{exact, provider_model, virtual+session}`：HTTP 目标不得变慢（gate 为 `None` 时只多一次分支）；新增 `route_selection/exact_web_gate`（gate 原子读取）。
  - `snapshot_swap`、`stream_proxy/{passthrough_200, convert}`、`ttft/*`：无回退。
- 浏览器路径本身不设 TTFT 目标（秒级），但测量并记录：mock 页上从选路完成到首个 `TextDelta` 的 Loadout 自身开销（目标 < 50 ms，不含页面与上游）；每个常驻 Edge 的内存（Stage 0 / Stage 8 实测，写入 Provider 文档）。

## 12. Known risks

| 风险 | 应对 |
|---|---|
| 封号 / 条款风险 | 两层开关 + 两步确认 + 风险说明引用条款原文；低频默认值；不主动测速 |
| 不伪装时被反爬拦截 | Stage 0 门槛；被拦截即 NeedsAttention 并由用户手动处理；不做规避 |
| 前端改版导致选择器 / 流格式失效 | 站点知识集中在一个模块；连续失败自动停用；Provider 文档的最后验证日期 |
| `streamResourceContent` 为 experimental，Edge 版本可能不支持或行为变化 | 最低版本检测；DOM 后备（仅在必要时实现） |
| ChatGPT 改用 WebSocket 推送 | NetworkTap 预留 WebSocket 帧来源 |
| Edge 效率模式 / 睡眠标签页在最小化时仍节流 | Stage 0 实测；不修改系统策略（禁止改系统设置）；必要时在文档中建议不要最小化，仅遮挡 |
| 强制结束 Edge 导致刚写入的 cookie 丢失 | 有序关闭先发 `Browser.close`；崩溃路径接受该风险 |
| 同一 profile 被残留的 Edge 进程占用，新进程转交后立即退出 | 启动前检查 profile 锁；Job 保证 daemon 退出时清理；报告"profile 被占用" |
| 用户在窗口中同时操作 | 输入框非空 / 正在生成时返回 busy，不覆盖 |
| 推理模型首字超时 | 每模块 first-token timeout 覆盖（默认 600 s，可调） |
| 显式 ID 映射在重启后丢失 | 文档说明；顾问回路默认用账号默认对话（持久化） |
| 嵌套 Job 与 Edge 沙箱的交互 | Stage 1 测试 renderer 进程是否进入 Job 且被清理 |

## 13. Definition of done

- [ ] Stage 0 门槛通过，`docs/providers/web-chatgpt.md` 含条款摘要、反爬观察（可见 / headless）、临时对话、流格式、Transport 状态、最后验证日期 —— 离线部分已完成；依赖已登录账号的观察留在 live test
- [x] §5 全部实现，测试覆盖 §8（未做：可见窗口的最小化节流测试 `LOADOUT_BROWSER_VISIBLE_TESTS=1`，按约定不在用户桌面上弹窗，并入 live test）
- [x] fmt / clippy / 全部测试通过（2026-10-15：45 个测试二进制、357 个测试）；benchmark 无可测回退（§11）
- [x] `python scripts\third_party_licenses.py --check` 通过（347 个包、0 违规）；没有移植代码，THIRD_PARTY_NOTICES 无需新增
- [ ] 文档完成；本文件移入 `completed/` —— 文档已完成，live test 后再移动
- [ ] 项目所有者完成 live test，结果记入 Progress

## 14. Decisions

| # | 决定 | 理由 / 状态 |
|---|---|---|
| D1 | **CDP 客户端自写最小实现**，不引入 CDP crate | 只需约 20 个方法；需要 pipe 传输（`--remote-debugging-io-pipes`）与方法白名单。已核对的候选（2026-10-15，crates.io）：`chromiumoxide` 0.9.1（MIT OR Apache-2.0）以 WebSocket 连接为主、未见 Windows pipe 传输支持，生成的协议类型很大，自带启动器不符合 AGENTS §10 的启动约定；`chromey` 2.58（MIT OR Apache-2.0，chromiumoxide 的分支，问题相同）；`headless_chrome` 1.0.22（MIT）是同步线程模型。协议定义以 `ChromeDevTools/devtools-protocol`（BSD-3-Clause）为准；Windows pipe 句柄传递参考 chromedp `pipe_windows.go`（MIT）的概念，自行实现，不复制代码 |
| D2 | **pipe 而非 TCP 调试端口** | 避免其他本地进程连接已登录的浏览器。若 Stage 0 证明 Edge 不支持 io-pipes，退回 `--remote-debugging-port=0` + `DevToolsActivePort`，需引入 `tokio-tungstenite` 0.30（MIT）/ `tungstenite`（MIT OR Apache-2.0），并在本表补记安全评估 |
| D3 | Edge 进程放入 daemon 持有的 `KILL_ON_JOB_CLOSE` Job，而不是 `spawn_detached` | ADR-014 §3 要求生命周期与 daemon 绑定；`spawn_detached` 有意脱离 Job。新函数同样不泄漏句柄 |
| D4 | 执行路径按 `TransportKind` 分派，站点知识通过 `WebSite` / `PageOps` trait 注入 | ADR-005；provider-sdk 不依赖 browser crate |
| D5 | 输出走 `Network.streamResourceContent` 被动截获，不用 `Fetch` 拦截 | `Fetch` 会暂停并接管页面请求，改变页面行为 |
| D6 | 每账号并发 1，排队上限 0（拿不到许可即跳过 / 返回可重试 429） | ADR-014 §7；顾问回路本身是串行调用 |
| D7 | 默认最小请求间隔 **10 s**、每日软上限 **150** 次 / 账号；额度信号无 reset 时冷却 1 h；连续 3 次失败 → NeedsAttention | **待项目所有者确认**（ADR-014 待确认问题 1），可在 Stage 0 后调整 |
| D8 | 默认 first-token timeout 对 `web-chatgpt` 为 600 s | 网页端推理模型思考时间长 |
| D9 | 不加、不删影响 `navigator.webdriver` 的参数；Stage 0 记录其实际值 | ADR-014 §2："不修改或隐藏"。主动加 `--enable-automation` 也属于改变站点所见特征，首版不做；**如所有者希望显式声明自动化，可改为加入该参数** |
| D10 | 账号添加时即完成两步确认（`account.web_add`），关闭 / 重新开启复用 `account.set_transport` | 网页会话账号未 opt-in 时没有用途；复用 ADR-012 流程 |
| D11 | 每账号配置存 `accounts.web_session_json`（Migration 0010），不新增 credentials 行 | 避免为 CHECK 约束重建表；profile 不是 Credential Manager secret |
| D12 | 只输出 assistant 回复正文；思考过程与搜索 / 工具中间消息首版不输出 | 它们不是回答本身；回答正文原样返回 |
| D13 | 默认集成测试用 headless Edge 驱动 mock 页；可见窗口测试单独 opt-in | 避免测试在开发者桌面弹窗；mock 页没有反爬问题 |
| D14 | 不把 `RouteTarget.endpoint` 改成 `Upstream` 枚举，而是新增 `web: Option<Arc<WebBinding>>`（gate + `SessionUpstream`）；也不新增 `TransportKind`，以 `ProviderModule::web_site().is_some()` 判断 | 改动面小（HTTP 路径一行不变），仍是 capability-first（ADR-005）；`endpoint` 对网页目标只记录站点 origin |
| D15 | `SessionGate`、`SessionUpstream`、`WebSite`、`PageOps` 都放在 provider-sdk 的 `web` 模块 | browser crate 只依赖 provider-sdk，不依赖 router |
| D16 | 每日软上限按 **UTC 日** 计数 | std 没有本地时区；为此引入时区依赖不值得。文档说明 |
| D17 | 持续对话模式下，**新开**对话时输入完整历史（展平），已有对话只输入最新一条 user 消息 | daemon 重启后显式 ID 映射丢失时不丢上下文 |
| D18 | `max_output_tokens` 接受但不执行（Messages 入口必带）；`temperature`、`top_p`、`stop`、tools、`reasoning`、`response_format` 拒绝（跳过该目标） | ExecPlan §5.3；否则 Messages 入口完全不可用 |
| D19 | 模型通过 URL 参数 `?model=<slug>` 选择（新对话与已有对话都用），`auto` 不带参数；slug 只允许 `[A-Za-z0-9._-]` | 不操作模型选择器的 DOM，更稳；实际回答的模型从答案流读取并写入 debug 日志 |

## 15. Progress

- [x] 2026-10-15 Stage 0（离线部分）：在仓库外的 spike 中用 Edge 155.0.4283.45 + 本地 mock 页验证：
  - `--remote-debugging-pipe --remote-debugging-io-pipes=<r>,<w>` 在 Windows 上可用（只继承这两个句柄）；
  - `navigator.webdriver` 为 `true`（远程调试时浏览器自己设置，Loadout 不修改，D9）；headless 时 UA 含 `HeadlessChrome`，即 headless 会如实暴露自身；
  - `Input.insertText` 一次插入 50 KB（含换行）完整无误；
  - `Network.streamResourceContent` 返回已缓冲数据，之后的 `Network.dataReceived` 携带增量 `data`；
  - 必须在提交前订阅网络事件（spike 第一次就因事后订阅漏掉了请求事件）；
  - 结束 Edge 主进程不会结束其子进程（观察到残留的 renderer），证实需要 Job（D3）。
  - **需要登录的部分**（真实 ChatGPT 的页面结构、反爬表现、流格式、最小化节流）留给所有者 live test（Stage 8）。
- [x] 2026-10-15 Stage 1 platform：`OwnedJob`、`spawn_owned`（handle list + suspended + 入 Job）、`restrict_dir_to_current_user`（受保护 DACL：当前用户 + SYSTEM）、`dacl_sddl`、`app_path`；测试覆盖只继承两个句柄、关闭 Job 结束进程、ACL 内容。
- [x] 2026-10-15 Stage 2 `loadout-browser`：CDP 客户端（NUL 分帧、方法白名单、订阅）、Edge 检测、启动参数（禁止列表 + 测试）、`Page`（PageOps、导航、截获）、`WebAccount`（启动、检查、显示窗口、请求执行、对话映射、取消时点击停止、冷却与 NeedsAttention）。
- [x] 2026-10-15 Stage 3 provider-sdk `web` 模块 + `web-chatgpt` 模块（URL、页面状态、提交、答案流解析 delta v1 / 旧版累积格式、错误分类、模型列表）；单元测试 + 合成样本。
- [x] 2026-10-15 Stage 4（部分）storage：Migration 0010 + 往返测试；`RoutableModel.web_session_json`；`count_account_requests`。core：`web_session.rs`（注册表、后台监管任务、`account.web_*` 操作、事件持久化、通知）、route_build 绑定、模型同步、删除账号删 profile、测速跳过、设置项 `web_session.enabled|headless`、daemon 启停。
- [x] 2026-10-15 Stage 5（部分）router：选路时读 gate；gateway：`proxy/web.rs`（预提交可 failover、提交后流式、非流式聚合、显式对话 ID）。
- [x] 2026-10-15 集成测试（真实 headless Edge + mock 页，`crates/browser/tests/web_session.rs`，约 4 s）：持续对话、显式 key、`?model=`、无状态临时对话、参数拒绝、额度冷却、客户端断开时停止、登出与验证页 → NeedsAttention、关闭 Job 结束浏览器。
- [x] 2026-10-15 daemon e2e（`apps/loadout/tests/web_session_e2e.rs`，真实 daemon + headless Edge + mock 页，约 13 s）：功能未开启时拒绝添加；别名确认不符时拒绝；就绪与模型列表；不测速、不进虚拟模型；Chat 非流式、Messages、Responses 流式三个入口返回内容与页面流逐字一致且接在同一对话；`temperature` 被拒且未输入页面；状态中的默认对话（auto）与今日计数；删除账号删除 profile。测试后无残留 Edge 进程。
- [x] 2026-10-15 Stage 6 CLI：`loadout account web add|status|show|recheck|bind|config`；`account transport … on` 对网页账号显示网页会话风险说明（按 provider kind 的 `auth == browser_login` 判断，不按 slug）；`config set web_session.enabled true` 打印风险说明。
- [x] 2026-10-15 Stage 7 桌面端：账号页"ChatGPT 网页会话"入口与添加对话框（风险列表 → 勾选 → 输入别名，必要时同时开启 `web_session.enabled`）；网页账号卡片显示 D 级标识、实时状态、需要处理的原因、今日次数、默认对话，提供"打开窗口""我已处理""会话设置"（模式、间隔、上限、默认对话）；网页账号隐藏测速 / 检测 / 密钥按钮；开启转发对话框显示网页会话风险；设置页开启 `web_session.enabled` 前弹出风险确认；IPC 白名单新增 6 个方法。验证：`npm run build`（含 tsc）；在内置浏览器中用模拟 IPC 预览（未在桌面上做 GUI 自动化）账号页、"我已处理"状态切换、开启转发对话框、添加对话框、会话设置对话框。
- [x] 2026-10-15 Stage 8 文档：`docs/providers/web-chatgpt.md`（条款、原则、用法、对话映射、降级、技术说明、待 live test 确认的项目）、providers 索引、README、AGENTS（命令与平台注意事项）、system-test-plan（网页会话 live test 清单）。
- [x] 2026-10-15 复查后补充：答案流开始后 10 分钟没有新数据时报错并停止页面生成（防止一直占用会话）；答案流里的限额消息同样触发冷却。
- [x] 2026-10-15 benchmark（§11）：基线是改动前提交 `d657f54` 在独立 worktree 中用 `--save-baseline before017` 跑的结果，改动后在同一个 target 目录用 `--baseline-lenient before017` 对比（结果见下表）。

### Benchmarks（Harness §20）

本机，均值。机器在测试期间同时有其他负载：`route_selection/provider_model_100_targets` 第一次对比为 −9.9%，同一份代码第二次对比为 +14.9%，以下数字的波动在 ±15% 左右。

| Benchmark | 改动后 | criterion 判定（第二次） |
|---|---|---|
| `route_selection/provider_model_100_targets` | 649 ns | +14.9%（第一次 −9.9%，噪声） |
| `route_selection/virtual_100_targets_with_session` | 6.76 µs | −13.9% |
| `route_selection/exact_100_targets` | 227 ns | −3.7% |
| `route_selection/provider_model_100_web_gates`（新增） | 691 ns | — 100 个目标全部带 gate，比无 gate 多约 40 ns |
| `snapshot_swap/build_100_targets_3_virtuals` | 39.0 µs | −5.8% |
| `snapshot_swap/load` / `load_during_swaps` | 6.4 / 6.5 ns | −4.0% / −5.3% |
| `stream_proxy/passthrough_200_events` | 169 µs | −6.0% |
| `stream_proxy/convert_chat_to_messages_200_events` | 417 µs | −8.6% |
| `ttft/direct_upstream` | 92.0 µs | 无变化 |
| `ttft/via_gateway_passthrough` | 602 µs | +3.4% |
| `ttft/via_gateway_converted_messages` | 622 µs | +4.4% |

结论：HTTP 目标只多了一次 `Option` 判空，没有可测回退（ttft 的 +3–4% 与 ExecPlan 016 观察到的本机波动同量级，且同一轮中其他 gateway 项为负）。gate 读取是几次原子 load，100 个目标约 40 ns。Loadout 自身在浏览器路径上的开销（选路到首个 `TextDelta`）没有单独计时；集成测试中 mock 页的整个往返在数百毫秒量级，主要是页面与 Edge 的时间。
- [ ] 所有者 live test（Stage 8；清单见 system-test-plan"ChatGPT 网页会话"一节）
