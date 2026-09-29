# AGENTS.md

> **Loadout — All your AI, one endpoint.**

本文件是任何 AI coding agent（及人类贡献者）进入本仓库时的入口。完整开发约束见 [docs/development-harness.md](docs/development-harness.md)，本文件只做导航与强制规则摘要，两者冲突时以 Harness 为准。

---

## 1. Boot Sequence

每次开始工作按顺序执行，不得看到任务后直接写代码：

1. 读本文件
2. 读与任务相关的产品文档（见 §2）
3. 读相关 ADR（`docs/adr/`）
4. 检查 `docs/exec-plans/active/` 是否有进行中的计划
5. 读相关代码与测试
6. 确认现有 abstraction，再开始修改

---

## 2. Document Map

| 文档 | 作用 |
|---|---|
| [docs/product/product-technical-design.md](docs/product/product-technical-design.md) | 产品定义与整体技术设计 |
| [docs/development-harness.md](docs/development-harness.md) | 开发约束（最高级别之一） |
| [docs/architecture/glossary.md](docs/architecture/glossary.md) | 统一术语与状态定义 |
| [docs/adr/](docs/adr/) | 架构决策记录 |
| [docs/legal/reference-map.md](docs/legal/reference-map.md) | 开源协议策略 + 参考项目速查 |
| `docs/providers/<provider>.md` | 每个 Provider 的实现参考、认证方式、ToS 风险、最后验证日期 |
| `docs/exec-plans/` | 多步骤任务的执行计划 |

**优先级**：ADR > Harness > 产品设计文档。产品设计文档写于对齐之前，凡与 ADR 冲突之处，以 ADR 为准。

---

## 3. v1 Scope Decisions (summary)

详见对应 ADR。

- **只支持 Windows**（Windows 10/11 x64）。代码保留平台抽象，但不为 macOS/Linux 编写或测试实现。— ADR-001
- **Core 是独立的用户级后台进程**，GUI 只是控制面客户端。— ADR-002
- **路由 v1 = 延迟档位 + 有序粘性 failover 链**，不做加权打分。档位按 TTFT 分为 超快 / 快 / 普通，同档内按 TPS 排序。— ADR-003
- **订阅类凭证（Claude Code / Codex / Kimi Code / Gemini CLI 等）v1 只做发现、认证状态、用量读取**，不作为推理 Transport。— ADR-009
- **模型寻址 = `provider/account/model`**，账号别名在导入时由用户填写。— ADR-010
- **Responses API**：无状态完整支持；有状态请求仅在上游原生支持 Responses 时透传并固定路由，否则明确报错。— ADR-006
- **协议**：Apache-2.0 + DCO，不设 CLA。— reference-map

---

## 4. Non-Negotiable Rules

1. **热路径（`POST /v1/*` 选路前）禁止**任何网络请求、CLI 启动、数据库聚合查询、LLM/远程分类器。只允许读取内存中的 `RouteSnapshot` 与原子状态。— Harness §2.2, ADR-003
2. **不得每次请求启动 CLI 进程**。— Harness §2.4
3. **Capability-first**：禁止 `if provider == "xxx"` 形式的分支出现在 Provider 代码之外。— ADR-005
4. **Secret 永不进入 SQLite、日志、fixture、错误信息**。SQLite 只存 `secret_ref`。— ADR-004
5. **请求正文默认不落盘**。— ADR-008
6. **UI 不得直接访问 Provider**，只能经 IPC 调用 Core。— ADR-002
7. **不偷偷换模型**：exact model 请求默认只允许同模型换账号，换模型需用户显式开启。— ADR-003, ADR-010
8. **数据库变更只能通过 migration**，且必须有测试。— Harness §21
9. **Router / Gateway 改动必须跑性能 benchmark**。— Harness §20
10. 修改上述任何规则，必须先新建或修订 ADR。

---

## 5. Research & Code Reuse

- 实现已有领域功能（协议转换、用量查询、OAuth、Tauri 生命周期等）前，**先查 [docs/legal/reference-map.md](docs/legal/reference-map.md)**，禁止默认全网重新调研。
- 每个任务最多读 1 个 primary 参考 + 1 个 secondary 参考 + 官方文档/源码，不够再扩展。
- 官方源码/文档优先于社区逆向实现。
- 复制或移植代码前，核对**具体文件路径**的 license（不只看仓库名）。GPL/AGPL/未知协议只能参考概念。
- 复用代码必须记录到 `THIRD_PARTY_NOTICES.md` 并保留上游 notice。
- 发现更好的参考项目时，更新 reference-map，而不是只写在 ExecPlan 里。

---

## 6. ExecPlan

满足 Harness §6 任一条件（跨 crate、新增 Provider/协议、改 Router/schema/credentials/daemon/request lifecycle、性能敏感、超过一个 session）的任务，必须先建：

```text
docs/exec-plans/active/NNN-title.md
```

包含 Harness §6 列出的全部章节。**Progress 必须随工作实时更新**，完成后移至 `completed/`。

---

## 7. Coding Conventions

- 代码、代码注释、commit message、标识符：**英文**。文档：中文正文 + 英文术语，与现有文档保持一致。
- 原创源文件首行加 SPDX 头：
  - Rust / TS：`// SPDX-License-Identifier: Apache-2.0`
- Rust：`cargo fmt`、`cargo clippy -- -D warnings` 必须通过。
- 敏感值使用带脱敏 `Debug` 的 secret 包装类型，禁止对其 `derive(Debug)` 输出原文。
- Provider 特例封装在该 Provider 模块内部，不得为单个 Provider 修改公共接口；确有必要时走 ADR。

---

## 8. Testing

- 默认测试不得访问真实 Provider、不得消耗用户额度。
- Live tests 必须显式 opt-in：`LOADOUT_LIVE_TESTS=1`。
- Fixture 必须脱敏，不得包含真实 credential、账号 ID、邮箱。
- 测试分层见 Harness §19。

---

## 9. Git

- 默认分支 `main`。
- Commit 需要 DCO sign-off：`git commit -s`。
- 不提交：数据库文件、`.env`、任何真实 credential、`target/`、`node_modules/`。
- 推送（`git push`）由仓库所有者确认执行。

---

## 10. Build & Run

### 开发环境（Windows）

- Rust：由 `rust-toolchain.toml` 固定版本（MSVC toolchain，经 rustup 自动安装）
- Visual Studio Build Tools 2019+（"Desktop development with C++" 工作负载）
- Node.js LTS（Phase 8 桌面端）
- WebView2 Runtime（Windows 10/11 通常已预装）

### 常用命令

```text
npm install && npm run build                     构建桌面端前端（apps/desktop/dist）；cargo 编译 loadout-desktop 前必须先有它
cargo build --workspace                          构建全部（loadout.exe / loadoutd.exe / loadout-app.exe）
cargo test --workspace                           全部测试（含真实 Named Pipe / Credential Manager / e2e）
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo bench -p loadout-benchmarks                性能基准（Harness §20）
python scripts\third_party_licenses.py --check   第三方许可证策略检查（发布前必须通过）
powershell -ExecutionPolicy Bypass -File scripts\package.ps1   发布包：dist\Loadout-<版本>-windows-x64.zip
```

发布包内的 `install.ps1` / `uninstall.ps1` 源文件在 `packaging/`。测试安装脚本时必须传 `-InstallDir <临时目录> -ShortcutDir <临时目录> -NoStart`，不要触碰真实安装与开始菜单。

### 运行

```text
loadout daemon start                              启动 daemon（数据在 %LOCALAPPDATA%\Loadout）
loadout provider add <slug> --kind openai-compatible --base-url https://…/v1
loadout account add <slug> <alias>                提示输入 API key（不回显），并同步模型列表
loadout model list                                可用的 provider/account/model 名称
loadout client create <name>                      创建客户端使用的 client key
loadout route status | loadout logs               路由健康、Tier、延迟 / 最近请求
loadout route list                                loadout/realtime|fast|default 与 Pool 的路由顺序
loadout bench [name…]                             立即测速（默认：所有未测量的路由）
loadout pool set <name> <models…> [--ordering latency]
loadout-app                                       桌面管理界面（托盘常驻；未运行 daemon 时自动拉起）
loadout daemon stop
loadoutd --foreground                             前台运行，日志输出到终端
```

客户端配置：Base URL `http://127.0.0.1:8317/v1`，API key 为 client key，模型名用 `loadout/<virtual 或 pool>`、`provider/account/model` 或 `provider/model`。

桌面端开发：`LOADOUT_PIPE_NAME=\\.\pipe\loadout-<test>` 让 `loadout-app` 连接隔离的测试 daemon（先用 CLI 以隔离参数启动它）。

### 测试隔离约定

测试绝不能触碰用户真实安装。凡是启动 daemon 或写系统状态的测试必须使用：

- 临时数据目录（`--data-dir`）、端口 `0`；
- 唯一的 pipe 名（`--pipe-name \\.\pipe\loadout-<test>-<uuid>`）和实例 id（`--instance-id`）；
- Credential Manager 前缀 `loadout-test:`，Run 注册表值名 `Loadout-Test-*` / `Loadout-E2E-*`，并在 drop 时清理。

### 平台注意事项

- 测试 daemon 必须传 `--secret-prefix loadout-e2e-<id>:`，测试结束清理该前缀的凭据。
- 判断 daemon 是否运行一律通过 IPC；Windows 上连接已关闭的 loopback 端口约需 2 秒才失败。
- 用 PowerShell 批量替换文本时使用单引号 here-string（`@' '@`）；双引号版本会吞掉反引号。
- 启动后台进程必须用 `loadout_platform::spawn_detached`，不要用 `std::process::Command`（会泄漏调用方句柄，见 ExecPlan 001 Stage 7）。
- Credential Manager 调用必须串行（见 ADR-004）。
