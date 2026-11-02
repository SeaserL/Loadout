# Ponytail 调研：对 Loadout 的启发（2026-10-23）

> 讨论稿，不是决定。落地前按 AGENTS.md §6 建 ExecPlan；凡是改变路由行为、数据保留或请求内容的条目，先写或修订 ADR。
> 研究对象：[DietrichGebert/ponytail](https://github.com/DietrichGebert/ponytail)，固定 commit `09c434ab73ca43761c5d8b0c5d341926c2fba710`（2026-10-18，v5.3.2 之后）。源码 clone 在本次会话的 scratchpad，未纳入仓库。
> **许可证**：MIT（根 `LICENSE`）。本文只借鉴概念与方法，没有复制代码；若以后移植具体文件，按 AGENTS.md §5 记入 `THIRD_PARTY_NOTICES.md`。
> 数字均来自作者自己的 benchmark 报告（`benchmarks/results/`），未经我们复现。

## 0. 结论速览

Ponytail 是 coding agent 的**提示词插件**，不是网关。它和 Loadout 的关系是：它在 agent 一侧改变 agent 的行为，Loadout 在 agent 和模型之间转发请求。所以能借鉴的不是它的提示词本身，而是三样东西：**按任务 / 会话的成本面板**、**严谨的 agent 级 A/B 测评方法**、**多 agent 适配的维护方式**。

| 编号 | 方向 | 价值 | 工作量 | 需要 ADR？ | 建议 |
|---|---|---|---|---|---|
| 1 | 请求日志记录会话标识，Stats 按会话聚合（网关版 Band / Board） | 高 | M | **是**（修订 ADR-008 §1 字段清单与 ADR-013 §4）+ migration | **P1** |
| 2 | Agent 级 A/B 验证：直连 vs 经 Loadout 透传 vs 跨协议转换 | 高 | M | 否 | **P1** |
| 3 | 版本号一致性检查（Cargo / package.json / tauri.conf / tag） | 中 | S | 否 | **P1**（顺手） |
| 4 | statusline 显示"本会话"的路由与真实花费；以后再考虑 Claude Code band 模块 | 中高 | S–M | 否（依赖 1） | **P2** |
| 5 | 价格估算：缓存价未知时不回退到输入价，拆分 5m / 1h 缓存写入 | 中 | S | 否 | **P2**（并入 AxonHub 调研 §1.7） |
| 6 | 客户端接入矩阵（含"已验证版本"）+ 桌面端一键接入 | 中 | S–M | 否 | **P2** |
| 7 | AGENTS.md 增补几条改动纪律（波及面清单、先复用、收尾写未验证项） | 中 | S | 否 | **P2**，由仓库所有者决定 |
| — | 网关注入规则 / 提示词预设、代码地图、复用率、Skills 分发、`shortcut:` 注释约定 | — | — | — | **不做**（§4） |

---

## 1. Ponytail 是什么

**它不是网关，也不是代码库**，而是一段给 coding agent 的提示词（"lazy senior dev"），加上把这段提示词装进 20 多种 agent 的适配层、度量工具和 benchmark。JavaScript / Python / TypeScript，约 17 万 star，2026-06 创建，迭代很快（363 个 commit）。

| 组成 | 位置 | 作用 |
|---|---|---|
| 核心提示词 | `skills/ponytail/SKILL.md`、精简版 `AGENTS.md` | 先读懂改动波及面，再按"阶梯"选最小完整改动 |
| 审查 / 审计 skill | `skills/ponytail-review`、`ponytail-audit` | 按"正确 → 安全 → 负载 → 测试 → 速度 → 精简"排序；每条发现必须有具体失败场景 |
| 债务账本 | `skills/ponytail-debt` | 收集代码里的 `shortcut: <上限>, <何时升级>` 注释 |
| 生命周期 hook | `hooks/ponytail-activate.js` 等 | SessionStart / SubagentStart / UserPromptSubmit 注入规则、记住档位、压缩后恢复 |
| 代码地图 | `hooks/ponytail-map.js` | 会话开始时注入 ≤2000 字符的"每个目录导出了哪些名字"，让"先复用"不必先搜索 |
| Band / Board | `hooks/band/`、`hooks/ponytail-band-import.js` | Claude Code 提示框上方的面板：本任务花费、token、agent 时间、复用率、测试是否通过；本地 CSV 账本 + HTML 看板 |
| 多宿主适配 | `.claude-plugin/`、`.codex-plugin/`、`.opencode/`、`.cursor/`、`gemini-extension.json` 等 | 一份规则，薄适配层；`docs/agent-portability.md` 记录每个宿主的加载方式与"已验证版本" |
| Benchmark | `benchmarks/agentic/` | 真实 headless Claude Code 会话在种子仓库里改代码，按留下的文件打分 |

### 1.1 核心规则（摘要）

- 写之前：读任务和它触及的代码，列出改动必须到达的所有地方（调用方、测试、fixture、配置、导出）。这是范围，额外功能不是。
- 选第一个能完整解决的选项：不需要就不做 → 代码库已有 → 标准库 / 平台 → 已装依赖 → 一行 → 最少代码。
- 绝不省：信任边界上的校验、防数据丢失的错误处理、安全、无障碍。
- 非平凡逻辑（分支、循环、解析、金额、安全）留一个小测试。
- 回复末尾一两行：跳过了什么、没验证什么、用户必须知道的风险。

### 1.2 作者自报的效果（2026-10-07，Opus 5.5，39 任务 × 5 次）

- 相对无 skill：代码 -53%、输出 token -45%、成本 -26%、时间 -41%；隐藏检查通过率持平（96.7% vs 95.6%），安全类任务三组都是 30/30。
- 需要测试的逻辑留下测试：98% vs 68%；注入 bug 被 agent 自己的测试抓到：66% vs 46%。
- **反例（2026-06 单轮测评）**：省钱只在 Claude 上成立。gpt-5.4-mini 贵 26%，gpt-5.5 贵 39% 且不更快——每次请求都重发整段规则，推理模型本来输出就短，输入和推理 token 的开销盖过了省下的代码。

### 1.3 Benchmark 方法里值得学的部分

- 基线是"同一个 agent 不加 skill"，不是裸模型；还放了一句话提示词（"Follow YAGNI principles."）做对照，检验复杂规则是否真的比一句话强。
- 各组按任务交替运行，让时段和模型漂移同等地影响每组。
- 每个工具都带 `good` / `bad` 参考实现，`--selftest` 必须先证明 good 通过、bad 被抓到，**才允许花一分钱 API**；LLM 评审也要先通过自检（必须把过度设计的参考排在最简参考之上）。
- 隔离：每个会话在 git 仓库之外的临时目录，关闭 `CLAUDE.md` 和自动记忆，只加载被测插件；作者发现早期版本把被测仓库的 commit message 泄露给了模型，修复后重跑。
- 统计：每任务取 5 次中位数，算与基线的比值，再对任务取几何平均 + 95% bootstrap 区间；"更好的任务数"用符号检验。所有工作区保留，可离线 `--rescore`。
- 测试质量用变异测试：往 agent 写的代码里注入小 bug，看它自己的测试能抓到多少。

---

## 2. 值得做的（P1 / P2）

### 2.1 按会话记录与聚合用量（P1，需修订 ADR-008 与 ADR-013）

**Ponytail**：
- Band 在每轮结束后记录本任务的花费、token、agent 时间、轮数、模型，写入每个项目一个 CSV 账本，再生成本地 HTML 看板（`hooks/band/register.tsx` 的 `record` / `writeBoard`）。
- `ponytail-band-import.js` 解析 Claude Code 会话 JSONL 补录历史会话，价格按 Anthropic 价目表在客户端计算。
- 局限：只支持 Claude Code；花费按 Anthropic 价目在客户端估算。Claude Code 经 Loadout 路由到其他 provider 时，这个数字不反映真实花费（推断，未验证）。

**Loadout 现状**：
- `request_events` 记录了 token、缓存读写、TTFT、估算成本、failover 链（`crates/gateway/src/proxy/meter.rs:132-178`）。
- 但**没有会话标识**。`ConversationSeed`（`crates/gateway/src/proxy/session.rs:33-56`）每个请求都会算，只用于发给上游的 session（ADR-013），不进入 `RequestRecord`（`crates/gateway/src/plane.rs:19-46`）。
- `request_stats` 只能按 model / route / provider / client / day 分组（`crates/storage/src/stats.rs:22-55`）。

**为什么值得做**：Loadout 能看到所有 agent（Claude Code、Codex、OpenCode……）的全部流量，也知道真实上游和价格。"这次编码会话花了多少、走了哪些账号、缓存命中多少、failover 几次"只有网关能跨 agent 给出准确答案。这是 Ponytail 在客户端做不到的。

**建议**：
- `request_events` 加一列 `session_key`：
  - **只取显式来源**（客户端 session header、`prompt_cache_key`）的哈希。不用"对话开头"兜底：那是提示词内容的哈希，存进 SQLite 与 ADR-008 的精神不符，而且不同会话的开头可能相同，会被合并成一个会话。
  - 用与发给上游的 UUID 不同的 domain 前缀，日志里的值不能和上游会话 id 对上。
- Stats 增加按会话分组：请求数、花费、token、缓存命中率、起止时间、用到的 provider / account、failover 次数。Logs 增加按会话筛选。
- Claude Code 的 subagent 请求如果带父会话的 header，会自然汇总到父会话（需实测）。Ponytail 的 band 要单独累加 subagent token，网关一侧不需要。
- ADR-013 §4 目前规定"对话 ID 不写入数据库、日志或请求记录"，必须先修订：明确"只记录显式来源的、另行加盐的哈希"是允许的。yetone/magpie 也只记录客户端发来的会话 id，从不自己生成（见 [2026-10-23-yetone-magpie-review.md](2026-10-23-yetone-magpie-review.md) §2.4）。
- 工程约束：migration + 测试（Harness §21）；修订 ADR-008 §1 字段清单；值在 meter 里写入，不影响选路前热路径，但属于 Gateway 改动，按规则 9 跑 benchmark。

### 2.2 Agent 级 A/B 验证（P1）

**Ponytail**：见 §1.3。核心是：基线就是同一个 agent，先自检工具再花钱，各组交替运行，多次取中位数，保留工作区以便离线重算。

**Loadout 现状**：
- `loadout-benchmarks` 只有热路径微基准（Criterion）。CI 只编译，不运行。
- 真实 agent 端到端只有 `scripts/verify_opencode_coding.py`：只覆盖 OpenCode，单任务，单次，pass / fail（ExecPlan 029）。Claude Code、Codex 没有同类验证。
- 没有和"agent 直连上游"对比过，所以说不出"经 Loadout 后 agent 有没有变差、贵了还是便宜了"。

**建议**：把 `verify_opencode_coding.py` 扩展成多 agent、多组的验证脚本：
- **分组**：
  - 直连：agent 直接连 provider，用 API key 账号；
  - 经 Loadout 透传：同一账号、同一模型；
  - 经 Loadout 跨协议转换：例如 Claude Code 接 OpenAI 兼容上游，或 Codex 的 Responses 请求转到 Chat 上游。
- **任务**：5–10 个带隐藏检查的小种子仓库，沿用 calculator 任务的写法：测试文件哈希不变、独立跑测试、至少若干轮工具调用。
- **指标**：
  - 隐藏检查通过率、轮数、工具轮数；
  - token、缓存读取占比、花费：经 Loadout 的组直接读 `request_events`，数字准确；
  - TTFT。
- **花钱之前先自检**：好 / 坏参考实现、relay 是否生效、隔离是否生效（临时目录在 git 仓库之外，不加载本仓库的 `AGENTS.md` / 记忆）。这部分不调 API，可以进 CI。真实运行必须设 `LOADOUT_LIVE_TESTS=1`，并且每次由用户确认，因为会消耗额度。
- **直接用途**：AxonHub 调研的 P1 项（跨协议自动补 `cache_control`、过滤外来 reasoning signature）在改之前和改之后各跑一次，用缓存命中率和通过率说话。
- **Ponytail 踩过的坑**：
  - 隔离泄漏：早期把被测仓库的 commit message 漏给了模型；
  - OpenAI 自动 prompt cache 会让重复的相同提示词的 token 统计塌缩。测成本时要单独记录缓存 token，并让每次运行的提示词有所不同。
- 订阅 transport 仍受 ADR-009 / ADR-012 约束：经 Loadout 的组只用 API key 账号。

### 2.3 版本号一致性检查（P1，S）

**Ponytail**：`scripts/check-versions.js` 要求 7 个文件的版本号相同；在 release tag 上还要求等于 tag。只比较文件之间是否一致不够：v4.8.0 发布时所有 manifest 一起停在 4.7.0，互相"一致"，测试照样通过（作者在脚本注释里记录了这次事故）。

**Loadout 现状**：版本号写在三个地方：`Cargo.toml:21`、`apps/desktop/package.json:4`、`apps/desktop/src-tauri/tauri.conf.json:4`，目前都是 0.4.0，但没有任何检查。`scripts/package.ps1` 只读 cargo 的版本给 zip 命名。更新器拿 GitHub `tag_name` 和 `crate::VERSION` 比较（`crates/core/src/updates.rs`）。所以 tag 和版本号不一致时，更新流程会出错：`packaging/update.ps1` 装完后会校验 `loadout --version`。

**建议**：
- 在现有 contract tests 里加一个测试，要求三处版本号相同；
- `package.ps1` 在版本号不一致时拒绝打包；当前 HEAD 恰好有 tag 时，还要求版本号等于 tag。

### 2.4 statusline 显示本会话；Claude Code band 模块（P2，依赖 2.1）

**Ponytail**：
- 用 Claude Code（≥ 2.1.290）插件的 `modules` 在提示框上方渲染 band，带按钮和 toast；
- 用到的接口有 `$.session.id/usage/cwd`、`$.http.fetch`、`$.ui.toast/status`、`$.fs`、`$.process.run`；
- 不在 VS Code 面板、云端会话和 `claude -p` 中显示。

**Loadout 现状**：`loadout statusline`（`apps/loadout/src/bin/loadout/statusline_cmd.rs`）显示今日总花费和最紧的额度，IPC 超时 800 ms，daemon 不在时直接返回空。

**建议**：
- 先做 S 级的改进：Claude Code 传给 statusline 的 stdin JSON 带 `session_id`。有了 2.1 的 `session_key`，statusline 可以显示"本会话 $x · provider/account · failover n 次"，替代 Claude Code 自己按 Anthropic 价目算出的花费。
  - 前提：实测 statusline 的 `session_id` 与请求头 `x-claude-code-session-id` 是否相同。
- band 模块（toast 提示 failover / 额度将尽，按钮打开 Loadout 或切换子号池）等 API 稳定后再说。它只是一个 IPC 客户端，必须保持 fail-open，不能拖慢 Claude Code。

### 2.5 价格估算：不知道就不猜（P2，并入 AxonHub §1.7）

**Ponytail**：`ponytail-band-import.js` 的原则是"空着的成本好过猜出来的成本"：不认识的模型、fast 模式都不估算；缓存写入分 5m / 1h 两种价格；Haiku 5.5 的提示词超过 10 万 token 时用更高的单价。

**Loadout 现状**：
- `Prices::cost`（`crates/router/src/target.rs:36-37`）在缓存价缺失时**回退到输入价**。按 Anthropic 的价格结构，缓存读取约为输入价的 0.1 倍，所以缓存读取会被高估约 10 倍；1h 缓存写入是输入价的 2 倍，会被低估。
- `decode_usage`（`crates/protocol/src/messages.rs:405`）只读 `cache_creation_input_tokens`，不读 `cache_creation.ephemeral_5m_input_tokens` / `ephemeral_1h_input_tokens`。

**建议**：
- 缓存价未知、缓存 token 又大于 0 时，把这条成本标成"不完整"，不要静默按输入价计算。Stats 显示"价格不全的请求数"，而不是直接把它们从合计里去掉。
- 5m / 1h 拆分和长上下文阶梯价并入 AxonHub 调研 §1.7 一起做。

### 2.6 客户端接入矩阵（P2）

**Ponytail**：
- `docs/agent-portability.md` 为 30 多个宿主逐一写明加载文件、方式、限制和"Verified with <宿主> <版本>"；
- 规则是"适配层要薄，一份事实来源"；
- 许多宿主共用格式：Kilo Code 直接运行 OpenCode 插件，Qwen Code 兼容 Gemini CLI 扩展，CodeBuddy 读取 Claude 格式的 manifest。

**Loadout 现状**：
- `loadout setup`（`apps/loadout/src/bin/loadout/setup_cmd.rs`）覆盖 Claude Code、Codex、OpenCode，会备份、原子写入、拒绝改写无法解析的文件，支持 `--restore`。
- 桌面端没有一键接入，Clients 页只显示通用的环境变量片段。2026-10-04 调研 §1 的 P1 只完成了 CLI 部分。

**建议**：
- 新建 `docs/clients/` 接入矩阵，每个 agent 一行：
  - 配置位置；
  - 协议（Anthropic / Chat / Responses）；
  - 模型名写法；
  - `loadout setup` 写入哪些内容；
  - 已验证版本和日期；
  - 已知限制，例如 Cursor 连不到本地网关。
  - 格式参照 `docs/providers/<provider>.md` 的"最后验证日期"。
- 共用格式的 agent（Kilo Code ≈ OpenCode 等）可能只需复用现有的 writer，但每个都要先实测，不能凭 Ponytail 的文档推断。
- 桌面端的一键接入：调用与 `loadout setup` 相同的 IPC 或逻辑，不再单独写一套。

### 2.7 AGENTS.md 增补几条改动纪律（P2，可选）

`docs/development-harness.md` 管架构约束（热路径、secret、ADR、ExecPlan），但没有关于"改动本身"的规则：最小改动、先复用仓库里已有的代码、审查报告格式都没有写。可以用我们自己的话，在 AGENTS.md 加最多 5 行：

- 动手前列出改动必须同步的位置：调用方、测试、fixture，以及 Loadout 常见的跨层链路 `crates/ipc/src/api.rs` → `crates/core/src/dispatcher.rs` → 桌面端 bridge → `apps/desktop/src/i18n.tsx` → README / docs。
- 先找仓库里已有的 helper 和模式；不为只有一种实现的东西加抽象，不加没人要的选项。
- 收尾写明哪些没验证、还剩什么风险。
- 审查意见必须附具体的失败场景。
- ExecPlan 里"后续可选项"写明触发条件。例如 ExecPlan 008 中"按 ccusage 方式估算 token 用量，记为后续可选项"就没有写何时重新考虑。

注意：
- Ponytail 自己的数据显示，规则的效果取决于模型；而且每一行规则每轮都会重发，增加输入 token。AGENTS.md 已经很长，宁少勿多。
- Harness 要求的 ExecPlan、ADR、测试优先于任何"最少代码"倾向。
- 是否在个人的 Claude Code 里安装 Ponytail 插件，由仓库所有者自己决定，不写进仓库规则。

---

## 3. 我们已经做对、无需再学的

- `loadout setup` 会备份、原子写入、拒绝改写无法解析的文件，并支持 `--restore`。这与 Ponytail 的 `scripts/cursor-hooks.js` 一致：合并写入、只删除自己写的条目、遇到格式错误的文件拒绝修改。
- statusline 经 IPC 调用，800 ms 超时，失败时什么都不输出。Ponytail 的所有 hook 也是 fail-open 加超时，它在 Windows 上多次修过"stdin 不关闭导致卡死""node 启动慢导致超时"之类的问题。
- Python 脚本读 JSON 时用 `utf-8-sig`。Ponytail 为 Windows 编辑器写入的 BOM 修过好几次 bug。
- `request_events` 已有缓存读写 token 和 `cache_savings_usd`，比 Ponytail 的看板更细。
- 请求正文默认不落盘（ADR-008）。Ponytail 的看板默认也只存在本机，两边的隐私立场一致。

## 4. 不建议照搬

- **在网关注入 Ponytail 类规则或"按 client key 的提示词预设"**：
  - 违背 ADR-006 透传时按字节保真的要求，以及 2026-10-04 调研 §6 "不改写提示词"的结论；
  - 改变 system 前缀会破坏上游的 prompt cache；
  - Ponytail 自己的数据显示，同一套规则在 GPT 推理模型上贵 26–39%。Loadout 会把同一个 agent 路由到不同模型，注入规则会让成本随路由悄悄变化；
  - 规则应该放在 agent 一侧。按 Ponytail 的矩阵，`AGENTS.md` 已经能被约 20 个 agent 读取。
  - （OpenCode bridge 插件追加的执行上下文属于上游适配，已在 ADR-017 记录，不是这一类。）
- **代码地图、复用率、"最后一次改动后测试是否通过"**：需要读用户的仓库，并在客户端挂 hook，不是网关该做的事。
- **Skills / 插件分发与同步**：2026-10-04 调研 §6 已排除。
- **`shortcut:` 注释约定与债务账本**：Loadout 代码里没有 TODO / FIXME 标记，延期事项写在 ExecPlan 里，现在的做法更好。只借鉴"写明触发条件"（§2.7）。

## 5. 参考文件清单（commit `09c434a`）

| 主题 | 路径 |
|---|---|
| 核心规则 | `skills/ponytail/SKILL.md`、`AGENTS.md` |
| 审查格式 | `skills/ponytail-review/SKILL.md` |
| 会话激活、subagent 注入、压缩后恢复 | `hooks/ponytail-activate.js`、`hooks/ponytail-subagent.js`、`hooks/claude-codex-hooks.json` |
| Band / Board | `hooks/band/register.tsx`、`hooks/band/metrics.ts`、`hooks/claude-band.json` |
| 历史会话导入与价格表 | `hooks/ponytail-band-import.js` |
| 多宿主适配矩阵 | `docs/agent-portability.md` |
| 配置合并写入 | `scripts/cursor-hooks.js`、`scripts/uninstall.js` |
| 版本一致性 | `scripts/check-versions.js` |
| Agent 级 benchmark | `benchmarks/agentic/README.md`、`run.py`、`judge.py`、`complete.py` |
| 结果报告 | `benchmarks/results/2026-10-07-agentic.md`、`2026-06-17-cost-verification.md` |
