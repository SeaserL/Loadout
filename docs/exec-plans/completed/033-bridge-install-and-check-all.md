# ExecPlan 033: OpenCode Bridge Fresh Install, Opt-in Card Wording, Provider Check-All

## Goal
1. 在另一台机器上安装 v0.4.0 后，OpenCode 免费桥接无法部署：Release 只提供"更新已有桥接"的工具包，缺少 `spawn_detached.exe` 与全新安装脚本，用户只能装 Rust 自行编译。让桥接工具包本身就能完成全新安装。
2. Codex 订阅转发 / OpenCode 桥接账号在未开启转发时，卡片仍显示"x/y 个模型参与路由"，且不提示它们不进 `loadout/realtime|fast|default`（ADR-012/017 的设计），用户误以为是 bug。修正文案。
3. 账号与用量页面：多账号 Provider 卡片（如商汤）增加"一键检查全部"，代替逐个点击诊断/测速。

## Context
- ADR-012 §1：订阅路由默认不进入内置虚拟模型；ADR-017：OpenCode 官方 2.0.32 常驻服务 + 本地桥接，部署须显式执行脚本，Loadout 包不内置官方 binary。
- 本机正式桥接为手工部署（`%LOCALAPPDATA%\LoadoutOpenCodeBridge`），`Update-OpenCodeBridge.ps1` 要求已有 `binding.json`。
- `account.check`（ExecPlan 014）带 `models=true` 时已做"凭证检查 + 全部已启用模型 1-token 探测 + 延迟评估"，覆盖单卡的诊断与测速按钮。

## Existing behavior
- `scripts/package.ps1` 生成 `*-opencode-bridge.zip`，只含 Update/Start 脚本与三个 Python 模块、controls 插件。
- `AccountCard` 对 opt-in 账号也显示 `acc.modelsRouted`。
- Provider 卡片头只有专用端点、用量来源、删除。

## Constraints
- 不改变 ADR-012/017 的路由决策：不把 Tier C 加入内置虚拟模型，不自动开启转发。
- 安装脚本只从 npm 官方 registry 下载固定 `@opencode/cli-windows-x64@2.0.32`，校验 tarball sha512 integrity 与 `opencode.exe` SHA-256；也允许用户提供本地 exe（同样校验 SHA-256）。
- Secret 只经 DPAPI 文件与 stdin 进入 Credential Manager，不落日志/命令行。
- 不新增系统自启；安装失败不覆盖已有桥接（已有 `binding.json` 时拒绝并提示用 Update 脚本）。
- 不改 Router/Gateway/schema/IPC。

## Architecture impact
仅打包脚本、外部桥接脚本与桌面前端。检查全部在前端以有限并发调用现有 `account.check`。

## Files expected to change
- `scripts/Install-OpenCodeBridge.ps1`（新）
- `scripts/opencode_bridge_setup.py`（`account` 动作：已有账号时 set-key，并同步模型）
- `scripts/package.ps1`（编译并打包 `spawn_detached.exe`、setup 脚本）
- `packaging/OPENCODE-BRIDGE.txt`
- `docs/providers/opencode-free.md`
- `apps/desktop/src/pages/Accounts.tsx`、`apps/desktop/src/i18n.tsx`、`apps/desktop/src/styles.css`

## Implementation stages
1. 安装脚本 + setup 脚本 + 打包。
2. 卡片文案。
3. Provider "检查全部"。
4. 前端构建、PowerShell 语法检查、脚本隔离测试（临时目录、本地 exe、不启动）。

## Tests
- `npm run build`（tsc + vite）。
- `python -m unittest discover -s scripts -p test_opencode_bridge.py` 回归。
- Install 脚本：PowerShell 解析检查；`-NoStart` 指向临时目录 + 本机已安装的官方 exe，验证文件布局、binding、DPAPI 初始化，不触碰正式目录/daemon。

## Performance validation
不涉及 Router/Gateway，不需要 benchmark。

## Security considerations
下载固定版本并双重校验哈希；临时下载目录独立、用后删除；bridge key 只经 stdin 传给 `loadout account add/set-key`。

## Migration considerations
无数据库变更。

## Known risks
- 官方上游 403 风控与网络有关（opencode-free.md），安装成功不代表可用。
- 用户机器无 Python 3 时无法安装；脚本明确报错。

## Definition of done
新机器解压桥接工具包后运行一个脚本即可完成安装（仍需在面板开启转发）；opt-in 卡片文案准确；多账号卡片可一键检查。

## Progress
- [x] 2026-10-20 调研：确认缺 `spawn_detached.exe`/全新安装路径；`account.check` 可复用。
- [x] Stage 1：`Install-OpenCodeBridge.ps1`（含 `-Resume`/`-NoStart`）、setup `account` 支持已有 provider/账号、`package.ps1` 编译并打包 `spawn_detached.exe` 与 setup/DPAPI helper、README 与 provider 文档。
- [x] Stage 2：opt-in 未开启时显示"已选 x/y 个模型，开启转发后才参与路由"；已开启时加"不进 loadout/default"标记及说明。
- [x] Stage 3：多账号（≥2 个可检查卡片）Provider 头部"全部检查"，并发 3，显示进度，汇总 toast（账号/模型/异常列表）。
- [x] Stage 4（2026-10-20）：`npm run build` 通过；`test_opencode_bridge.py` 24 tests OK；安装脚本隔离测试（临时 kit + 临时目录 + 本机官方 exe）：`-NoStart` 布局与正式部署一致、binding/DPAPI 正常；重复安装、伪造 exe、缺 `spawn_detached.exe`、无可 Resume、错误 Python 均拒绝；改用空闲端口 28789/28790 启动临时桥接，health `ok`/2.0.32、`/v1/models` 正常，随后只停止临时 PID。setup `account` 用假 CLI 验证"新机器"（provider add + account add）与"已有账号"（set-key + model sync）两条路径，key 只经 stdin。真实 CLI JSON 字段只读核对。UI 在 Vite + 注入的模拟 IPC 中验证按钮、进度、汇总与文案。未真实下载 npm 包（metadata/integrity/SHA 已核对）。

## Decisions
- "检查全部"用 `account.check(models=enabled)`，不调用 `model.sync`（同步会改变模型配置，不属于状态刷新）。
