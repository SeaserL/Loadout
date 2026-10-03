# Loadout

> **All your AI, one endpoint.**

Loadout 是一个运行在你自己 Windows 电脑上的 **个人 AI 控制面板与网关**：把你已经拥有的 API key、API 余额、编码订阅、本地模型和兼容网关整合成一个可观察的"算力池"，并在 `127.0.0.1` 上提供一个 OpenAI / Anthropic 兼容的接口。所有配置、日志与密钥都留在本机。

- 一个地址、一把 client key，接入任意支持 OpenAI 或 Anthropic API 的工具（OpenCode、Claude Code、Cursor 类工具、自己的脚本…）
- 按 **延迟分级**（超快 / 快 / 普通）自动选路：`loadout/realtime`、`loadout/fast`、`loadout/default`，同一档内按输出速度排序
- 账号额度用完、限流、出错时 **自动切换到下一条路由**；平时稳定使用同一条（保护上游 prompt cache）
- 订阅与 API 的 **用量 / 余额 / 续费 / 订阅价值** 一览（Codex、Claude Code、Gemini CLI 只读用量；ChatGPT 订阅可选择单独登录并开启转发，见 ADR-012）
- 请求日志只存元数据（不存提示词与回复），统计、按 client 限额、配置备份与恢复

状态：**v0.2.0**，系统测试中（新增：Claude 桌面应用用量识别、可选的 Codex 订阅转发、桌面端改版）。

---

## 安装

### 从发布包安装（推荐）

1. 解压 `Loadout-0.2.0-windows-x64.zip`。
2. 双击 `install.cmd`（或 `powershell -ExecutionPolicy Bypass -File install.ps1`）。可选参数：
   - `-AddToPath`：在新终端里可以直接使用 `loadout` 命令
   - `-Autostart`：登录 Windows 时自动启动网关
   - `-NoStart`：安装后不立即启动
3. 从开始菜单打开 **Loadout**。

| 内容 | 位置 |
|---|---|
| 程序 | `%LOCALAPPDATA%\Programs\Loadout`（`loadout-app.exe` 桌面端、`loadoutd.exe` 网关、`loadout.exe` 命令行） |
| 数据 | `%LOCALAPPDATA%\Loadout`（数据库、日志、备份） |
| API key | Windows 凭据管理器（目标名以 `loadout:` 开头），**从不**写入数据库或日志 |

升级：解压新版本后再次运行 `install.cmd`（会先优雅停止网关，替换程序后重新启动）。
卸载：`powershell -ExecutionPolicy Bypass -File "%LOCALAPPDATA%\Programs\Loadout\uninstall.ps1"`，加 `-RemoveData` 同时删除数据与已保存的 API key。

### 从源码构建

需要：Rust（版本由 `rust-toolchain.toml` 固定）、Visual Studio Build Tools（C++）、Node.js LTS、WebView2。

```text
npm install
npm run build                                     # 桌面端前端（必须先于 cargo 构建）
cargo build --release -p loadout -p loadout-desktop
powershell -ExecutionPolicy Bypass -File scripts\package.ps1   # 生成 dist\Loadout-<版本>-windows-x64.zip
```

开发规则见 [AGENTS.md](AGENTS.md)。

---

## 快速开始

### 桌面端

1. **Accounts & usage → Add account**：选择提供方（OpenCode Go、Command Code、Kimi Code、OpenRouter、DeepSeek、OpenAI、Anthropic、Ollama，或任意 OpenAI / Anthropic 兼容端点），填入 API key，模型列表会自动同步。
   **Scan this PC** 可发现本机已登录的 Codex / Claude Code / Gemini CLI / Kimi Code，导入后显示其额度。Claude Code 也能从 Claude 桌面应用识别；每个订阅提供方卡片上的“用量来源”可切换为“本地文件（不联网）”以降低风险（CLI：`loadout provider set claude-code usage_source=local`）。
2. **Client keys → New client key**：为每个应用创建一把 key（只显示一次）。
3. 应用里设置：Base URL `http://127.0.0.1:8317/v1`，API key 为上面的 client key，模型名见下文。
4. **Routes → Measure unmeasured routes**：测一次延迟，`loadout/realtime` 与 `loadout/fast` 才会有可用路由（真实请求也会持续计入）。

关闭窗口只是隐藏到托盘，网关继续运行；托盘菜单可以启动 / 停止网关或退出桌面端。界面默认简体中文，可在侧栏底部或 设置 → 外观 切换为 English（托盘菜单同步切换）。

### 命令行

```text
loadout daemon start
loadout provider add openrouter
loadout account add openrouter main          # 提示输入 API key（不回显），并同步模型
loadout client create my-app                 # 输出 client key（只显示一次）
loadout model list                           # 可用的 provider/account/model 名称
loadout bench                                # 测量所有未测速的路由
loadout route list                           # 各虚拟模型与 Pool 的路由顺序
loadout route status                         # 每条路由的健康状态、Tier、TTFT、tok/s
loadout usage --refresh                      # 额度、余额、订阅计划、本月花费
loadout logs --errors --hours 24             # 最近失败的请求
loadout stats --by route --days 7            # 按路由统计
loadout --help                               # 全部命令
```

---

## 模型名

| 写法 | 含义 |
|---|---|
| `loadout/default` | 所有已启用模型，最快的在前（测得的 Tier，再按输出速度） |
| `loadout/fast` | 仅"超快"和"快"两档的路由 |
| `loadout/realtime` | 仅"超快"路由（默认 P90 首 token 时间 < 800 ms，可在设置中调整） |
| `loadout/<pool>` | 你自己建的 Pool：手动顺序或按延迟排序（`loadout pool set …` / Routes 页面） |
| `provider/account/model` | 精确到账号，例如 `openrouter/main/anthropic/claude-sonnet-4.5`；该账号不可用时会切到同一提供方其他账号的同一模型 |
| `provider/model` | 该提供方任一账号上的该模型 |

请求头（可选）：`X-Loadout-Session-ID`（同一会话固定同一路由）、`X-Loadout-No-Failover: true`（不切换）。响应头 `X-Loadout-Route` 告诉你实际用了哪条路由。

如果某个应用只能发送固定的模型名（如 `gpt-4o`），在 **Client keys → Policy** 给它设置 "Default model"。同一处还可以限制可用模型、每日 token / 费用上限、每分钟请求数。

### 接入示例

```text
# OpenAI 兼容（OpenCode、大多数 SDK 与工具）
OPENAI_BASE_URL=http://127.0.0.1:8317/v1
OPENAI_API_KEY=lo-…

# Anthropic 兼容（Claude Code、Anthropic SDK）
ANTHROPIC_BASE_URL=http://127.0.0.1:8317
ANTHROPIC_API_KEY=lo-…
```

支持的接口：`/v1/chat/completions`、`/v1/responses`（无状态完整支持；`previous_response_id` 仅在上游原生支持 Responses 时固定路由转发）、`/v1/messages`、`/v1/models`。协议不同时自动转换（流式同样支持）。

---

## 提供方

每个提供方的认证方式、接口、用量来源与风险说明见 [docs/providers](docs/providers/README.md)。官方 CLI 的订阅登录（Codex、Claude Code、Gemini CLI、Kimi Code CLI）**只用于读取用量**，不会用来转发请求（[ADR-009](docs/adr/ADR-009-subscription-credential-scope.md)）。

例外：ChatGPT 订阅可以由 Loadout 单独登录（`loadout account login codex-sub <别名>`），并在两步风险确认（`loadout account transport codex-sub <别名> on`）后用于转发。这不是官方 API，有被限制或封号的风险，详见 [codex-subscription.md](docs/providers/codex-subscription.md) 与 [ADR-012](docs/adr/ADR-012-subscription-transport-opt-in.md)。Claude 订阅不提供这种用法；Claude 模型请使用 Console API key。

## 安全与隐私

- 网关只监听 `127.0.0.1`，每个请求都需要 client key；校验 Host 头（防 DNS rebinding），默认拒绝浏览器跨域请求（[ADR-011](docs/adr/ADR-011-local-gateway-security.md)）。
- 管理操作只能通过当前用户专属的命名管道（桌面端 / 命令行），没有 HTTP 管理接口。
- 请求日志只保存时间、客户端、模型、路由、token、耗时、费用、状态（[ADR-008](docs/adr/ADR-008-request-logging-privacy.md)），保留 400 天。

## 备份与恢复

- 网关每天自动导出一次配置到 `%LOCALAPPDATA%\Loadout\backups\auto`（保留 7 份）；也可以在 Settings → Backup 或 `loadout backup export` 手动导出。导出文件不含任何 API key。
- 换电脑：导出 → 新电脑 `loadout backup import <文件>` → 为提示的账号重新输入 API key。原有 client key 继续有效。
- 数据库损坏时，网关会把坏文件改名保留并以空库启动，界面与 `loadout daemon status` 会提示从自动备份导入。

## 文档

- [产品与技术设计](docs/product/product-technical-design.md) · [开发规范（Harness）](docs/development-harness.md) · [架构决策（ADR）](docs/adr/README.md) · [术语表](docs/architecture/glossary.md)
- [执行计划与进展](docs/exec-plans/completed) · [性能基线](docs/performance/baseline.md) · [系统测试清单](docs/testing/system-test-plan.md)
- [许可证与参考项目](docs/legal/reference-map.md)

## 许可证

[Apache License 2.0](LICENSE)。第三方许可证见发布包中的 `THIRD_PARTY_LICENSES.md`（由 `scripts/third_party_licenses.py` 生成并检查）与 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。贡献遵循 [DCO](https://developercertificate.org/)。
