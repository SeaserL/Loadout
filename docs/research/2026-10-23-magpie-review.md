# Magpie 调研：Windows 桌面工程对 Loadout 的启发（2026-10-23）

> 讨论稿，不是决定。落地前按 AGENTS.md §6 建 ExecPlan；凡是改变数据目录、更新流程或系统状态的条目，先写或修订 ADR。
> 研究对象：[Blinue/Magpie](https://github.com/Blinue/Magpie)，默认分支 `dev`，固定 commit `9e8e90274ba07f555bf87f09321dabdac97b1b4f`（2026-10-23，v0.13.0 之后）。浅 clone 在本次会话的 scratchpad，未纳入仓库。
> **许可证**：GPL-3.0（根 `LICENSE`，源文件带 GPL 头）。按 reference-map §5.4，**只能参考概念，不得复制或翻译实现**。`AutoStartHelper.cpp` 注明其任务计划程序用法参考自 PowerToys（MIT），需要时应直接读 PowerToys 原文件。

## 0. 结论速览

Magpie 和 Loadout 的业务完全不同（窗口超分辨率 vs LLM 网关）。可借鉴的是"常驻 Windows 桌面工具"的工程做法。对照下来，Loadout 大多数地方已经做得比 Magpie 更严（更新器、单实例、托盘、安装）。**最有价值的是一个由对照引出的缺口**：升级时数据库只能向前迁移，没有备份，更新失败回滚后旧程序可能起不来（§2.1，推断的故障路径，未实测）。

| 编号 | 方向 | 价值 | 工作量 | 需要 ADR？ | 建议 |
|---|---|---|---|---|---|
| 1 | 迁移前自动备份数据库；更新回滚时一起恢复；`SchemaTooNew` 在界面上说清楚 | 高 | S–M | **是**（修订 ADR-018 的回滚流程） | **P1** |
| 2 | 诊断：panic hook、启动日志补系统信息、app 端日志、单文件大小上限、一键导出诊断包 | 高 | S–M | 否（诊断包遵守 ADR-008 脱敏） | **P1** |
| 3 | 更新器收尾：清理事务目录、只在原来运行时才拉起 app、先请求 app 退出再强杀、删除已移出清单的旧文件 | 中 | S | 否 | **P2** |
| 4 | 自启状态识别失效路径；安装目录变化时刷新 Run 值 | 中 | S | 否 | **P2** |
| 5 | 面向用户的 SmartScreen / 杀毒软件说明，以及如何核对 SHA-256 | 中 | S | 否 | **P2** |
| 6 | 预览更新通道（仅自己的机器开启，用来先验证更新路径） | 低中 | S | **是**（ADR-018 目前明确拒绝 prerelease） | P3 |
| 7 | 给第三方工具的稳定契约（`--json` 输出字段，而不是 IPC） | 低中 | S | 否 | P3（等 statusline / band 需要时再做） |
| — | 自签名根证书 + 写入 System32、任务计划程序自启、MD5 + raw 分支的版本文件、便携模式、Weblate 多语言 | — | — | — | **不做**（§4） |

---

## 1. Magpie 是什么

Windows 10/11 上的窗口超分辨率工具：把任意窗口实时放大到全屏或更大的窗口，内置 Anime4K、FSR 等 HLSL 效果。C++/WinRT + WinUI 3（XAML Islands），约 1.6 万 star，2021 年起持续维护，支持 x64 与 ARM64。

**和 Loadout 的业务没有交集**（不涉及 LLM、网络代理），但它是一个成熟的、只面向 Windows 的常驻桌面工具，和 Loadout 面对同一批平台问题：托盘常驻、开机自启、单实例、自更新、配置存储与升级迁移、日志、多语言、系统级改动的授权。本文只看这些工程部分。

| 组成 | 位置 | 作用 |
|---|---|---|
| 主程序 | `src/Magpie/` | WinUI 界面、设置、托盘（`NotifyIconService`）、自启（`AutoStartHelper`）、更新检查（`UpdateService`） |
| 缩放运行时 | `src/Magpie.Core/`、`src/Effects/` | 捕获、渲染、效果编译 |
| 独立更新器 | `src/Updater/` | 等主程序退出后替换文件，再启动新版 |
| 触控助手 | `src/TouchHelper/` | 需要 UIAccess 的辅助进程（见 §4） |
| 发布 | `.github/workflows/release.yml`、`scripts/publish.py`、`scripts/release.py`、根 `version.json` | 手动触发：输入版本号 → 构建、签名、打 tag、发 release、回写 `version.json` |

### 1.1 值得注意的做法（摘要）

- **更新**：程序读 `main`（正式版）或 `dev`（预览版）分支上的 `version.json`，比较版本号，下载 zip 并校验 MD5，解压到 `update\`。然后把 `Updater.exe` 移到根目录、启动它、自己退出。`Updater.exe` 用单实例 mutex 等主程序退出，再反复尝试删除 `Magpie.exe`（最多约 10 秒），确认文件锁已经释放。接着**按旧版本的文件清单**（`src/Updater/PackageFiles.h`，每个历史版本一份）删除旧文件，只删程序文件，不删用户数据。最后移入新文件、启动新版。没有回滚。
- **自启**：优先用任务计划程序的登录触发器（可以无 UAC 地以管理员身份启动），并延迟 3 秒，避免资源管理器还没起来；失败时退回到"启动"文件夹快捷方式。
- **托盘**：监听 `TaskbarCreated`，资源管理器重启后重新添加图标。另外借用 PowerToys 的做法：启动时任务栏还不存在、`Shell_NotifyIcon` 失败的话，监听 `WM_WINDOWPOSCHANGING`，等资源管理器起来后再补上图标。
- **配置**：`%LOCALAPPDATA%\Magpie\config\v<N>\config.json`，目录名带配置格式版本。升级时找不到当前版本的配置，就从高到低找旧版本目录，读出来迁移后**写到新目录，旧文件原样保留**，所以退回旧版本时它仍能读到自己格式的配置。配置解析失败时弹窗显示配置文件路径并退出，**不覆盖损坏的文件**。程序目录下存在 `config\config.json` 时进入便携模式。
- **单实例**：GUID 命名的 mutex；第二个实例广播已注册的窗口消息，让第一个实例显示主窗口。以管理员身份重启时，用第二个 mutex 协调新旧实例的交接。
- **日志**：spdlog 滚动日志，单文件 500 KB、保留 2 个；第一行记录版本（或 dev commit）、系统版本、是否管理员。bug 报告模板（`.github/ISSUE_TEMPLATE/01_bug.yaml`）**必填**版本、系统版本和日志附件。
- **对外契约**：`docs/以编程方式与 Magpie 交互.md` 公开一组承诺不变的接口：广播消息 `MagpieScalingChanged`、固定的窗口类名、窗口属性。另外提供示例程序 MagpieWatcher。
- **多语言**：20 种语言的 `.resw` 资源，在 Weblate 上协作翻译。

---

## 2. 值得做的（P1 / P2）

### 2.1 迁移前备份数据库，回滚时一起恢复（P1，需修订 ADR-018）

**Magpie**：配置目录带格式版本（`config\v5\`）。升级只读旧目录、写新目录，旧配置原样保留，所以退回旧版本时不会因为格式更新而起不来。

**Loadout 现状**：
- 每次 `Database::open` 都直接 `to_latest` 迁移，迁移前不备份（`crates/storage/src/database.rs:35-46`、`:169-179`）。down 脚本只在测试里用，ExecPlan 007 定为"只向前"。
- 旧程序打开新 schema 时返回 `SchemaTooNew`（`database.rs:171-176`），daemon 以退出码 1 退出。界面只显示通用的"loadoutd exited with code 1; see the log"（`apps/desktop/src-tauri/src/bridge.rs:230-238`）。
- `packaging/update.ps1` 回滚时只恢复程序文件（`:92-110`），不恢复数据库。
- 每日自动备份只包含配置 JSON（`crates/core/src/backup.rs`），不含请求历史。

**推断的故障路径**（未实测）：
1. 新 daemon 启动，完成迁移；
2. 但没能在 15 秒内报告就绪（例如 `request_events` 保留 400 天、表很大，迁移耗时较长），或者就绪前崩溃；
3. `update.ps1` 判定失败，恢复旧程序；
4. 旧 daemon 遇到 `SchemaTooNew` 退出。回滚脚本里的 `daemon start` 输出被丢弃（`*> $null`），用户最后没有网关可用，界面只看到"exit code 1"。

用户手动安装旧版 zip 时也会遇到同样的情况。

**建议**：
- **迁移前备份**：`Database::open` 发现 `user_version` 小于 `LATEST_VERSION`、且不是新建的库时，先 `VACUUM INTO <data>\backups\pre-migrate\loadout-v<旧版本>-<时间>.db`，保留最近 2–3 份。只在 schema 变化时执行，平时不增加启动时间。
- **回滚时恢复**：`update.ps1` 回滚时，如果数据库版本已经高于旧程序支持的版本，恢复对应的 pre-migrate 备份（连同 `-wal` / `-shm`）后再启动旧 daemon。从迁移到回滚之间几秒钟的新数据会丢失，可以接受。旧程序支持的 schema 版本由旧 daemon 写进更新 manifest。
- **退出码**：给 `SchemaTooNew` 分配专用退出码（现有 2 / 3 / 4 之外），`explain_exit` 显示"数据库来自更新的版本 v{found}，当前程序支持 v{supported}；可升级，或从 `backups\pre-migrate` 恢复"。参照 Magpie 配置解析失败时会显示文件路径的做法。
- **迁移测试**：用一个"大表"测试测出迁移耗时，必要时把就绪等待时间与迁移挂钩，而不是固定 15 秒。
- 工程约束：数据库相关改动走 migration 测试（Harness §21）；ADR-018 的回滚一节需要写明数据库也在回滚范围内。

### 2.2 崩溃与诊断（P1）

**Magpie**：
- 日志首行记录版本（或 dev commit）、系统版本、是否管理员；
- spdlog 单文件 500 KB、保留 2 个，warn 级别立即落盘；
- bug 模板**必填**版本、系统版本和日志附件。

**Loadout 现状**：
- **没有 panic hook**。daemon 没有窗口，默认的 panic 输出写到 stderr，直接丢失。`supervisor` 只记录"critical service panicked"，不记录 panic 内容（`crates/core/src/supervisor.rs:36`）。
- **app 没有日志文件**：release 构建是 `windows_subsystem`，`eprintln!` 的输出全部丢失。
- daemon 日志按天滚动、保留 14 个，**单文件没有大小上限**（`crates/core/src/logging.rs`）。如果出现日志风暴（例如反复 failover），单个文件会一直变大。
- 首行只有 version 和 data_dir（`daemon.rs:63`），没有系统版本、exe 路径和构建 commit。
- 没有诊断导出。

**建议**：
- `loadoutd` 和 `loadout-app` 都安装 `std::panic::set_hook`，把 panic 消息和位置写进 tracing 日志（经过现有的脱敏层）；supervisor 把 `JoinError` 里的 panic payload 也记下来。
- 启动日志补充：系统 build（`RtlGetVersion`）、exe 路径、构建 commit（构建时注入）、安装方式（正式安装目录还是开发构建）。
- app 写自己的滚动日志 `logs\loadout-app.*.log`，复用 daemon 的日志设置。
- 按天滚动之外再加单文件大小上限。
- `loadout diagnostics export` 加设置页按钮，导出一个 zip，包含：
  - 最近几份日志；
  - `loadout --json daemon status`、版本和系统信息；
  - 已脱敏的设置。
  - **不含**数据库、请求正文和凭证（ADR-004、ADR-008）。
  - 导出前列出文件清单，由用户确认后再分享。

### 2.3 更新器收尾（P2）

**Magpie**：
- `Updater.exe` 先等单实例 mutex 释放，再反复尝试删除 `Magpie.exe` 直到成功，确认文件锁已经释放；
- 按**每个历史版本的文件清单**（`src/Updater/PackageFiles.h`）删除旧文件；
- 最后删除 `update\` 临时目录。

**Loadout 现状**：`update.ps1` 已经有白名单、事务备份和回滚，比 Magpie 严格得多。但还有几处没收好：
- 每次更新的 `transaction-<guid>` 目录（内含 3 个 exe 的备份）**从不清理**（`update.ps1:30`），磁盘占用会随更新次数增长。
- 更新成功后**总会**隐藏启动 `loadout-app.exe`（`:89`），即使更新前 app 并没有运行（回滚分支则会检查 `$wasApp`）。需要确认这是不是有意为之。
- app 直接 `Stop-Process -Force`（`:72`），没有先请求它退出。`install.ps1` 会强杀**所有** loadout-app，不按路径过滤（`install.ps1:36-45`），开发构建或测试实例也会一起被杀掉。
- 只覆盖白名单内的文件。将来如果某个文件从白名单里移除，旧安装目录里的那份会一直留着。

**建议**：
- 事务目录只保留最近一份；
- 成功分支也按 `$wasApp` 决定是否启动 app；
- 先经单实例窗口或 IPC 请求 app 退出，超时后再强杀；
- `install.ps1` 按路径过滤进程；
- 在 manifest 里带上"上一版的文件清单"，删除已经不在新清单里的文件。

### 2.4 自启状态识别失效路径（P2）

**Magpie**：任务计划程序的登录触发器延迟 3 秒，避免资源管理器还没准备好；托盘另做了两处补救（§1.1）。

**Loadout 现状**：
- HKCU Run 值 `Loadout` → `"<dir>\loadoutd.exe" --background`，只启动 daemon（`crates/platform/src/autostart.rs`）。Run 值由资源管理器执行，不存在 Magpie 那种时序问题。
- 托盘用的 tray-icon 0.25.1 已经处理 `TaskbarCreated`，启动时 `NIM_ADD` 失败也会等它广播后再补上图标（`tray-icon-0.25.1/src/platform_impl/windows/mod.rs:53`、`:159-162`、`:371-381`）。**Magpie 的两处补救都不需要再做。**
- 缺口：app 的 `autostart_get` 只要有注册就返回 true（`apps/desktop/src-tauri/src/lib.rs:39-42`）。exe 路径失效时，开关仍显示为开启，CLI 却会提示需要重新启用（`autostart.rs:139-146`）。`install.ps1` 只在目标目录已有 `loadout.exe` 时才刷新注册（`:49`），换目录安装后 Run 值仍指向旧路径。

**建议**：
- app 读取 `executable_exists`，路径失效时显示"需要重新启用"；
- `install.ps1` 发现 Run 值指向一个已经不存在的 `loadoutd.exe` 时，改写到新路径；
- 是否增加"开机时显示托盘"选项，等 Windows 通知（2026-10-04 调研 #3）需要 app 常驻时再决定。

### 2.5 SmartScreen / 杀毒软件说明（P2，文档）

**Magpie**：只给需要 UIAccess 的 `TouchHelper.exe` 签名，主程序同样没有签名。

**Loadout 现状**：所有 exe 都没有签名。`install.ps1` 会 `Unblock-File`。ExecPlan 001 已把"Run 键里的未签名 exe 可能被杀毒软件拦截"列为风险。面向用户的 README 里没有任何说明。

**建议**：在 `README.txt` 加一节，说明：
- SmartScreen 拦截时的处理办法；
- 被杀毒软件误报时，怎样用发布页的 `.sha256` 核对 zip；
- 只从本仓库 Release 下载。

正式的 Authenticode 签名成本较高，仍按原计划推后。**不要**用自签名证书（见 §4）。

---

## 3. 我们已经做对、无需再学的

- **更新器**：
  - GitHub API、SHA-256、重定向主机白名单、大小上限、zip 条目白名单（Magpie 只用 MD5，检查的是 raw 分支上的 `version.json`）；
  - 事务备份 + 回滚、装完后校验 `loadout --version`（Magpie 没有回滚）。
- **单实例**：
  - daemon 用 `Local\Loadout-<SID>` mutex，再加一个 `FILE_FLAG_FIRST_PIPE_INSTANCE` 管道，DACL 只允许本用户和 SYSTEM，比 Magpie 的 GUID mutex 更严；
  - app 用 tauri single-instance 插件，第二次启动会显示已有的主窗口，与 Magpie 广播 SHOWME 消息效果相同。
- **托盘**：资源管理器重启、启动时任务栏尚未就绪，这两种情况 tray-icon 都已处理（§2.4）。
- **损坏数据**：SQLite 损坏时把文件移到一旁、新建空库，界面显示横幅，不覆盖损坏的文件。Magpie 对损坏配置的处理同样是不覆盖。
- **安装**：按用户安装，不需要管理员；卸载默认保留数据，`-RemoveData` 才删除数据和 Credential Manager 条目。
- **多语言**：`zh: Record<MessageKey, string>` 让 `tsc` 在缺少 key 时直接编译失败。只有 2 种语言，不需要 Weblate。小缺口：只有中文的 `settingDescriptionsZh` 没有 key 检查。

## 4. 不建议照搬

- **自签名证书写入本机受信任根 + 程序复制到 System32**（Magpie 触控支持）：这是对系统信任链的重大改动，而且需要管理员权限。Loadout 没有 UIAccess 需求，坚持按用户、不提权安装。可以借鉴的只是它的做法：默认关闭、文档写明会改动什么、关闭时完整撤销、失效时提示修复。这与 ADR-014 的 `web_session.enabled` 一致。
- **任务计划程序自启**：只有需要以管理员身份自启时才有必要。Loadout 以普通用户运行，用 Run 值是正确的。
- **MD5 + raw 分支上的 `version.json`**：完整性和来源校验都比现在的方案弱。
- **便携模式**：Loadout 依赖 Credential Manager 和按用户命名的管道，便携模式收益小、复杂度高；`--data-dir` 已经够测试用。
- **Weblate / 20 种语言**：当前规模不需要。

## 5. 参考文件清单（commit `9e8e902`，GPL-3.0，只读概念）

| 主题 | 路径 |
|---|---|
| 更新检查与下载 | `src/Magpie/UpdateService.cpp`、根 `version.json` |
| 独立更新器、旧版本文件清单 | `src/Updater/main.cpp`、`src/Updater/PackageFiles.h` |
| 自启（任务计划程序 → 启动文件夹） | `src/Magpie/AutoStartHelper.cpp`（概念源自 PowerToys `src/runner/auto_start_helper.cpp`，MIT） |
| 托盘与资源管理器重启 | `src/Magpie/NotifyIconService.cpp`（`WM_WINDOWPOSCHANGING` 补救源自 PowerToys PR #789） |
| 单实例、管理员重启交接 | `src/Magpie/App.cpp`、`src/Shared/CommonSharedConstants.h` |
| 版本化配置目录与迁移 | `src/Magpie/AppSettings.cpp`（`_UpdateConfigPath`、`FindOldConfig`） |
| 日志 | `src/Shared/Logger.cpp`、`src/Magpie/main.cpp` |
| 对外契约 | `docs/以编程方式与 Magpie 交互.md` |
| 发布流水线 | `.github/workflows/release.yml`、`scripts/publish.py`、`scripts/release.py` |
| bug 模板 | `.github/ISSUE_TEMPLATE/01_bug.yaml` |
| 触控支持（反面参考） | `src/Magpie/TouchHelper.cpp`、`docs/关于触控支持.md` |
