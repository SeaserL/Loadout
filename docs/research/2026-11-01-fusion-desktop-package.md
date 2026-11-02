# 066 桌面候选包验收

## 范围

收口 065 已有界面与 Tauri 窗口/托盘，不新增业务功能。OmniRoute 固定 v3.8.52；完整核对 064 封印、上游 patch 和 source-additions 后复用已编译后台。静态界面与 API 由同一后台服务，不依赖 Vite。启动、停止、凭据与账号规则复用既有工具。

当前候选信息以 current.json 为准。此轮是 064.1→066.1 包升级，不是新 OmniRoute 上游版本升级；不替换正式桌面端，不自动迁移原有 059 隔离实例。默认别名、订阅转发、GPT 池仍不启用。

## 交付结果

- 指定候选：`D:\Loadout\dist\fusion\fusion-v3.8.52-isolated-066.1-cec9977a9720.zip`，400409411 bytes（381.86 MiB）。
- SHA256：`8a2e19de10e97c65ea1b76e77b3b1460974fabb0283c107fb202b0448938fac3`。
- 可直接运行的同一候选展开目录：`D:\Loadout\development\fusion\runs\066-desktop\packages\desktop`。
- 完整证据：`development/fusion/evidence/066-desktop/evidence/`；当前指针 `development/fusion/current.json`。
- `desktopAcceptance=blocked`、`cleanupPending=true`、`activationReady=false`。打包与自动回归完成，正常托盘退出、视觉及清理未完成。
- 物理 ZIP 共 3 个：1 个当前候选 + 2 个明确作废/待删旧包。没有把清理阻塞隐藏成“已只留一包”。
- 没有提交或上传，没有切换正式安装；首次误唤起正式窗口的事件及修复见下文。

## 交互约定

入口为包内 `Start-Loadout.cmd`，启动后台后打开真正的 Tauri/WebView2 桌面窗口。`LoadoutFusion.exe` 是由入口传参的内部程序。没有安装器/开始菜单/自启操作。所有管理 API 仍需原生登录。

| 操作 | 预期 |
|---|---|
| 首次启动 | 创建或复用明确的隔离实例，空闲 loopback 端口，窗口显示登录页 |
| 再次启动同一实例 | 同一后台，单一桌面窗口恢复 |
| 最小化后恢复 | 内容与登录会话保持 |
| × / Alt+F4 | 隐藏到托盘，后台和桌面进程仍在 |
| 托盘左键 / 打开 Loadout | 同一窗口恢复 |
| 托盘退出窗口 | 桌面进程退出，后台继续运行 |
| Stop-Backend 后重开 | 后台停止并按原实例/端口恢复，保留配置 |

## 验收记录

构建、Rust 两项单测（含实际编入配置的隔离检查）/clippy/fmt、前端类型与打包、许可证策略检查通过。首次 253 项集中回归通过，但原生启动暴露单实例冲突，因此该首包判为失败；修复后已重新封印并重跑全套：最终 066.1-cec9977a9720 的 17 组、253/253 项通过，含 064→066 升级、回滚和拒绝未知升级对。48 个测试实例审计确认 active=0、remaining credentials=0、sourceMismatch=0。

原生窗口 `88148638` 和候选进程 `74388` 实际启动，加载 `127.0.0.1:58750` 的合成测试登录页。重复启动保持一个进程；Alt+F4 后窗口从清单消失，桌面进程及后台仍存活；再次启动恢复相同 HWND。可访问性树保存在 `native-login-ax.txt`。这些是原生运行证据，不能替代视觉验收。

截图对候选窗口及 ChatGPT 窗口均超时，点击报 `coordinate input geometry is unavailable`。键盘/可访问性可以执行部分动作，但登录提交、窗口视觉、最小化/恢复、托盘左键与菜单、正常退出后的 WebView 会话恢复仍未验收。最终状态为 **blocked / 待人工验收**。合成窗口按准确的 PID/路径/实例命令行核对后强制结束并清理测试后台；强制结束不算正常退出通过。

### 本轮发现并修正的问题

Tauri 的配置加载函数会读目录中的常规 `tauri.conf.json` 再合并 `TAURI_CONFIG`；单独传入另一个文件名没有选中隔离配置。首包因此向正在运行的正式桌面单实例锁发出了唤起消息，曾让正式窗口出现在窗口清单中。正式桌面 PID 55500、daemon PID 10532 都是此前已运行的进程；没有操作正式窗口，没有执行正式配置、数据或凭据写入。

构建入口现在显式传入隔离 `TAURI_CONFIG`；二进制在初始化任何单实例插件前检查实际 identifier、初始窗口、capabilities 与 devUrl。配置不隔离则拒绝启动。修复后的最终包已观察到自己的独立原生窗口，并与正式桌面同时存在。首包 `066.1-24bf70b13cc6` 不可使用，等待手动删除。

## 审批拦截时的手动步骤

最终包的进程启动没有被审批阻止。当前剩余的是桌面截图/指针自动化不可用，因此需要人工完成上表尚未验收的交互。以下使用空白隔离测试实例，不访问生产数据或真实 Provider。

```powershell
& 'D:\Loadout\development\fusion\runs\066-desktop\packages\desktop\Start-Loadout.cmd' 'D:\Loadout\development\fusion\isolated\loadout-fusion-isolated\desktop-066'
```

如果需要登录，在自己的终端查看该隔离实例管理密码，不发送到聊天、不保存到文件：

```powershell
& 'D:\Loadout\development\fusion\runs\066-desktop\packages\desktop\fusion.cmd' management-password 'D:\Loadout\development\fusion\isolated\loadout-fusion-isolated\desktop-066' --show
```

按上表依次检查窗口/托盘；最后先从托盘退出窗口，再停止后台：

```powershell
& 'D:\Loadout\development\fusion\runs\066-desktop\packages\desktop\Stop-Backend.cmd' 'D:\Loadout\development\fusion\isolated\loadout-fusion-isolated\desktop-066'
```

Windows 安全提示或自动审批不能通过换工具、改包装或关闭安全保护来绕过。需要人工完成的动作与实际拒绝原因单独写入验收证据。

## 待人工清理（禁止把失败首包当候选使用）

删除首个失败 ZIP 的命令被自动审批拒绝，仅返回 `blocked by policy`，没有细项；未换工具重试删除。当前只指定一份 066.1-cec9977a9720 为候选，但物理上仍保留两个待删旧 ZIP（064 与失败的 066 首包）和约 1.97 GiB Rust 编译缓存，不能称为已经只剩一个 ZIP。

完成候选发布后，在 PowerShell 手动执行以下受限清理入口：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File 'D:\Loadout\scripts\fusion\cleanup-066.ps1'
```

脚本先验证当前候选 ID 与 ZIP SHA256，再仅删除列明的两个旧 ZIP、专用 Rust 缓存及约 0.3 MiB 静态 UI 构建目录。拒绝链接/junction 和仍被执行的缓存路径；保留全部源码、未提交修改、候选运行目录、正式安装与数据、凭据。清理后写入 `manual-cleanup.json` 并报告删除的文件字节数。该脚本尚未由代理运行，待删约 2.71 GiB，不把它计为已释放空间。

原有 059 隔离实例继续使用原来的运行目录；此次没有切换它，也没有为回滚再生成一个旧包。候选解压目录用于实际运行，不另建第二份候选副本。