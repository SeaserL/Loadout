# ADR-001: Tauri 2 + Rust Core, Windows-only v1

- **Status**: Accepted
- **Date**: 2026-09-29

## Context

Loadout 需要一个常驻、低开销、低延迟的本地 Gateway，以及一个桌面管理界面。用户群体在 v1 阶段只有 Windows 用户（项目所有者本人）。Router 热路径的目标开销是 P95 < 5 ms（Harness §2.3），流式代理需要稳定、低额外 TTFT。

## Decision

1. **Core 使用 Rust**：Tokio、Axum、Tower、Hyper / Reqwest、rustls、Serde、tracing。
2. **Desktop 使用 Tauri 2 + React + TypeScript**。
3. **Storage 使用 SQLite（WAL mode）**；数据库访问库与 migration 工具在 Phase 1 ExecPlan 中选定，要求：migration 以版本化 SQL 文件管理、可测试。
4. **v1 只支持 Windows 10/11 x64**，toolchain 为 `x86_64-pc-windows-msvc`。
   - 平台相关代码（secret store、进程/IPC、自启动、路径）放在 trait 之后，但 v1 只提供 Windows 实现。
   - 不为 macOS / Linux 编写实现或 CI。
5. Cargo workspace + npm workspace 单仓库，结构遵循 Harness §4。

## Alternatives

| 方案 | 否决原因 |
|---|---|
| Electron + Node Core | 常驻内存高；Node 网关在尾延迟与 CPU 上不如 Rust 可控 |
| Go Core（参考 CLIProxyAPI） | 生态合适，但 Tauri 后端本身就是 Rust，一种语言能共享类型与 IPC 定义 |
| 纯 Rust GUI（egui / Slint） | 管理界面以表格、图表、表单为主，Web 技术栈开发效率更高 |
| 首版跨平台 | 增加三套 secret store / 自启动 / IPC 的实现与测试成本，没有当前用户需求 |

## Consequences

- 开发机需要 Rust MSVC toolchain、VS Build Tools（C++ 工作负载）、Node.js LTS、WebView2。
- Windows 专属 API（Credential Manager、Named Pipe、Run 注册表项）可直接使用，简化 v1。
- 未来支持其他平台时，只需补齐 trait 实现，不改变 Core 架构。
