# yetone/magpie 竞品调研（2026-10-23）

> 讨论稿，不是决定。落地前按 AGENTS.md §6 建 ExecPlan；凡是改变路由行为、数据保留、请求内容或凭证处理的条目，先写或修订 ADR。
> 研究对象：[yetone/magpie](https://github.com/yetone/magpie)，固定 commit `3dc25a61681fac109c12f4b065146b9204cdc37e`（2026-10-23，release v0.1.1333）。浅 clone 在本次会话的 scratchpad，未纳入仓库。`site/`（官网与图片）和部分 testdata 没有检出；少数文件通过 `git show HEAD:<path>` 读取，没有运行仓库里的任何代码。
> **许可证**：根 `LICENSE` 为 MIT（Copyright 2026 yetone），可以移植，但要按 AGENTS.md §5 逐个文件核对，并记入 `THIRD_PARTY_NOTICES.md`。已知例外：`internal/qoder/endpoints.go` 带有 Router-For.ME / Luis Pater（CLIProxyAPI）的版权头。
> 同名的 Windows 窗口放大工具 Blinue/Magpie 是另一个项目，见 [2026-10-23-magpie-review.md](2026-10-23-magpie-review.md)。

## 0. 结论速览

**这是 Loadout 最直接的竞品**：同样是本地网关，加一键改写 agent 配置、订阅接入和路由组。区别在于：

- **覆盖面大得多**：64 个内置 agent、WSL 内的 agent、macOS / Linux / Docker / Termux，还有插件、库、联网搜索；
- **迭代极快**：代码由 agent 编写、合并和发布，一个月约 1300 个 release；
- **在服务条款和安全上激进得多**：
  - 读取其他工具的凭证，并把刷新后的 token 写回；
  - 冒充官方客户端（Codex、Antigravity、OpenCode 的 UA / originator）；
  - 密钥明文保存；
  - 本机请求不校验 key；
  - 插件运行时没有沙箱。

它自己把 11 个有封号风险的订阅接入移出主程序，改由社区插件提供，理由是"避免 magpie 本身被封"。

**对 Loadout 的意义**：
- 拼覆盖面拼不过它。Loadout 的差异应该是**合规与安全姿态**（ADR-004 / 009 / 011 / 012）、**热路径纪律**和 **Windows 原生质量**。magpie 主动撤下高风险接入，反过来证明 Loadout 当初选择的方向是对的。
- 但它在 agent 接入工程、粘性与 failover 的细节、计价正确性上明显更成熟，值得直接学。对照它，还发现了 `loadout setup` 的几处实际缺陷（§1.1）。

| 编号 | 方向 | 价值 | 工作量 | 需要 ADR？ | 建议 |
|---|---|---|---|---|---|
| 1 | 修 `loadout setup` 的实际缺陷：重复 apply 后 restore 回不到原始文件、Codex 不校验、`ANTHROPIC_MODEL` 压过 `/model`、client key 只增不减 | 高 | S | 否 | **P1** |
| 2 | Agent 接入引擎：保格式编辑、stash + applied 账本（选择性恢复、漂移检测）、Claude Code 分档模型变量、桌面端一键接入 | 高 | M | 否 | **P1** |
| 3 | 粘性"按缓存是否还值得保留"：只在完整成功后记录，流中断时清除 | 高 | S–M | **是**（修订 ADR-003 §5，与 AxonHub §1.1 合并） | **P1** |
| 4 | 跨 provider 推理签名：被拒后去掉签名重试一次，并按会话记住被拒的哈希 | 中高 | S | 否 | **P1**（AxonHub §1.3 的具体解法） |
| 5 | WorkBuddy 错误码 11128 / 11140 / 14018 记入文档与错误分类 | 中 | S | 否（补充 ADR-016 的风险说明） | **P1**（文档） |
| 6 | 计价正确性：5m / 1h 缓存写入、长上下文阶梯、未定价单列、读取时重新计价、models.dev 目录 | 中高 | M | 否 | **P2**（并入 AxonHub §1.7、ponytail §2.5） |
| 7 | 首字节前的稳健性：空回复重试、按模型与按账号分开冷却、只探测一个冷却中的候选 | 中 | M | 否（拒答后换模型除外，见 §2.3） | **P2** |
| 8 | 额度提醒："窗口快要重置了但还剩很多" | 中 | S | 否 | **P2** |
| 9 | client key 多窗口限额（周 / 月 / 每 N 小时）+ 在途预留 | 中 | S–M | 否（扩展 ADR-011 §2） | **P2** |
| 10 | 公开集成契约：发现接口、按会话查路由、查用量、上报用量 | 中 | S–M | 部分（记录会话 id 需修订 ADR-013 §4） | **P2** |
| 11 | Claude 用量改用官方 CLI 读取（`claude -p /usage`） | 中 | S | 否（ADR-009 §2 官方 CLI 优先） | **P2**，先实测 |
| 12 | 请求进行中阻止系统睡眠；从 CC Switch 只读导入；"添加到 Loadout" 导入链接 | 低中 | S–M | 否 | P3 |
| — | Claude CLI 常驻桥接、读写其他工具的凭证、冒充 UA、预热刷窗口、明文密钥、本机不校验 key、无沙箱插件与 JS 中间件、意图路由、负载均衡式路由、联网搜索循环、局域网共享、遥测 | — | — | — | **不做**（§4） |

---

## 1. P1

### 1.1 `loadout setup` 的实际缺陷（P1，S）

对照 magpie 的 `internal/agent`、`internal/edit` 后核对了 `apps/loadout/src/bin/loadout/setup_cmd.rs`，发现以下问题：

1. **重复 apply 后 restore 回不到原始文件**：`backup()`（`:84-100`）的注释写的是"before the first change"，实际每次 apply 都会备份一次（`:335`）。`restore` 取最新的备份（`:276-297`）。所以 apply 两次之后，最新的备份已经是第一次接入 Loadout 后的配置，restore 只能回到那里。原文件不存在的情况也一样：`.absent` 标记会被第二次 apply 产生的备份盖过，restore 不会删除文件。
   - magpie 的做法：`stash.json` 只在**第一次接入时**记下原来的值，断开时只恢复 magpie 改过的键（`internal/agent/stash.go`）。
2. **Codex 不做解析校验**：JSON 文件 apply 前会先解析，Codex 的 TOML 则直接走 `codex_config` 按行改写（`:186-230`），写完也不校验。
   - magpie 的做法：用 TOML 解析器取得字段的位置再改写，写入前重新校验（`internal/edit/toml.go`）。
3. **写入 `ANTHROPIC_MODEL`**（`:145`）：它会压过用户在 Claude Code 里用 `/model` 做的选择。也没有写分档变量：`ANTHROPIC_DEFAULT_{OPUS,SONNET,HAIKU}_MODEL`、`ANTHROPIC_SMALL_FAST_MODEL`、`CLAUDE_CODE_SUBAGENT_MODEL`。
   - magpie 的做法：写入分档变量，并**主动删除** `ANTHROPIC_MODEL`（`internal/agent/claude.go` 的 `writeTiers`）。
4. **client key 只增不减**：每次 apply 都新建一个 `<client>-<秒>` 的 client key（`:310-314`），restore 不作废它，重复执行会留下一串无主的 key。
5. **JSON 被重新格式化**：serde_json 能保留键的顺序，但不保留空白；带注释的 JSONC 会被拒绝。写入时没有 fsync，不保留 BOM / CRLF，符号链接会被替换成普通文件。

**建议**：先修 1–4。
- 备份只在"文件还没指向 Loadout"时做，或者改成 stash 账本（见 §1.2）；
- Codex 改用 `toml_edit` 并在写入前校验；
- Claude Code 改写分档变量，不再写 `ANTHROPIC_MODEL`；
- 再次 apply 时复用该客户端已有的 key，restore 时提示（或自动）作废。

第 5 点并入 §1.2。

### 1.2 Agent 接入引擎（P1，M）

**magpie**：
- **保格式编辑**（`internal/edit`）：
  - JSON / JSONC：用 `tidwall/jsonc` 把注释替换成等长的空白，再用 `gjson` 找到目标值的字节偏移，只替换那一段，其余字节原样保留。
  - TOML：用 go-toml 的 `unstable` 解析器取得字段位置后按行改写，自己的表整体替换，写入前重新校验。
  - 所有格式保留 BOM 和 CRLF。
- **原子写入**：先写临时文件，fsync，再 rename；保留文件权限，符号链接写到目标文件，硬链接原地写；多文件编辑失败时整体回滚。
- **两本账**：
  - `stash.json`：接入前的原值，断开时恢复；
  - `applied.json`：magpie 自己写入的值。用它判断漂移（`unwired` / `replaced` / `bypassed` / `unreachable` / `proxied`），用户可以选择重新应用，或接受现状。
- **断开预览**：在临时 HOME 里用子进程真实执行一次断开，再对比文件差异（`preview.go`）。
- **检测已安装的 agent**：看配置文件、目录（排除只剩 `skills/`、日志的残留目录）和可执行文件（PATH、npm、bun、Volta、pnpm、`~/.local/bin`）。
- **缺陷**：卸载 magpie 时不会断开已接入的 agent，这些 agent 会一直指向一个已经不存在的网关。

**Loadout 现状**：
- CLI 的 `loadout setup` 只支持 Claude Code / Codex / OpenCode 三个 agent，状态检查只是子串匹配（`points_at_loadout`）；
- 桌面端只显示环境变量片段（`apps/desktop/src/pages/Clients.tsx`）；
- 2026-10-04 调研 §1.2 设计的三方恢复没有实现。

**建议**：
- 把 `setup` 的逻辑下沉到 Core，经 IPC 提供给 CLI 和桌面端，桌面端做一键接入。
- 编辑层：TOML 用 `toml_edit`（它原生保留格式）。JSONC 用 CST 解析器，或按 magpie 的思路"偏移拼接"。保留 BOM / CRLF，写入时 fsync，符号链接写到目标文件。
- 用 stash + applied 两本账代替"整文件备份"，支持只恢复 Loadout 改过的键、漂移提示，以及卸载时逐个断开（magpie 没有做这一点，我们补上）。
- 覆盖面先求稳：Claude Code、Codex、OpenCode，再加 Gemini CLI、Qwen Code、Cline 这类配置格式稳定的 agent。每个 agent 都在临时 HOME 下测试接入、切换和断开，并按 ponytail 调研 §2.6 记录已验证版本。不追 64 个：magpie 的各 agent 特例约 3.9 万行，维护成本很高。
- 可移植性：`internal/edit` 是 MIT，可以参考思路用 Rust 重写；直接移植的文件要记入 NOTICE。

### 1.3 粘性"按缓存是否还值得保留"（P1，需修订 ADR-003 §5）

**magpie**（`internal/gateway/affinity.go`）：
- **会话标识**：按顺序取 8 个客户端会话头中的第一个（含 `x-claude-code-session-id`、`x-opencode-session` 等）。都没有时，用第一条 user 消息原始 JSON 字节的 SHA-256。
- **auto 模式**：
  - 同一轮内始终粘住；
  - 跨轮时，只有上一次回复读或写了至少 1024 个缓存 token，**并且**距今不到 5 分钟（或者上游报告了 1h 缓存写入且仍在有效期内），才继续粘住；
  - 否则重新按路由规则选择。账号冷却中、用量 ≥98%、回复中途断开、上下文超出窗口时也会解除粘性。
- **只在完整成功后记录**（`answered`），回复中断时清除。

**Loadout 现状**：
- 只认 `X-Loadout-Session-ID`，固定 2 小时 TTL（`crates/router/src/session.rs`）；
- 在 commit point（首字节）就记录，流中断也不清除（`crates/gateway/src/proxy/mod.rs:277`、`:485`）。

**建议**：与 AxonHub 调研 §1.1 合并成一个 ExecPlan：
- 会话标识改用 `ConversationSeed`。它只哈希文本、不受 `cache_control` 位置影响，又绑定了 client 和账号，**比 magpie 的原始字节哈希更好**。magpie 的哈希在客户端移动缓存标记时会变化，导致第二轮被当成新会话。
- TTL 按缓存实际情况决定：上一次回复确实用到了缓存，且仍在 5 分钟 / 1 小时的有效期内，才继续粘住。
- 粘性只在回复完整成功后写入，回复中断时清除。
- **不照搬**：magpie 把粘性存到 `affinity.json`，未命中时会 `os.Stat` 这个文件，等于在热路径上做文件 I/O。Loadout 继续只放内存。

### 1.4 跨 provider 推理签名（P1，S）

**magpie**：
- 自己 API 的签名（`Part.Sealed`）只回写给同一种 API；
- 遇到别家 Responses 的 `encrypted_content` 被拒时，先去掉推理内容重试一次，再去掉压缩内容重试，并按"会话 + 账号"记住被拒条目的 SHA-256，之后直接剔除（`refusedSeals`、`withoutRefused`）；
- Gemini 的签名存在磁盘上，借工具调用 id 携带（**不照搬**：落盘不符合 ADR-008，改写 tool id 也有风险）。

**Loadout 现状**：`crates/protocol/src/messages.rs:283` 原样透传所有签名。AxonHub 调研 §1.3 建议"先写测试确认会不会出错"。

**建议**：先用 AxonHub §1.3 的测试确认问题存在。如果存在，采用 magpie 这种"被拒后重试一次并记住"的方案：只在内存里记、只对已知的拒绝错误生效、不增加正常请求的开销。

### 1.5 WorkBuddy 错误码（P1，文档）

magpie 的测试用例（`internal/gateway/workbuddy_restricted_test.go`、`prompt_refused_test.go`）记录了 WorkBuddy 的三类错误：
- `11128` "Illegal API invocation from an unapproved channel"：magpie 认为由**提示词内容**触发，不让账号进入冷却；
- `11140` "request illegal"：账号受限；
- `14018` "Credits exhausted"：额度用完。

magpie 的 WorkBuddy 插件为了通过 11128 检查，改写过提示词。

**Loadout 现状**：`docs/providers/workbuddy.md` 和 ADR-016 都没有提到这些错误码。Loadout 的桥接会把调用方的 system prompt 原样转发，同样可能触发 11128。

**建议**：
- 把这三个错误码写进 `docs/providers/workbuddy.md`，注明来源是 magpie 的测试用例、Loadout 未实测；
- 在 WorkBuddy 的错误分类里区分"提示词被拒"（不冷却、不换账号）、"账号受限"、"额度用完"；
- **不跟进**改写提示词绕过检查（ADR-016 / ADR-012 的反伪装原则）。

---

## 2. P2 / P3

### 2.1 计价正确性（P2，并入 AxonHub §1.7、ponytail §2.5）

**magpie**：
- **价格来源**依次为：用户自定价 → `<provider>/*` → provider 自己的目录 → models.dev（按模型厂商）→"同某某模型"。
- **缓存写入**：第四个价格是 5m 写入（1.25× 输入），第五个价格是 1h 写入（Claude 默认 2× 输入），按 `ephemeral_5m/1h_input_tokens` 拆分。
- **长上下文阶梯**：提示词超过阈值后，整次调用按阶梯价计费。
- **未定价**：不计入合计，单独显示"N 次调用未定价"。
- **读取时重新计价**：改价后历史合计会跟着变化。
- **目录刷新**：models.dev 目录超过 24 小时就刷新；离线时退回 OpenCode 的缓存，再退回内置的一小份列表。

**Loadout 现状**：
- 缓存价缺失时回退到输入价（`crates/router/src/target.rs:36-37`），不拆分 5m / 1h，没有长上下文阶梯；
- `estimated_cost_usd` 在写入时就固定下来；
- ExecPlan 011 §5 把"内置 models.dev 价格快照"列为未做。

**建议**：三份调研的计价条目合成一个 ExecPlan。
- "读取时重新计价"要在 Stats 里实现：`request_events` 已经存了各类 token，可以按当前价格重新计算。保留写入时的估算作为"当时的估算"。
- models.dev 在后台刷新，不在热路径上，符合 Harness §2.2。

### 2.2 额度提醒与 client key 限额（P2）

- **额度提醒**（`internal/provider/quota_alert.go`）：
  - 每 5 分钟检查一次，提醒三类情况：用量百分比、余额，以及"窗口 ≥24 小时、在设定的提前量内就要重置、但已用不到 85%"。
  - 每个窗口只提醒一次。
  - Loadout 的 `crates/core/src/notify.rs` 有额度低、余额低、认证失败、登录将过期、client 花费上限，**没有"快重置但还剩很多"**。可以补上，S 级。
- **client key 限额**（`internal/access/limit.go`）：
  - 每个 key 最多 5 条限额，单位是 token 和 / 或美元，窗口可以是日 / 周 / 月（本地时间）或每 N 天 / 每 N 小时；
  - 请求进行中先占用额度，超额时返回 429 和 `Retry-After`；
  - Loadout 的 `ClientPolicy` 只有按 UTC 日计算的 token / 花费、rpm 和 allowed_models，也没有在途预留，并发请求会一起越过上限。
  - 扩展它与 ADR-011 §2 不冲突。
  - **不照搬**：magpie 的 key 明文保存在 `caller-keys.json`，Loadout 只存哈希。

### 2.3 首字节前的稳健性（P2）

magpie 的做法（`internal/gateway/fallback.go`、`routing.go`）：
- **首字节前**：先把响应头和错误压住，等第一个内容到达再放出；推理阶段最多压 4 分钟，这样推理结束后才出现的拒答仍能换候选。
- **空回复**：重试。
- **冷却分两层**：按"模型"和按"key / 账号"分开计算。两个账号对同一模型都返回 403 时，判定是请求本身的问题，撤销两者的冷却。
- **只探测一个**：冷却中的候选排在最后，并且只探测最快恢复的那一个（`probeOne`）。
- **全员繁忙**：用 SSE 注释保活，同时退避重排（最长 60 秒）。

Loadout 的 `FailureKind::fails_over` 和 commit point 机制与此等价，但粒度更粗。

- **可以学**：空回复重试、两层冷却、撤销误判的冷却、`probeOne`、等待期间保活。
- **不学或要先过 ADR**：
  - 拒答或上下文过长后换到别的模型：对精确模型违反 AGENTS §4 规则 7 / ADR-010，只能用于虚拟模型和 Pool；
  - 流中断后用前缀续写：可能产生重复内容，而且 ADR-003 §7 规定此时以流内错误结束；
  - 按多语言正则词表分类错误：维护成本高，仍以结构化错误码为主。

### 2.4 公开集成契约（P2）

magpie 的 `docs/integrating.md` 给第三方应用提供了三个层次：
- **直接用网关**：`GET /api/hello` 用于发现；按 key 名或 User-Agent 归属调用方；`X-Magpie-Session` 加 `GET /v1/magpie/route?session=` 在首个 token 前返回本轮选了哪个模型、试过哪些候选；`/v1/magpie/quotas` 和 `/v1/magpie/usage` 查询额度与用量；`POST /v1/magpie/usage/requests` 让应用上报自己直连厂商的用量。
- **在 Agents 页加一行**：写一个 adapter，提交 PR。
- **写插件**。

Loadout 的 IPC 只供内部使用，CLI 的 `--json` 输出也没有文档。

**建议**：结合 ponytail 调研 §2.1 / §2.4（按会话聚合、statusline 显示本会话）和 Blinue/Magpie 调研 §1.1（对外契约）：
- 先在 gateway 上提供只读的 `/loadout/v1/route?session=` 和用量查询，**必须带 client key**，并且只返回这个 key 自己的数据（ADR-011）；
- 记录会话 id 与 ADR-013 §4"对话 ID 不写入数据库、日志或请求记录"冲突，需要先修订该节。magpie 只记录客户端发来的会话 id、从不自己生成，这与 ponytail §2.1 里"只取显式来源"的建议一致。

### 2.5 Claude 用量改走官方 CLI（P2，先实测）

magpie 用 `claude -p /usage` 读取 Claude 订阅用量（`claude_usage_cli.go`），并从流里的 `rate_limit_event` 被动更新。只有会话文件发生变化时才去读，间隔是 5–15 分钟内随机。

ADR-009 §2 规定"官方 CLI 优先"。这比 Loadout 目前"用 CLI 凭证调私有端点"的风险低。

**需要先实测**：
- `/usage` 在 `-p` 模式下是否消耗额度；
- 用户只装了 Claude 桌面应用时，自带的 `claude.exe`（ExecPlan 008）能否使用；
- 每次读取启动一个 CLI 进程的开销（只在后台做，不违反 Harness §2.4）。

### 2.6 其他（P3）

- **请求进行中阻止系统睡眠**：magpie 在有请求时调用 `SetThreadExecutionState`，结束后再留一段宽限期（`internal/awake`）。长时间运行的 agent 任务可以避免笔记本中途睡眠。放在 `loadout_platform` 里实现，默认关闭。
- **从 CC Switch 只读导入**：以 `mode=ro` 打开 `~/.cc-switch/cc-switch.db`，逐项让用户确认（`internal/provider/importapps.go`）。2026-10-04 调研已经列为 P3。
- **"添加到 Loadout" 导入链接**：HKCU 注册 URL scheme。magpie 的校验规则值得照抄：
  - 只接受 `import` 动作；base URL 必须是 https（私网和 loopback 除外），不得含 userinfo、query、fragment；
  - key、名称、模型数量都有长度上限；
  - 解析结果用随机 id 暂存，只能读取一次，用户确认后才保存。

---

## 3. 我们已经做对的（magpie 反而证明了这些选择）

- **本机也要 client key**：Loadout 只存 key 的 SHA-256，即使是 loopback 也要求 key，并检查 Host / Origin（ADR-011）。magpie 在 loopback 上接受任意 token。它虽然有防 DNS rebinding 的 Host 检查，但本机任何进程都能花用户的订阅额度。
- **凭证存 Credential Manager**（ADR-004）。magpie 的 `logins.json`、`providers.json`、`plugin-auth.json` 都是明文 JSON（0600 权限，Windows 上意义有限）。
- **ADR-009 / 012 的服务条款姿态**：magpie 自己把 11 个订阅接入移到社区插件，理由是避免主程序被封，并且所有用户都被强制迁移、不能撤回。这说明这些接入的风险是真实存在的。
- **ADR-016 / ADR-017**：WorkBuddy 走 IPC 桥接，OpenCode 用官方 `serve`，都没有冒充 UA、改写提示词，比 magpie 的做法干净。缺少的只是 §1.5 的风险说明。
- **ConversationSeed** 比 magpie 的会话哈希更稳（§1.3）。
- **有状态 Responses**：ADR-006 固定路由，无法固定时明确报错；magpie 在转换时总是发 `store:false`。
- **数据保留**：`request_events` 存在 SQLite，保留 400 天后清理；magpie 的 `usage.jsonl` 没有清理机制，会无限增长。
- **加密导出**：ChaCha20-Poly1305 + PBKDF2 600k，与 magpie 的 AES-256-GCM + PBKDF2 600k 相当。
- **daemon 与 GUI 分离**（ADR-002）：magpie 的 GUI 和网关在同一个进程里。

## 4. 不建议照搬

| magpie 做法 | 为什么不做 |
|---|---|
| Claude 订阅：每个对话常驻一个真正的 `claude` 进程（`-p --dangerously-skip-permissions --setting-sources ""`），把调用方的工具经 MCP 桥接进去 | ADR-012 已经否决了方案 B。Windows 上进程数没有上限，空闲时每个约 205 MiB。是否违反 Anthropic 条款，它的文档避而不谈 |
| 读取并刷新 `~/.codex/auth.json`，把轮换后的 token **写回** Codex 的存储；在 `.credentials.json` 里切换多个 Claude 账号；导入 CLIProxyAPI 等工具的文件（会把源工具登出） | ADR-009 §2、ADR-004 §2、ADR-012 |
| 冒充官方客户端：Codex 的 `originator` / UA、强制 HTTP/1.1、Antigravity 的 UA（它的代码注释承认 Google"可能封停"这类账号）、OpenCode 的 UA | ADR-012（反伪装，TLS 指纹同理） |
| 每天预热：用 "hi" 提前开启 5 小时窗口 | 属于刷窗口；ADR-009 规定健康检查不发推理请求 |
| 明文密钥、loopback 接受任意 token | ADR-004、ADR-011 |
| 无沙箱的 Bun 插件（自动下载、自动升级），把有条款风险的代码"外包"给插件；JS 网关中间件（改写或屏蔽请求、加系统提示词） | ADR-007 插件隔离；改写提示词与 ADR-006 及前几份调研"不改写提示词"的结论冲突 |
| 意图路由：每轮先调一次小模型做分类（默认超时 8 秒）；首个请求最多等 3 秒拉取额度；热路径上 `os.Stat` | AGENTS §4 规则 1 / ADR-003 |
| smart / pace / usage / rotate / weight 负载均衡 | ADR-003 §5 / §9。pace 与 AxonHub §1.8 重合，那里已经标为需要 ADR 的 P2 |
| 联网搜索：网关内部的工具循环，由另一个模型或搜索 API 代为搜索 | 实际上是另一个模型在回答，涉及 ADR-010 / 规则 7 |
| 局域网共享、远程 magpie（默认明文 HTTP） | ADR-011 §1 |
| 库：把指令、MCP、技能、子代理写进各个 agent | 2026-10-04 调研 §6 已经排除 |
| 每日遥测（PostHog） | 与本地优先的定位不符，要做也必须先写 ADR |

## 5. 流程上可以借鉴的

这些来自 magpie 的 `docs/code-standards.md`、`LESSONS.md` 和 `docs/subsystems/README.md`。由于它的代码全部由 agent 编写和合并，它的规则写得非常具体：

- **读取或解析失败是"未知"，不是"空"**：访问控制失败时一律拒绝。Loadout 的 Stats 在 SQL `SUM` 里静默跳过未定价调用（`estimated_cost_usd` 为 NULL），合计偏低却没有任何提示，就属于这一类（§2.1）。
- **修复要覆盖所有同类路径**：一处限制要覆盖到达同一资源的每条路径，例如 `count_tokens` 也要受限。
- **修复必须附带测试，且没有修复时这个测试会失败**：审查者回退修复，确认测试真的变红。
- **默认值永远不覆盖用户设置过的值**。
- **"当前行为"参考文档**：每个子系统维护一份当前行为参考，接口、状态、失败行为一旦改变，就在同一个 PR 里更新。Loadout 的 ADR 记录"决定"，ExecPlan 记录"过程"，但缺少一份随代码更新的"现在是什么样"。可以考虑给 Gateway、Router、更新器先各写一份。

## 6. 参考文件清单（commit `3dc25a6`，MIT，按文件核对）

| 主题 | 路径 |
|---|---|
| 子系统索引与审查规则 | `docs/subsystems/README.md`、`AGENTS.md`、`docs/code-standards.md`、`LESSONS.md` |
| 路由、failover、粘性 | `docs/subsystems/gateway-routing.md`、`internal/gateway/routing.go`、`fallback.go`、`affinity.go`、`patience.go`、`internal/routes/strategies.go` |
| 协议转换与签名 | `internal/gateway/ir.go`、`anthropic.go`、`responses.go`、`gemini_sigstore.go` |
| 会话与上下文统计 | `docs/subsystems/gateway-sessions.md`、`internal/sessions/`、`internal/gateway/prompt.go` |
| Agent 接入与保格式编辑 | `docs/subsystems/agent-wiring.md`、`internal/agent/`（`claude.go`、`codex.go`、`stash.go`、`applied.go`、`preview.go`、`wsl.go`）、`internal/edit/` |
| 导入与导入链接 | `internal/provider/importapps.go`、`internal/provider/importurl.go`、`internal/gui/importlink.go` |
| 计价与目录 | `internal/catalog/`、`docs/reference.md`（What a model costs） |
| 额度提醒 | `internal/provider/quota_alert.go` |
| client key 限额 | `internal/access/access.go`、`limit.go` |
| 集成契约 | `docs/integrating.md` |
| WorkBuddy 错误码 | `internal/gateway/workbuddy_restricted_test.go`、`prompt_refused_test.go` |
| 阻止睡眠 | `internal/awake/` |
| 反面参考 | `docs/subsystems/claude-subscription-bridge.md`、`internal/gateway/claude_subscription.go`、`internal/provider/account.go`、`codex_request.go`、`google.go`、`internal/plugin/host.go`、`internal/provider/migrate*.go` |
