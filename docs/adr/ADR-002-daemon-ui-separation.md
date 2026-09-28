# ADR-002: Daemon / UI Process Separation

- **Status**: Proposed
- **Date**: 2026-09-29

## Context

Harness §2.1 要求 Gateway 独立于 GUI 生命周期：关闭主窗口不得中断 API 服务。需要确定 Windows 上 Core 以什么形态运行、UI 如何与其通信、托盘归属，以及如何自启动与升级。

## Decision

### 进程划分

```text
loadoutd.exe      Core daemon（无窗口）
                  Gateway · Router · Provider Runtime · Control-plane loops
                  Storage · Secrets · IPC server

Loadout.exe       Tauri 桌面应用
                  React UI · 托盘图标 · IPC client

loadout.exe       命令行工具（开发 / 调试 / 脚本）
                  IPC client
```

### Core 形态：用户级后台进程，不是 Windows Service

- `loadoutd.exe` 以**当前登录用户身份**运行。
- 不使用 Windows Service。原因：Service 运行在 session 0 / 其他账户下，无法直接访问当前用户的 Credential Manager 条目，也不便读取 `%USERPROFILE%` 下各 CLI 的配置与登录状态（Discovery / Usage 依赖这些）。
- 单实例：通过 per-user 命名 mutex 保证。

### 自启动

- 通过 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 注册 `loadoutd.exe`（用户可在设置中关闭）。
- `Loadout.exe` 自启动为可选项，默认最小化到托盘。
- `Loadout.exe` 或 `loadout.exe` 启动时若发现 daemon 未运行，负责拉起它。

### 托盘

- 托盘属于 `Loadout.exe`。
- 关闭主窗口 = 隐藏到托盘；在托盘中选择"退出"只退出 UI，**不停止 daemon**。
- 停止 daemon 必须是显式操作（托盘菜单"Stop Gateway"或 CLI `loadout daemon stop`）。

### IPC

- Windows Named Pipe：`\\.\pipe\loadout-<user-SID>`。
- Pipe 的安全描述符只允许当前用户 SID 访问。
- 协议：长度前缀帧 + JSON 消息，请求/响应 + 服务端推送事件（用于 UI 实时刷新）。消息类型定义放在共享 crate，由 Tauri 后端与 CLI 共用。
- React 只调用 Tauri command；Tauri 后端（Rust）转发到 pipe。React 永不直接访问 Provider 或 Gateway 管理接口（Harness §22）。

### Management HTTP API

- 产品文档 §30 的 `/_loadout/*` HTTP 管理接口 **v1 不启用**。管理操作只走 IPC。
- 如未来启用，必须独立端口、默认关闭、需要单独的 management token（见 ADR-011）。

### 升级

- 升级流程：UI 经 IPC 请求 daemon 优雅退出（停止接收新请求、等待在途流结束或超时）→ 替换二进制 → 重新拉起。
- 安装器与自动更新细节留到 Phase 10。

## Alternatives

| 方案 | 否决原因 |
|---|---|
| Gateway 内嵌在 Tauri 进程 | 违反 Harness §2.1；UI 崩溃或退出会中断 API |
| Windows Service | 见上，无法访问用户凭据与用户目录 |
| 托盘放在 daemon 中 | daemon 需要窗口消息循环，失去"无界面后台进程"的简单性 |
| IPC 用 localhost HTTP | 需要额外端口与认证；Named Pipe 可直接用 OS ACL 限定用户 |

## Consequences

- 需要维护三个二进制，IPC 消息定义必须版本化（带协议版本号，UI 与 daemon 版本不一致时提示）。
- 用户注销时 daemon 随会话结束；这对个人桌面场景是可接受的。
- Phase 1 就交付 `loadout.exe` CLI，使 Phase 1–7 在没有 UI 时也能完成配置与调试。

## Open Questions

- 是否需要在用户未登录时提供服务（例如远程桌面场景）？当前假设不需要。
