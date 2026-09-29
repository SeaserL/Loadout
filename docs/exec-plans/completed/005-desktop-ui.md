# ExecPlan 005: Desktop Management UI (Tauri 2 + React)

- **Status**: Completed
- **Created**: 2026-09-29
- **Phase**: Harness §26 Phase 8（ADR-001、ADR-002、Harness §22）

---

## 1. Goal

一个托盘常驻的桌面管理界面，覆盖 CLI 的日常操作：

| 页面 | 内容 |
|---|---|
| Dashboard | Gateway 状态与地址、今日请求 / token / 花费、各账号额度摘要、Virtual Model 当前首选路由 |
| Accounts | Provider 与账号列表、添加 Provider / 账号（API key）、扫描并导入本机 AI 工具、改 key、启停、删除、订阅计划、用量窗口与余额 |
| Models | 模型列表、同步、启停、Tier 固定、价格 |
| Routes | Virtual Models 与 Pools 的顺序、每个 route 的状态 / Tier / TTFT / TPS、立即测速、Pool 编辑 |
| Clients | 客户端 key 创建（仅显示一次）/ 撤销，接入示例 |
| Logs | 最近请求（仅元数据） |
| Settings | 所有设置项、开机自启（daemon）、启动 / 停止 daemon |

## 2. Constraints

- ADR-002：UI 是独立进程，只通过 Named Pipe IPC 访问 daemon；关闭窗口 = 隐藏到托盘；托盘"退出"只退出 UI；停止 daemon 必须显式操作。UI 启动时若 daemon 未运行则拉起。
- Harness §22：React → Tauri command → IPC → Core。前端不访问 Provider、不访问 Gateway 管理接口。
- 密钥：API key 只经 Tauri command 转发给 daemon，不写入前端存储；客户端 key 只在创建时显示一次。
- 前端只加载本地打包资源；CSP 禁止外部来源。

## 3. Design

### 3.1 结构

```text
package.json                 npm workspaces: ["apps/desktop"]
apps/desktop/
  package.json               React 19 + TypeScript + Vite
  index.html, src/           前端
  src-tauri/                 Rust crate `loadout-desktop`（workspace 成员），产物 loadout-app.exe
```

- **可执行文件名**：`loadout-app.exe`。Windows 文件名不区分大小写，ADR-002 中的 `Loadout.exe` 会与 CLI `loadout.exe` 冲突；产品显示名仍为 "Loadout"。（ADR-002 追加说明。）
- 构建顺序：`npm run build`（生成 `apps/desktop/dist`）→ `cargo build`。`tauri::generate_context!` 在编译期嵌入前端资源，因此 `cargo test --workspace` 之前需要先构建前端（CI 与检查脚本同样处理）。不依赖 tauri-cli；启用 `tauri/custom-protocol` 特性以生产模式加载内嵌资源。

### 3.2 Tauri 后端（Rust）

- `ipc_call(method, params)`：按**白名单**把调用转发给 daemon（`IpcClient::call_raw`），每次调用独立连接（请求量很小，避免连接状态管理）；daemon 未运行时返回 `not_running` 错误码供前端显示"启动"按钮。
- `daemon_start()`：以 `spawn_detached` 拉起同目录的 `loadoutd.exe --background`，轮询直到 IPC 可用（15 s）。
- `daemon_stop()`：`daemon.shutdown`。
- `autostart_get/set()`：`loadout_platform::Autostart`（daemon 自启动，HKCU Run）。
- 托盘：显示主窗口、Gateway 地址（复制）、启动 / 停止 Gateway、退出 UI。
- 单实例：`tauri-plugin-single-instance`，再次启动时聚焦已有窗口。
- 关闭窗口：阻止关闭并隐藏。

### 3.3 前端

- React 19 + TypeScript，无 UI 框架依赖（自写少量 CSS），Hash 路由的简单页面切换。
- `api.ts`：类型化的 IPC 包装（类型与 `crates/ipc/src/api.rs` 一一对应）。
- 数据刷新：页面可见时每 5 s 轮询当前页面需要的数据（IPC 本地调用成本很低）。
- 错误显示：IPC 错误信息原样展示（daemon 的错误信息已为用户可读）。

## 4. Stages

1. 脚手架：npm workspace、Vite + React + TS、Tauri crate、白名单 IPC 转发、daemon 启停、托盘、单实例、窗口隐藏；构建脚本 / CI 更新。
2. 页面：Dashboard、Accounts（含扫描导入）、Models、Routes（含 Pool 编辑、测速）、Clients、Logs、Settings。
3. 测试与文档：Rust 单元测试（白名单、错误映射）、`tsc --noEmit` 类型检查、手动冒烟清单、README / AGENTS.md 构建说明。

## 5. Tests

- Rust：白名单拒绝未知方法；`not_running` 错误映射。
- 前端：TypeScript 严格模式类型检查（`npm run typecheck`），构建成功。
- 冒烟（手动，写入文档）：启动 UI → 自动拉起 daemon → 添加 provider / account → 创建 client key → 发请求 → Logs / Routes 刷新 → 关闭窗口后 Gateway 仍可用 → 托盘退出。

## 6. Security

- 白名单外的方法（例如未来的调试方法）不可从前端调用。
- CSP：`default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:`；不加载远程资源。
- Tauri capabilities 只开放自定义命令与窗口 / 事件的最小权限。

## 7. Known Risks

| 风险 | 缓解 |
|---|---|
| Tauri 依赖使编译变慢 | 可接受；CI 缓存 |
| 前端未构建时 cargo 编译失败 | 检查脚本与 CI 先运行 `npm ci && npm run build`；AGENTS.md 说明 |
| WebView2 缺失 | Windows 10/11 通常已安装；安装器（Phase 10）引导安装 |

## 8. Definition of Done

- [x] §1 全部页面可用
- [x] clippy / fmt / 测试 / typecheck / build 通过
- [x] AGENTS.md 构建说明；ADR-002 可执行文件名说明（README 与手动系统测试清单在最终交付时统一编写）
- [x] 本文件移入 `completed/`

## 9. Progress

- [x] Stage 1 — scaffold（2026-09-29）
  - npm workspace（React 19 + TypeScript 7 + Vite 8）、`loadout-desktop` crate（Tauri 2.12，`custom-protocol` 内嵌前端，不依赖 tauri-cli）、白名单 IPC 转发、daemon 启停、托盘、单实例、关闭即隐藏、开机自启开关。
  - 图标由 `icons/make_icons.py`（纯 Python）生成。
  - CI：先 `npm ci && npm run build` 再 cargo；workspace lint `linker_messages = "allow"`（MSVC 对导出符号的二进制打印 "Creating library"）。
- [x] Stage 2 — pages（2026-09-29）
  - Dashboard、Accounts & usage（添加账号、扫描导入、改 key、计划、启停、删除、额度 / 余额 / 订阅价值）、Models（Tier 固定、价格、手动添加）、Routes（Virtual / Pool 顺序、测速、Pool 编辑、健康与延迟表）、Client keys（仅显示一次 + 接入示例）、Request log、Settings。
- [x] Stage 3 — tests / docs（2026-09-29）
  - Rust：白名单与错误映射测试；`tsc --noEmit` 严格模式通过。
  - 冒烟：隔离 daemon（临时数据目录、专用 pipe、测试凭据前缀）+ `LOADOUT_PIPE_NAME` 启动 `loadout-app`，逐页截图核对；测速失败信息、Pool、设置均正常；结束后清理凭据与数据。

## 10. Decisions

- 桌面可执行文件名 `loadout-app.exe`（§3.1）。
- 不引入 UI 组件库，控制体积与依赖审计范围。
