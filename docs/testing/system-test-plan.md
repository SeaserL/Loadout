# Loadout v0.1.0 系统测试清单

本清单用于首次系统测试。自动化测试已覆盖协议转换、故障切换、限额、备份恢复等逻辑（mock 上游）；这里集中在**只有真实账号、真实客户端和真实桌面环境才能验证**的部分。

每项记录：通过 / 失败 / 备注（失败时附 `loadout logs --errors`、`%LOCALAPPDATA%\Loadout\logs` 最新日志中的相关行、截图）。日志不含提示词与密钥，可以直接贴出来。

---

## 0. 准备

- [ ] 用发布包安装：双击 `install.cmd`（建议加 `-AddToPath`：`powershell -ExecutionPolicy Bypass -File install.ps1 -AddToPath`）。
- [ ] 开始菜单出现 **Loadout**；打开后侧栏底部显示 "Gateway running" 与地址 `http://127.0.0.1:8317/v1`。
- [ ] 新终端中 `loadout daemon status` 显示运行中。

## 1. 账号与模型（每个你有的提供方各做一次）

| 提供方 | 添加方式 | 期望 |
|---|---|---|
| OpenCode Go | Add account → OpenCode Go | 模型列表同步；Accounts 页显示滚动 / 每周 / 每月额度 |
| Command Code | Add account → Command Code | 模型列表同步；用量显示为"由 Loadout 请求估算" |
| Kimi Code | Add account → Kimi Code（国内账号：先用命令行 `loadout provider add kimi-code --setting region=china`） | 模型同步；显示每周额度与 5 小时窗口 |
| OpenRouter | Add account → OpenRouter | 模型含价格；显示余额 / 今日与本月花费 |
| DeepSeek | Add account → DeepSeek | 显示余额 |
| OpenAI / Anthropic | Add account | 模型同步（OpenAI 只保留对话类模型） |
| Ollama（如有） | Add account → Ollama，key 留空 | 本地模型同步 |
| 其他兼容端点 | Add account → OpenAI-compatible，填 Base URL | 模型同步或手动添加 |

- [ ] 输错 key：添加时报错或同步失败并给出原因；**Change key** 改正后恢复。
- [ ] Models 页：模型名可点击复制；禁用一个模型后它从 `loadout/default` 中消失。
- [ ] Windows 凭据管理器（控制面板 → 凭据管理器 → Windows 凭据）中出现 `loadout:` 开头的条目；`%LOCALAPPDATA%\Loadout\data\loadout.db` 中搜索不到你的 key。

## 2. 订阅用量（只读）

- [ ] Accounts → **Scan this PC**：列出已安装 / 已登录的 Codex、Claude Code、Gemini CLI、Kimi Code。
- [ ] 分别 Import：Accounts 页出现对应账号（标记 "usage only"），额度窗口在几秒内出现（Codex 通过官方 `codex app-server`，可能需要 10–20 秒）。
- [ ] 官方 CLI 的登录过期时显示 "run `claude` once" 一类提示；运行一次官方 CLI 后点 **Refresh usage** 恢复。
- [ ] **Plan** 填写价格与续费日后，页面显示续费日期与 "value captured"（本月等价 API 花费 ÷ 订阅价格）。

## 3. 接入真实客户端

- [ ] Client keys → New client key，复制 key 与示例配置。
- [ ] OpenAI 兼容客户端（如 OpenCode 或自己的脚本）：模型用 `loadout/default` 与一个精确名 `provider/account/model`，流式与非流式都能正常输出。
- [ ] Anthropic 兼容客户端（如 Claude Code：`ANTHROPIC_BASE_URL=http://127.0.0.1:8317`、`ANTHROPIC_API_KEY=<client key>`、模型设为某个 Loadout 模型名）：能正常对话、调用工具。
- [ ] 使用 Responses API 的客户端（如 Codex CLI 指向 Loadout）：无状态请求正常；在只支持 Chat 的上游上发送带 `previous_response_id` 的请求时得到明确错误。
- [ ] Request log 中出现每个请求：客户端名、模型 → 路由、协议（converted / passthrough）、token、TTFT、费用。
- [ ] 响应头 `X-Loadout-Route` 显示实际路由（可用 `curl -i` 查看）。

## 4. 延迟分级与路由

- [ ] Routes → **Measure unmeasured routes**：每条路由出现 TTFT；测 3 次以上后出现 Tier（Ultra fast / Fast / Normal）。
- [ ] `loadout/realtime` 只包含超快路由，`loadout/fast` 包含超快 + 快；顺序与 Route health 页的 Tier / tok/s 一致。
- [ ] 真实使用一段时间后，Route health 的 SAMPLES 增加、TTFT 反映真实负载。**请记录你各路由的 P90 TTFT**——ADR-003 的阈值（800 / 2000 ms）是初始猜测，需要用这些数据校准（Settings → `latency.ultra_fast_ms` / `latency.fast_ms`）。
- [ ] Models 页把某个模型固定为 "Ultra fast (pinned)"，它进入 `loadout/realtime`；改回 "Measured" 后恢复。
- [ ] 新建 Pool（手动顺序），用 `loadout/<pool>` 请求时走第一条；改为 "Fastest first" 后顺序随测量变化。

## 5. 故障切换与额度

- [ ] 同一模型配两个账号（或两个提供方的同一模型加入同一 Pool）：给第一个账号设置错误的 key → 请求自动走第二个，Route health 中第一个显示 AUTH_FAILED；改回正确 key 后恢复。
- [ ] 某账号额度耗尽（或上游返回 429）时，请求切到下一条路由；Route health 显示 QUOTA_EXHAUSTED / RATE_LIMITED 及恢复时间。
- [ ] Settings → `routing.quota_reserve_percent` 设为例如 10：额度剩余低于 10% 的账号不再被选中。
- [ ] 请求头 `X-Loadout-No-Failover: true` 时不切换，直接返回错误。

## 6. Client 策略

- [ ] Client keys → Policy：Default model 设为 `loadout/default`，客户端发送 `gpt-4o` 也能得到回答。
- [ ] Allowed models 设为 `loadout/*`：精确名请求得到 403 `model_not_allowed`；`/v1/models` 只列出 `loadout/…`。
- [ ] Requests per minute 设为 2：第 3 个请求得到 429 `client_limit`。每日 token / 费用上限同理（UTC 零点重置）。

## 7. 桌面端与后台行为

- [ ] 关闭窗口 → 托盘图标仍在，客户端请求仍然成功。
- [ ] 托盘左键打开窗口；右键菜单 Stop gateway → 侧栏变为 "Gateway stopped" 并出现 **Start gateway** 按钮；Start 后恢复。
- [ ] 托盘 Quit → 桌面端退出，网关继续服务（`loadout daemon status`）。
- [ ] 再次双击开始菜单图标 → 只打开已有窗口，不会出现第二个托盘图标。
- [ ] Settings → Start at sign-in 勾选后注销 / 重新登录，网关自动启动。
- [ ] 深色模式下界面可读。

## 8. 统计与备份

- [ ] Statistics 页按模型 / 路由 / 提供方 / 客户端 / 日期切换，数字与 Request log 一致。
- [ ] Settings → Backup → Export configuration：生成的 JSON 中搜索不到任何 API key。
- [ ] `%LOCALAPPDATA%\Loadout\backups\auto` 中有自动备份（网关启动时生成，之后每天一次）。
- [ ] （可选）在另一台电脑或另一个 Windows 用户下 Import，重新输入 API key 后一切可用，原 client key 仍有效。

## 9. 升级、卸载与恢复

- [ ] 再次运行 `install.cmd`（模拟升级）：网关被优雅停止后重新启动，配置与日志都在。
- [ ] （可选，恢复演练）停止网关，把 `%LOCALAPPDATA%\Loadout\data\loadout.db` 替换为任意非数据库文件，再启动：网关正常启动，界面顶部提示数据库已损坏及恢复步骤；按提示从 `backups\auto` 导入后配置恢复。
- [ ] `uninstall.ps1`：程序、开始菜单图标、自启动被移除，数据保留；加 `-RemoveData` 后数据目录与 `loadout:` 凭据都被删除。

---

## 已知限制（v0.1.0）

- 只支持 Windows 10/11 x64；安装为 PowerShell 脚本（无签名安装器、无自动更新）。
- 订阅登录只读取用量，不转发请求（ADR-009）；Command Code 的额度无法通过 API key 读取，显示为估算值。
- 延迟阈值为初始值，需要你的真实数据校准；`balanced` / `quality` / `economy` 等产品文档中的打分式路由在 v1 中未实现（ADR-003）。
- 每日限额与"今天"的统计按 UTC 日计算（北京时间 08:00 重置）。
