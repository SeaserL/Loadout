# 069 — 1.0 内置更新器

## Goal
用户确认 1.0 没有更新器后要求做一个：在软件里检查、下载并安装新版本，失败能自动恢复。

同一轮里用户还要求了几处小改动，版本号保持 1.0.0：去掉侧边栏的主题按钮，去掉流向图 logo 外的圆圈，放慢页内标签切换的淡入，把动效开关改成 0 到 2 倍的全局动画速率。

## Context
见 [ADR-020](../../adr/ADR-020-updater.md)。1.0 是一个文件夹式便携包，运行中的程序都在要被替换的文件夹里，后端带出站拦截。

## Existing behavior
设置页只显示版本串。`scripts/fusion/` 的 `promote` 流程能在本机换包，靠的是同一上游版本的实例可以被新包直接接管（`sameBase`）。

## Constraints
不放开 `isolation-guard`。窗口关着时不产生出站。下载和安装要用户点击。不执行下载包里的脚本。不碰 0.4 和 8317。默认测试不访问真实 GitHub。测试不能启动真实窗口：`Loadout.exe` 是单实例的，会抢用户开着的窗口。

## Architecture impact
- `fusion/updater.mjs`（新）：检查、下载、校验、解压，独立短进程，只依赖 Node 内置模块，CI 可以直接测。
- `fusion/apply-update.ps1`（新）：换文件夹、启动、回滚。
- `backend/.../control/updates.ts`（新）：RPC `update.status`、`update.check`、`update.download`、`update.install`、`update.set_auto_check`、`update.open_notes`，只负责启动进程和读状态文件。
- `fusion/runtime.mjs`：`validateConfig` 接受只限本机的 `updateFeed`，测试用它指向模拟 GitHub。
- 控制台：设置页“软件更新”卡片，左侧导航的“设置”上有更新提示点。

## Files expected to change
上面这些，加 `apps/fusion-console/src/` 的多个文件、`scripts/fusion/update-e2e.mjs`（新）、`scripts/fusion_updater.test.mjs`（新）、`.github/workflows/ci.yml`、`tests/fusion/control-test.mjs`、`README.md`、发布文档。

## Implementation stages
1. `updater.mjs` 和它的测试（模拟 GitHub，各种恶意输入）。
2. `apply-update.ps1` 和测试模式（换文件夹、失败回滚）。
3. 控制面 RPC 和设置页。
4. 打包，全量回归。
5. 端到端：真实包从旧版本更新到新版本，包括新版本起不来时的回滚。

## Tests
`scripts/fusion_updater.test.mjs`（`node --test`，进 CI）：版本比较、资产校验、重定向白名单、哈希和大小不符、压缩包路径穿越、文件多了少了改了、上游版本变化，以及换文件夹和回滚。`tests/fusion/control-test.mjs`：`loopback-only` 下更新状态是 `disabled`，不产生任何进程。`scripts/fusion/update-e2e.mjs`：手动运行，真实包端到端。

## Performance validation
没有改请求路径。

## Security considerations
见 ADR-020。更新进程的环境变量是白名单，不带 `API_KEY_SECRET`、`JWT_SECRET`、`INITIAL_PASSWORD`。RPC 在管理会话之后。

## Migration considerations
无数据迁移。同一上游版本的实例直接被新包接管，换上游版本拒绝自动更新。

## Known risks
- 更新器只认比当前高的版本号。版本号保持 1.0.0 时它会一直显示已是最新，第一次完整的真实更新要等下一个更高的版本。检查、下载、解压、校验已经对真实的 v1.0.0 跑过（假装当前是 0.9.0），换文件夹只在模拟 GitHub 上跑过。
- 系统代理用本机的 CONNECT 代理验证过（请求确实经过代理到达 `github.com` 和资产 CDN），没有在真实的第三方代理软件上跑过，本机没有开系统代理。
- 更新后重新打开窗口没有自动验证：`Loadout.exe` 是单实例的，测试不能启动它。

## Definition of done
设置页能检查、下载、安装；端到端更新成功，失败能恢复；回归通过；文档更新。

## Progress
- [x] `updater.mjs` 和测试。
- [x] `apply-update.ps1` 和测试。查到：PowerShell 在 `DETACHED_PROCESS` 下立刻退出且返回 0；Node 把非分离的子进程放进作业对象，退出时一起结束，所以脚本要用 `Start-Process` 重启自己才能活过 `updater`（这条是端到端测试抓到的，单元测试里父进程一直活着）；`Start-Process -Wait` 会连后台进程一起等；5.1 按 ANSI 读无 BOM 的文件，所以脚本里不写中文。
- [x] 控制面和设置页。
- [x] 真实 GitHub 上查到：匿名 API 配额被同一出口的其他程序用光，直接 403。改走 `releases/latest` 的跳转，不再用 API。
- [x] 小改动：主题按钮、logo 圆圈、标签淡入、动画速率。
- [x] 测试挂住一小时：PowerShell 的工作目录在 `loadout-update`，Windows 锁住了它，测试清理失败（EBUSY）；`node:test` 跳过之后的清理钩子，模拟服务器没关，进程永远不退出。脚本的工作目录改到系统目录，服务器 `unref`，CI 加 `--test-timeout`。
- [x] 复查修掉：失败的检查不记时间，断网时每次读状态都起一个进程（设置页每 2 秒一次）；连不上 GitHub 时已校验的下载装不了；旧的待安装版本留在磁盘上；脚本没能启动时界面要等 10 分钟才报错；旧文件夹删除失败后残留；“更新说明”链接被只开登录页的 `system.open_url` 拒绝。
- [x] 打包、回归、端到端（包 `5fc36202b92f`）。
- [x] 文档。
- [x] 用户验收通过（2026-11-03），新包已换到本机实例。

## Verification
均在包 `fusion-v3.8.52-isolated-067.1-5fc36202b92f` 上，2026-11-03：

- 单元测试：`scripts/fusion_updater.test.mjs` 27/27，连跑三遍，每遍 3 到 5 秒。
- 真实包端到端（`update-e2e.mjs`，模拟 GitHub，临时实例）：1.0.0 到 1.0.1 用时 8 秒，地址和数据不变，旧文件夹已清理，下载期间网关一直可用。新版本起不来时回滚，版本、地址、数据都恢复，并告知用户。
- 全量回归 275/275，审计 370 个实例无残留、无来源不符。`tsc --noEmit` 通过。
- 真实 GitHub（包外临时目录，假装当前是 0.9.0）：检查到 1.0.0、下载 381 MB、解压、逐文件校验共 75 秒，访问的主机只有 `github.com` 和 `release-assets.githubusercontent.com`；按 1.0.0 再查显示已是最新。这一项用的是修复前的代码，之后改动的只是失败处理和工作目录，下载路径没有变。
- 代理：本机 CONNECT 代理，按注册表格式解析后设置，真实请求经过代理到达上面两个主机。
- 换到本机实例后（8318，WorkBuddy 真实请求）：`ping-live` 10/10；同一地址的 OpenAI 和 Anthropic 客户端都返回 200，模型列表相同（84 个）；长流式回复 382 块，实时视图看到字节持续增长，结束后没有残留。窗口打开后后端自己启动了一次更新检查，结果是已是最新（1.0.0），`egress.log` 只多了 `github.com`。安装的 0.4（8317）没有动。

没有验证的见 Known risks。动画速率、logo、标签淡入在无头 Edge 里验证过，用户还没有在真实窗口里看过。

## Decisions
- 版本号保持 1.0.0：用户明确要求。
- 动画速率靠 `playbackRate`：CSS 动画和过渡在开始时被告知速度，这样不用改写几百条规则。错开的延迟在开始之前，所以用 `--motion-rate` 换算。画布和数字滚动直接乘速率。
- 主题按钮去掉后，配色、主题和动画速率都在设置页。
- logo 的发亮改成自身形状的投影，不再是圆形光晕。
- 检查由控制台读状态时触发，不加定时器：窗口关着就不需要提醒任何人。
