# 同类项目功能调研与可接入功能（2026-10-04）

> 讨论稿，不是决定。落地前按 AGENTS.md §6 建 ExecPlan；凡是改变路由行为、数据保留或凭证存储的条目，先写或修订 ADR。
> 原始调研笔记（READMEs、元数据、下载的源码片段）在本次会话的 scratchpad，未纳入仓库。

## 0. 结论速览

| 编号 | 方向 | 价值 | 工作量 | 需要 ADR？ | 建议 |
|---|---|---|---|---|---|
| 1 | 一键把 Loadout 配置写入客户端 | 高 | M | 否（每个接入单独铸造 client key 即可） | **P1** |
| 2 | Claude Code statusline 命令 | 高 | S | 否 | **P1** |
| 3 | Windows 通知 + 账号终态（封禁 / 过期 / 欠费） | 高 | M | 否 | **P1** |
| 4 | 账号 / 模型可用性检测、流式健康检查 | 中高 | S–M | 否 | **P1** |
| 5 | 价格目录同步 + 跨服务商比价 | 中高 | M | 否（展示）/ 是（按价格排序） | **P2**，先修价格不更新的 bug |
| 6 | 额度感知路由（软降级 / 按 reset 排序） | 中 | M | **是**（修订 ADR-003 §9） | **P2** |
| 7 | 日志里的决策链、缓存命中统计、数据来源标签 | 中 | S | 否 | P2 |
| 8 | 模型别名 / 按客户端改写、更丰富的 `/v1/models` | 中 | S | 否（需遵守 ADR-010 不偷换模型） | P2 |
| 9 | 深度链接导出（cc-switch / Cherry Studio）、Webhook、从 cc-switch/CCR 导入、WebDAV 同步 | 低中 | S–M | 否 | P3 |
| — | 自动签到、MCP/Skills 同步、Token 压缩、TLS 伪装 | — | — | — | **不做**（理由见 §6） |

---

## 1. 一键把 Loadout 配置写入客户端（all-api-hub / cc-switch 思路）

### 1.1 各客户端的接入方式（Windows）

| 客户端 | 方式 | 位置 | 关键字段 | 可行性 |
|---|---|---|---|---|
| Claude Code CLI | `settings.json` 的 `env` | `%USERPROFILE%\.claude\settings.json`（或 `CLAUDE_CONFIG_DIR`） | `ANTHROPIC_BASE_URL=http://127.0.0.1:8317`、`ANTHROPIC_AUTH_TOKEN`、`ANTHROPIC_MODEL`、`ANTHROPIC_DEFAULT_{OPUS,SONNET,HAIKU}_MODEL` | 写文件，设置实时生效 |
| Codex CLI / IDE | `config.toml` 自定义 provider | `%USERPROFILE%\.codex\config.toml` | `model_provider="loadout"`、`[model_providers.loadout] base_url="http://127.0.0.1:8317/v1", wire_api="responses"` + key | 写文件（必须用保留注释的 TOML 编辑器）。**绝不碰 `auth.json`**：否则会清掉 ChatGPT 登录，Loadout 读取 Codex 用量也依赖它 |
| OpenCode | provider 条目 | `%USERPROFILE%\.config\opencode\opencode.json[c]` | `provider.loadout{npm:"@ai-sdk/openai-compatible", options{baseURL, apiKey}, models}` | 写文件（JSONC） |
| Cherry Studio | 深度链接 | — | `cherrystudio://providers/api-keys?v=1&data=<base64>`（来自源码，官方未文档化） | 深度链接 |
| Roo Code | 设置导入 | VS Code `roo-cline.autoImportSettingsPath` | `providerProfiles.apiConfigs` | 第 2 期 |
| Continue / Zed / Aider / SDK / Cline | — | — | — | 复制粘贴卡片 |
| Cursor | — | — | — | **不可能**：请求从 Cursor 云端发出，访问不到 127.0.0.1；ADR-011 也不允许把 Loadout 暴露到公网 |
| Gemini CLI | — | — | — | 不适用：需要 Gemini 原生协议，Loadout 未提供 |

### 1.2 设计要点

- **每个接入单独铸造一把 client key**（如名为 `claude-code`）。原因：
  - Loadout 只保存 client key 的哈希（ExecPlan 006），无法取回旧 key；
  - 每个应用一把 key，可以单独吊销、单独限额，统计页也能按应用区分。
  - 如果改成"key 可取回"，再配合 Claude Code 的 `apiKeyHelper` / Codex 的 `auth.command`，客户端文件里就可以不放明文 key。但这涉及凭证存储，需要修订 ADR-004，列为第 2 期。
- **写入流程**（借鉴 cc-switch 的写入引擎，MIT）：
  1. 解析原文件；解析失败就拒绝写入，**绝不从空文档开始写**（cc-switch 有两次事故就是因此清空了用户配置）；
  2. 只改 Loadout 拥有的键，其他键一律不动；
  3. 保留注释：TOML 用 `toml_edit`，JSONC 用 CST 编辑器，或在有注释时拒绝写入；
  4. 首次写入前做字节级备份，每次写入另存快照；
  5. 写临时文件，重新检查原文件哈希没有变化后，用 `ReplaceFileW` 原子替换；
  6. 写完重新解析，验证结果。
- **恢复：三方合并**（借鉴 EasyCLIProxyAPI，MIT）：只有仍等于 Loadout 写入值的键才回滚到原值，用户后来自己改的值保留。
- **状态检测**：读回配置文件，显示"已指向 Loadout / 被外部修改 / 未配置"；检测客户端进程是否在运行，提示用户重启。
- **第 2 期：启动时注入（CCR 模式）**：`loadout run claude|codex` 只给子进程注入环境变量，不改任何文件，是最安全的方式。
- 所有写入都在 Core 里经 IPC 完成（ADR-002），不在热路径上；日志和备份列表不打印 key；备份放在 `%LOCALAPPDATA%\Loadout`，只有当前用户可读。

### 1.3 风险

- **客户端配置格式变化很快**：Codex 的 `wire_api`、Kilo 7.x 的新格式都变过。每个客户端都要像 Provider 一样有一份文档，记录"最后验证日期"。
- **Claude Code**：使用网关凭证时，claude.ai 订阅的限额不再生效，Remote Control 和语音功能会被禁用；如果同时保存了 claude.ai 登录，会出现认证冲突提示。Anthropic 不支持经 `ANTHROPIC_BASE_URL` 使用非 Claude 模型。UI 里要写清楚。
- **深度链接把 key 放在 URL 里**，系统或浏览器可能记录。只能通过 ShellExecute 打开，生成前要让用户确认。

### 1.4 可移植代码（MIT，需记入 THIRD_PARTY_NOTICES）

- cc-switch：
  - `src-tauri/src/config.rs`（原子写，约 399–610 行）；
  - `live/engine.rs`（哈希复查、首次写入备份）；
  - `live/patch/{toml,json}.rs`；
  - `live/floor.rs`（Loadout 拥有哪些键）。
- EasyCLIProxyAPI：
  - `src-tauri/src/agents/configuration.rs`（Claude env 写入约 550 行，Codex 的 `toml_edit` 写入约 2695 行）；
  - `agents/transactions.rs`、`agents/backups.rs`（三方恢复、带校验的备份）；
  - `agents/windows_probe.rs`（安装与进程探测）。

---

## 2. Claude Code statusline（新发现，S）

- `loadout statusline`：读取 Claude Code 通过 stdin 传入的 JSON，输出一行状态：当前路由、该 client key 今日花费 / 日限额、剩余额度。
- 经 IPC 向 daemon 查询，设置短超时；daemon 未运行时输出空行，不能卡住 Claude Code。
- **附带收益**：Claude Code 用订阅运行时，stdin JSON 里带有官方的 `rate_limits.five_hour` / `seven_day`（已对照官方文档）。statusline 命令可以把它记下来，成为 Claude 订阅**不联网、不读 token、数据又是官方的**用量来源，比桌面应用的本地文件更新、更准。
  - 注意：Claude Code 改走 Loadout 之后就不再使用订阅，这个字段会消失。所以"显示 Loadout 状态"和"采集订阅用量"是两个使用场景。
- 可选（未验证）：Loadout 在响应头里返回 `anthropic-ratelimit-unified-*`，让 Claude Code 原生显示 client key 的花费额度。需要先做原型验证。

---

## 3. 跨服务商比价（all-api-hub 思路）

### 3.1 先修现有 bug

`crates/storage/src/account_models.rs:106` 的 `COALESCE(input_price, ?)` 会让已经存在的价格永远不被目录同步更新；数据库也没有记录价格是用户填的还是目录同步来的。修复方式：

- 新增 migration，加 `price_source`（`user` / `catalog` / `models.dev` / …）和 `price_updated_at`；
- 用户填的价格永远优先，目录同步来的价格可以被刷新。

### 3.2 数据源

| 来源 | 许可证 | 评价 |
|---|---|---|
| **models.dev `api.json`** | MIT | **主数据源**。226 家服务商、8386 个条目，按每 1M token 计价，含缓存价格与 `canonical_model_id`（可以解决大部分模型匹配问题）。注意：coding plan 类服务商标的 `cost: 0` 表示"订阅内含"，不是免费 |
| LiteLLM `model_prices_and_context_window.json` | MIT（仓库根目录，不在 enterprise/ 里） | 补缺。按每 token 计价，细节多，但没有统一的模型 id |
| pydantic genai-prices | MIT | 匹配规则最好（`api_pattern` 正则、`starts_with` 等），可以用 Rust 重新实现同样的规则 |
| OpenRouter `/models`、`/endpoints` | 无开放数据许可 | 只给有 OpenRouter 账号的用户实时拉取，**不打包、不分发** |
| all-api-hub | AGPL | 只借鉴思路：缺失价格不当 0；只比较同单位、完整的报价；明确标注"本地估算" |

### 3.3 功能

- **MVP**：
  - 打包一份过滤后的价格快照（几百 KB），可选在后台每周刷新一次（带 ETag，可关闭），只在 Core 里运行；
  - `loadout price compare <模型>` 和 UI 对比抽屉：同一个 canonical 模型在各账号的价格，用**用户自己近 30 天的 token 构成**算加权价格，并列出 TTFT、TPS、延迟档位和价格来源；
  - "本月如果走别家要花多少"：用 `request_events` 按其他账号的价格重新计算；
  - 订阅价值倍数：订阅等价 API 花费 ÷ 月费（产品文档 §34 已有设想，尚未实现）。
- **后续**：
  - 价格变动提醒；
  - 按价格给 Pool 排序：需要修订 ADR-003，且换成不同模型仍须用户显式开启（ADR-010）；
  - 长上下文阶梯价、1 小时缓存写入、批量价、DeepSeek 错峰价、人民币计价。
- **匹配策略**：每个 Provider 模块声明自己对应的 models.dev / LiteLLM provider id（能力优先，ADR-005）→ 精确查表 → 规范化后的 token key（只接受唯一匹配）→ 用户手动关联（`loadout model link`）。价格优先级：用户 > 服务商实时目录 > models.dev > LiteLLM > 未知。**未知价格永远不当 0。**

---

## 4. 额度感知路由（AntigravityManager / OmniRoute 思路）

### 4.1 现状

- 已有：`routing.quota_reserve_percent`（默认 0，即关闭）。任一额度窗口剩余低于该比例时，账号**直接退出**候选（ADR-003 §9 允许）。这和 AntigravityManager 的"低于 5% 换号"等价。
- 缺口：
  1. 只能全局设置，不能按账号或按窗口设置（例如"5 小时窗口留 10%，周窗口留 20%"）；
  2. 只能排除，不能"降级到链尾"；
  3. 没有"优先用先重置的账号"；
  4. `codex-sub` 账号还没有用量数据，阈值对它不起作用。Codex 响应头里带有 primary/secondary 窗口，可以直接利用；也可以和同一 ChatGPT 账号的 `codex` 用量账号关联。
- 更新（ExecPlan 013）：缺口 1、4 已完成——按账号 / 按窗口的保留阈值（`loadout account reserve`），`codex-sub` 用量来自响应头（被动）和 `wham/usage`（后备轮询）。

### 4.2 方案与 ADR 影响

| 方案 | 行为 | ADR-003 |
|---|---|---|
| A. 按账号 / 按窗口的保留阈值 | 仍是排除，只是阈值更细 | §9 允许，**不需要修订** |
| B. 软降级 | 低于阈值的账号移到候选链尾部，作为最后兜底 | 改变候选顺序，**需要修订** §5 / §9（§9 已预留 pressure 评分作为后续扩展） |
| C. 按 reset 时间 / 剩余量排序 | 同档位内，优先用先重置或剩余多的账号 | **需要修订**；而且会削弱粘性、破坏 prompt cache，必须有滞后机制 |
| D. 缓存亲和 | 同一个 prompt 前缀固定走同一账号（只用内存哈希表） | 与 §5 的会话粘性同类，需要说明 |

**建议**：先做 A（零 ADR 风险）和 `codex-sub` 的用量来源；B 写进 ADR-003 修订后再做，C、D 等有真实流量数据再说。所有计算都放在快照构建阶段完成，跑 benchmark。

---

## 5. 其他值得接入的功能（按主题）

- **健康与检测（P1）**：
  - 每个账号一个"检测"按钮：key 有效 / 失效 / 限流 / 欠费；
  - 每个模型 1 token 的可用性测试（消耗极少额度，需要用户主动触发）；
  - 流式健康检查：SSE 帧是否合法、流是否正常结束（扩展 `loadout bench`）。
- **账号终态（P1）**：区分"暂时冷却"和"封禁 / 登录过期 / 欠费"。进入终态的账号移出快照，直到用户处理。
- **通知（P1）**：
  - Windows Toast：额度或余额低、账号 401 或封禁、登录即将过期、达到日花费上限、连续 failover、daemon 崩溃；
  - 由 daemon 发出事件，托盘应用显示，不涉及热路径；
  - Webhook（飞书 / 钉钉 / Discord）作为 P3。
- **透明度（P2）**：
  - 日志记录决策链：候选、跳过原因、failover 次数（只存元数据，符合 ADR-008）；
  - 缓存命中率与节省金额；
  - 每个数字标注来源：官方 / 本地估算 / 未知。
- **调试（需 ADR）**：限时开启的请求 / 响应正文捕获加重放。与 ADR-008"正文不落盘"冲突，只能在修订 ADR 后作为默认关闭、自动过期、脱敏的功能。
- **模型（P2）**：
  - 全局别名（如 `sonnet` 指向某个 Pool）、按客户端改写，必须遵守 ADR-010 不偷换模型；
  - `/v1/models` 带上上下文长度、价格、档位、健康状态；
  - 路径前缀兼容（`/anthropic/v1/messages`，P3）。
- **额度预测（P2）**：消耗速率和"预计 HH:MM 用完"。
- **导入与同步（P3）**：
  - 从 cc-switch（`~/.cc-switch` SQLite）、CCR、CLIProxyAPI 导入配置，secret 直接存入凭证管理器；
  - 加密的 WebDAV 配置同步（不同步 secret）。
- **中转站余额模板（P3）**：New API 类站点的 `/api/user/self` 声明式余额读取（cc-switch 的模板已验证）。

## 6. 不建议做

- **自动签到**：需要网站 session 并自动执行网站操作，可能违反站点条款。至多提醒用户"打开签到页"。
- **MCP / Skills / 提示词同步**：这是 cc-switch 的核心功能，面很大，与 Loadout 的定位相邻但不重合。至多做互通。
- **Token 压缩**（RTK 等）：会改写提示词，与"不偷换"和保真原则冲突。
- **TLS 指纹伪装**（OmniRoute 等）：ADR-012 已排除。

---

## 7. 新发现的项目（已补入 reference-map）

| 项目 | Stars | 许可证 | 借鉴点 |
|---|---|---|---|
| farion1231/cc-switch | ~140k | MIT（已确认） | 客户端配置写入引擎、深度链接、models.dev 价格、用量模板 |
| SaladDay/cc-switch-cli | 5.2k | MIT | speedtest、stream-check、fetch-models |
| diegosouzapw/OmniRoute | 72.6k | MIT | 额度感知 / 按 reset 排序 / 缓存亲和路由、三层失败处理、决策响应头、Webhook |
| decolua/9router | 30.2k | MIT | 订阅 → 便宜 → 免费的回退组合、reset 倒计时 |
| kaitranntt/ccs | 2.9k | MIT | 按运行时 / 配置启动、认证健康监控 |
| ding113/claude-code-hub | 3.4k | MIT | 5 小时 / 周 / 月花费上限、每请求决策链 |
| maximhq/bifrost | 8.5k | Apache-2.0 | 按路径兼容各家 base URL、分层预算 |
| yym68686/uni-api | 1.3k | Apache-2.0 | 渠道冷却、通配模型权限、`5/hour` 限速语法 |
| looplj/axonhub | 5.3k | Apache-2.0 | 每请求时间线、含缓存 token 的成本 |
| Maciek-roboblog/Claude-Code-Usage-Monitor | 8.7k | MIT | 经 statusline 采集官方额度、数字来源标签、消耗预测 |
| sirmalloc/ccstatusline | 13.2k | MIT | statusline 组件 |
| junhoyeo/tokscale | 5.6k | MIT | 15+ 种 agent 本地日志解析 |
| anomalyco/models.dev | 7.0k | MIT | 价格 / 模型元数据 |
| pydantic/genai-prices | — | MIT | 模型匹配规则 |
| weiruchenai1/api-key-tester | 220 | MIT | 批量检测 key 状态 |
| bestruirui/octopus | 2.7k | AGPL | 仅借鉴思路：面向个人、价格与模型自动同步 |

更正：

- sub2api 为 LGPL-3.0；
- ccusage 已迁至 `ccusage/ccusage` 并改用 Rust，许可证按路径核对；
- TensorZero 已归档，RouteLLM 自 2024-08 起停更。
