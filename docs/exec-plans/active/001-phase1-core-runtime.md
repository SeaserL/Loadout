# ExecPlan 001: Phase 1 — Core Runtime

- **Status**: In progress（2026-09-29 项目所有者确认）
- **Created**: 2026-09-29
- **Phase**: Harness §26 Phase 1 — Core runtime · Database · Daemon · Secret store · Gateway skeleton

---

## 1. Goal

交付一个**可运行但还不转发任何推理请求**的 Loadout Core：

```text
loadout.exe (CLI) ──Named Pipe──▶ loadoutd.exe
                                     ├─ Gateway skeleton  127.0.0.1:8317
                                     ├─ SQLite (WAL) + migrations
                                     ├─ Windows Credential Manager secret store
                                     ├─ tracing logs
                                     └─ single instance · autostart · graceful shutdown
```

完成后可以做到：

- `loadout daemon start|stop|status`：启动、停止、查看 daemon；
- `loadout autostart enable|disable`：开关登录自启；
- `loadout client create|list|revoke`：创建 client key（原文只显示一次）；
- 用 client key 调用 `GET /v1/models` 返回空列表；`POST /v1/*` 返回结构化的 "not implemented" 错误；
- 无 key、错误 key、非法 Host、带浏览器 Origin 的请求被拒绝。

Phase 2 在此基础上接入 canonical protocol 与第一个真实 Provider。

---

## 2. Context

- 相关 ADR：001（技术栈、Windows-only）、002（daemon/UI 分离、IPC、自启动）、004（secret 存储）、007（隔离/supervisor）、008（日志）、011（Gateway 安全）。
- 仓库目前只有文档，没有任何代码。
- 开发环境（2026-09-29 验证）：Rust 1.98.1 stable-msvc、VS Build Tools 2019（C++）、Node 22.18、WebView2。

## 3. Existing Behavior

无。这是第一个代码 ExecPlan。

---

## 4. Constraints

- 遵守 AGENTS.md §4 全部硬规则。
- 热路径（Gateway 请求处理）不得访问数据库：client key 校验使用内存中的哈希表（`ArcSwap`），client 变更时由 IPC 触发刷新。
- Secret 不进 SQLite、不进日志；client key 只存 SHA-256 哈希 + 显示前缀。
- 只实现 Windows；平台相关代码集中在 `platform` crate 的 trait 之后。
- 不引入 Phase 2 及以后的内容（协议转换、Provider、路由），但目录与 trait 命名不得与后续阶段冲突。
- 所有依赖必须是 MIT / Apache-2.0 / BSD 类许可证。

---

## 5. Architecture Impact

### 5.1 Workspace 结构

在 Harness §4 推荐结构基础上，Phase 1 只创建需要的部分：

```text
/
├─ Cargo.toml                 workspace（resolver 3，统一依赖版本）
├─ rust-toolchain.toml        固定 stable + msvc target + rustfmt/clippy
├─ .cargo/config.toml         静态链接 CRT 等 Windows 构建选项
│
├─ apps/
│  ├─ daemon/                 bin: loadoutd
│  └─ cli/                    bin: loadout
│
├─ crates/
│  ├─ core/                   runtime 组装、配置、supervisor、shutdown 协调
│  ├─ gateway/                Axum server、安全中间件、skeleton 路由
│  ├─ ipc/                    IPC 消息定义 + Named Pipe client/server
│  ├─ storage/                SQLite 连接、migrations、repositories
│  ├─ secrets/                SecretStore trait + Credential Manager 实现 + Secret 包装类型
│  └─ platform/               单实例 mutex、自启动、路径、当前用户 SID
│
├─ benchmarks/                criterion benches
└─ tests/integration/         跨进程集成测试（daemon + cli）
```

**新增（Harness §4 未列出）**：

- `crates/ipc`：daemon、CLI、未来 Tauri 后端共用的消息定义，必须独立成 crate。
- `crates/platform`：ADR-001 要求平台代码置于 trait 之后。
- `apps/daemon`、`apps/cli`：ADR-002 定义的三个二进制中的两个（`apps/desktop` 在 Phase 8）。

完成后更新 Harness §4 的结构图。

### 5.2 运行时数据位置

```text
%LOCALAPPDATA%\Loadout\
├─ data\loadout.db (+ -wal, -shm)
└─ logs\loadoutd.log.YYYY-MM-DD
```

所有路径可通过 `--data-dir` 覆盖（测试用）。

### 5.3 IPC

- Pipe 名：`\\.\pipe\loadout-<user-SID>`；测试可通过 `--pipe-name` 覆盖。
- 安全描述符：仅当前用户 SID（+ SYSTEM）可访问；拒绝远程客户端（`PIPE_REJECT_REMOTE_CLIENTS`）。
- 帧：4 字节长度前缀 + UTF-8 JSON（`tokio_util::codec::LengthDelimitedCodec`，单帧上限 1 MiB）。
- 消息信封：

```json
{ "v": 1, "id": 42, "method": "client.create", "params": { ... } }
{ "v": 1, "id": 42, "result": { ... } }
{ "v": 1, "id": 42, "error": { "code": "...", "message": "..." } }
{ "v": 1, "event": "daemon.state", "data": { ... } }
```

- 连接建立时交换 `hello`（协议版本、daemon 版本）；主版本不一致时 CLI 报错并退出。
- Phase 1 方法：`daemon.status`、`daemon.shutdown`、`client.create`、`client.list`、`client.revoke`、`settings.get`、`settings.set`。

### 5.4 Gateway skeleton

- 监听地址来自 settings（默认 `127.0.0.1:8317`）；端口占用时 daemon 启动失败并给出明确错误（不自动换端口，避免客户端配置失效）。
- 中间件顺序：`request-id` → `host check` → `origin check` → `client auth` → `body size limit` → handler。
- 路由：
  - `GET /v1/models` → `{"object":"list","data":[]}`
  - `POST /v1/chat/completions`、`POST /v1/responses` → OpenAI 格式错误，HTTP 501
  - `POST /v1/messages` → Anthropic 格式错误，HTTP 501
  - 其他 → 404
- 错误体按客户端协议格式返回（OpenAI `{"error":{...}}` / Anthropic `{"type":"error","error":{...}}`），Phase 2 复用。

### 5.5 Daemon 生命周期

```text
start
 ├─ 解析参数 / 路径
 ├─ 获取单实例 mutex（Local\Loadout-<SID>）→ 失败则退出码 2
 ├─ 初始化 tracing（文件 + 可选 stderr）
 ├─ 打开 SQLite，执行 migrations
 ├─ 加载 client key 哈希表
 ├─ 启动 IPC server、Gateway（由 supervisor 管理）
 └─ 等待 shutdown 信号（IPC daemon.shutdown / Ctrl+C / Windows 注销事件）

shutdown
 ├─ Gateway 停止接收新连接
 ├─ 等待在途请求（上限 30 s）
 ├─ 刷新 telemetry / 关闭 DB
 └─ 释放 mutex
```

- `loadout daemon start`：以 detached 方式启动 `loadoutd.exe`（无控制台窗口，`CREATE_NO_WINDOW | DETACHED_PROCESS`），轮询 IPC 直到就绪（上限 5 s）。
- `loadoutd.exe` 从同目录定位；找不到时报错。

### 5.6 自启动

- `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`，值名 `Loadout`，值为 `"<abs path>\loadoutd.exe" --background`。
- `enable` 时写入当前可执行文件的绝对路径；`status` 检查注册表路径是否仍指向存在的文件。

### 5.7 数据库 schema（migration v1）

Phase 1 只建本阶段用到的表，其余表在各自阶段通过新 migration 加入，不提前设计：

```sql
-- settings: key/value 设置（端口等）
settings(key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at INTEGER NOT NULL)

-- clients: 本地客户端（ADR-011）
clients(
  id          TEXT PRIMARY KEY,        -- UUID
  name        TEXT NOT NULL UNIQUE,
  key_prefix  TEXT NOT NULL,           -- 例如 "lo-3f9a"，仅用于显示
  key_hash    BLOB NOT NULL UNIQUE,    -- SHA-256(key)
  enabled     INTEGER NOT NULL DEFAULT 1,
  created_at  INTEGER NOT NULL,
  revoked_at  INTEGER
)
```

- 连接参数：`journal_mode=WAL`、`synchronous=NORMAL`、`foreign_keys=ON`、`busy_timeout=5000`。
- 每个 migration 同时提供 down 脚本。

---

## 6. Files Expected to Change

新建：

```text
Cargo.toml, rust-toolchain.toml, .cargo/config.toml, Cargo.lock
apps/daemon/**, apps/cli/**
crates/{core,gateway,ipc,storage,secrets,platform}/**
crates/storage/migrations/0001_init/{up.sql,down.sql}
benchmarks/**
tests/integration/**
.github/workflows/ci.yml
```

修改：

```text
AGENTS.md              §10 Build & Run
docs/development-harness.md  §4 结构图
THIRD_PARTY_NOTICES.md 若有复制/移植代码
```

---

## 7. Implementation Stages

每个阶段结束时 `cargo build`、`cargo test`、`cargo clippy -- -D warnings`、`cargo fmt --check` 必须通过。

### Stage 0 — Workspace 骨架

- workspace `Cargo.toml`、`rust-toolchain.toml`、各 crate 空壳、SPDX 头。
- 统一 lint 配置（`[workspace.lints]`：`unsafe_code = "deny"`，仅 `platform`/`secrets`/`ipc` 中 Windows FFI 模块局部允许）。
- 确认依赖版本组合可编译（特别是 `rusqlite` 与 `rusqlite_migration` 的兼容版本）。

### Stage 1 — `secrets`

- `Secret` 包装类型（基于 `secrecy`），`Debug` 输出 `Secret([REDACTED])`。
- `SecretStore` trait：`put`、`get`、`delete`、`exists`，参数为 `SecretRef`（UUID）。
- Windows 实现：`CredWriteW` / `CredReadW` / `CredDeleteW`，Target Name `loadout:<uuid>`，`CRED_TYPE_GENERIC`，`CRED_PERSIST_LOCAL_MACHINE`（不随漫游配置同步）。
- 测试使用 `loadout-test:` 前缀，测试结束清理。

### Stage 2 — `storage`

- 连接管理：一个写连接（专用线程）+ 只读连接池；Phase 1 规模下可先用单连接 + `Mutex`，但 API 形状按"写入走单 writer"设计，为 ADR-008 的 telemetry writer 预留。
- migration runner（`rusqlite_migration`），migration SQL 以文件形式嵌入。
- `SettingsRepo`、`ClientRepo`。

### Stage 3 — `platform`

- 当前用户 SID、数据目录、单实例 mutex、自启动注册表读写。
- `trait Platform` + `WindowsPlatform`。

### Stage 4 — `ipc`

- 消息类型（serde）、帧编解码、`IpcServer`（带 ACL 的 Named Pipe，多连接）、`IpcClient`。
- 协议版本握手。

### Stage 5 — `gateway`

- Axum server、四个中间件、skeleton 路由、双协议错误体。
- `ClientKeyIndex`：`ArcSwap<HashMap<[u8;32], ClientId>>`，常数时间比较不需要（查的是哈希），但 key 解析需拒绝过长输入。
- Graceful shutdown。

### Stage 6 — `core` + `apps/daemon`

- 组装运行时、supervisor（任务 panic 捕获 + 退避重启，ADR-007）、shutdown 协调。
- tracing：文件按天轮转，保留 14 个文件；脱敏层（过滤 `authorization` / `x-api-key` 等字段）。
- IPC 方法实现；`client.*` 变更后刷新 `ClientKeyIndex`。

### Stage 7 — `apps/cli`

- `clap` 子命令：`daemon start|stop|status`、`autostart enable|disable|status`、`client create|list|revoke`、`config get|set`。
- `client create` 输出 key 原文一次，并提示"只显示一次"。
- 输出人类可读格式；`--json` 输出机器可读格式。

### Stage 8 — 集成测试、benchmark、CI

- `tests/integration`：以临时 data dir、随机端口、唯一 pipe 名启动 daemon，驱动 CLI 与 HTTP 完成端到端场景（见 §8）。
- `benchmarks/`：criterion 基准（见 §9）。
- GitHub Actions（`windows-latest`）：fmt、clippy、test。
- 更新 AGENTS.md §10 与 Harness §4。

---

## 8. Tests

| 层级 | 覆盖 |
|---|---|
| Unit | `Secret` 的 Debug/Display 脱敏；key 生成格式与哈希；Host/Origin 校验规则表；IPC 帧编解码与超长帧拒绝；settings 解析 |
| Storage | migration up → down → up 往返；空库升级到最新；WAL 模式生效；client 唯一约束 |
| Secrets | 真实 Credential Manager 读写删（测试前缀，自动清理）；读取不存在的 ref 返回 `NotFound` |
| IPC | 版本不一致握手失败；并发多客户端；ACL：以非当前用户身份连接被拒（若 CI 环境无法构造，则以单元测试校验生成的 SDDL） |
| Gateway | 无 key → 401；错误 key → 401；已撤销 key → 401；`Host: evil.com` → 421；带 `Origin` → 403；合法请求 → `/v1/models` 200；`POST /v1/messages` → Anthropic 格式 501 |
| Integration | daemon 启动 → CLI 创建 client → HTTP 调用成功 → CLI 撤销 → HTTP 401（验证无需重启即刷新）；第二个 daemon 实例被单实例锁拒绝；`daemon stop` 优雅退出 |
| Redaction | 发送带 `Authorization` 的请求后，日志文件与错误输出中不出现 key 原文 |

默认测试不访问网络。

---

## 9. Performance Validation

Phase 1 建立 benchmark 基础设施与基线，数值记入本文件 §Progress：

| Benchmark | 目标 |
|---|---|
| `client_auth`：解析 header + SHA-256 + 哈希表查找 | P95 < 20 µs |
| `gateway_skeleton`：本机 HTTP `GET /v1/models` 往返（keep-alive） | P95 < 1 ms |
| `ipc_roundtrip`：`daemon.status` 往返 | P95 < 2 ms（非热路径，仅作参考） |

Harness §20 列出的 `route_selection`、`canonical_conversion`、`stream_proxy`、`snapshot_swap`、`sqlite_usage_write` 在对应阶段加入。

---

## 10. Security Considerations

- Named Pipe ACL 仅限当前用户，拒绝远程客户端。
- Gateway 只绑定 loopback；Host / Origin 校验防 DNS rebinding 与浏览器跨站调用（ADR-011）。
- Client key：≥ 32 字节随机数（OS CSPRNG），只显示一次，SQLite 只存哈希。
- 日志脱敏层 + 专门的脱敏测试。
- `unsafe` 仅限 Windows FFI 模块，每处写 `// SAFETY:` 说明。
- 请求体大小上限（默认 32 MiB，Phase 2 按需调整）。

## 11. Migration Considerations

- 这是第一个 migration（v1），没有旧数据需要升级。
- 从本阶段起：schema 只通过新增 migration 修改；每个 migration 有 down 脚本与往返测试（Harness §21）。

---

## 12. Known Risks

| 风险 | 缓解 |
|---|---|
| Named Pipe 安全描述符需要 Win32 FFI，容易出错 | 用 SDDL 字符串（`ConvertStringSecurityDescriptorToSecurityDescriptorW`）而不是手动构造 ACL；单元测试校验 SDDL |
| `rusqlite_migration` 与最新 `rusqlite` 版本不兼容 | Stage 0 验证；必要时固定 `rusqlite` 到兼容版本 |
| Run 注册表项中的未签名 exe 被杀毒软件误报 | 记录为已知问题；签名放到 Phase 10 installer |
| Credential Manager 在 CI runner 上行为差异 | Secrets 测试在 CI 中若失败，标记为需要交互式会话的测试并单独运行 |
| 私有仓库 GitHub Actions 分钟数（Windows runner 按 2 倍计费） | CI 只在 PR 与 push 到 main 时运行；启用依赖缓存 |
| 桌面应用终端 PATH 未刷新导致 `cargo` 不可用 | 已知；重启 Claude 桌面应用即可 |

---

## 13. Definition of Done

- [ ] §1 中列出的全部 CLI 命令与 HTTP 行为可用
- [ ] §8 全部测试通过；CI 绿色
- [ ] §9 基线数值记录在 Progress 中
- [ ] `cargo clippy -- -D warnings`、`cargo fmt --check` 通过
- [ ] 日志与错误中无 secret 原文（测试验证）
- [ ] AGENTS.md §10 写明构建、测试、运行命令
- [ ] Harness §4 结构图更新
- [ ] 所有新增依赖的许可证已核对（MIT / Apache-2.0 / BSD 类）
- [ ] 本文件移入 `docs/exec-plans/completed/`

---

## 14. Progress

- [x] Stage 0 — Workspace 骨架（2026-09-29）
  - workspace（edition 2024、resolver 3）、6 个 lib crate + 2 个 bin 空壳、`rust-toolchain.toml` 固定 1.98.1、CRT 静态链接。
  - `rusqlite` 0.40.2 与 `rusqlite_migration` 2.6.0 兼容：依赖树中只有一份 `rusqlite` / `libsqlite3-sys` 0.38.2。
  - 141 个第三方包许可证已扫描：全部为 MIT / Apache-2.0 / BSD / Zlib / Unicode-3.0 / Unlicense 等宽松协议；唯一含 LGPL 的包为多选一（`MIT OR Apache-2.0 OR LGPL-2.1-or-later`），按 MIT/Apache 使用。
  - `cargo build`、`cargo test`、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check` 全部通过。
  - `windows-sys` 的 feature 按 stage 逐步补充。
- [x] Stage 1 — secrets（2026-09-29）
  - `Secret`（Debug 输出 `Secret([REDACTED])`，无 Display/Clone/Serialize，drop 时清零）、`SecretRef`（UUID）、`SecretStore` trait、`InMemorySecretStore`（供其他 crate 测试）、`CredentialManagerStore`（Windows）。
  - Credential Manager：`CRED_TYPE_GENERIC`、`CRED_PERSIST_LOCAL_MACHINE`、UTF-8 blob、上限 2560 字节；读取后先清零 blob 再 `CredFree`。
  - **发现**：同一进程内多线程并发写 Credential Manager 会丢失删除（已删除条目在另一线程写入后重新出现）。复现：并行跑测试 20 次漏删 5 条，串行 0 条。修复：进程级锁串行化所有调用；新增回归测试 `concurrent_writes_do_not_resurrect_deletes`（去掉锁时 5/5 失败，加锁后并行 20 次 0 失败 0 残留）。已写入 ADR-004。
  - 测试：5 个单元测试 + 7 个 Credential Manager 集成测试（`loadout-test:` 前缀，drop guard 清理）。
- [x] Stage 2 — storage（2026-09-29）
  - `Database`（单连接 + Mutex，WAL / synchronous=NORMAL / foreign_keys / busy_timeout）、`with_conn` / `with_tx`、`SettingsRepo`、`ClientRepo`。
  - Migration 0001（`settings`、`clients`，STRICT 表，`key_hash` 长度约束），up/down 往返测试。
  - 数据库版本高于当前程序时拒绝打开（`SchemaTooNew`），不做降级猜测修复（Harness §21）。
  - Client 撤销只置 `enabled=0` + `revoked_at`，保留行以便请求历史可追溯；名称在撤销后仍占用。
  - 9 个测试。
- [ ] Stage 3 — platform
- [ ] Stage 4 — ipc
- [ ] Stage 5 — gateway
- [ ] Stage 6 — core + daemon
- [ ] Stage 7 — cli
- [ ] Stage 8 — 集成测试、benchmark、CI

---

## 15. Decisions

依赖选择（版本为 2026-09-29 crates.io 最新，Stage 0 锁定）：

| 用途 | 选择 | 许可证 | 备注 |
|---|---|---|---|
| async runtime | `tokio` 1.53 | MIT | 自带 Windows Named Pipe 支持 |
| HTTP server | `axum` 0.8 + `tower-http` 0.7 | MIT | ADR-001 |
| 帧编解码 | `tokio-util` 0.7 | MIT | `LengthDelimitedCodec` |
| SQLite | `rusqlite`（`bundled`） | MIT | 同步 API，配合专用 writer 线程；不用 `sqlx`（编译期检查需要数据库、异步收益不大） |
| migrations | `rusqlite_migration` 2.6 | Apache-2.0 | 基于 `user_version`，支持 down |
| Windows API | `windows-sys` 0.61 | MIT OR Apache-2.0 | Credential Manager、mutex、注册表、SID、pipe ACL；**不用 `keyring` crate**：只需 Windows，直接调用能控制 Target Name 与持久化类型，依赖更少 |
| Named Pipe | `tokio::net::windows::named_pipe` | MIT | **不用 `interprocess`**：tokio 已提供，且可传入原始 security attributes |
| secret 包装 | `secrecy` 0.10 | MIT OR Apache-2.0 | |
| 哈希 | `sha2` 0.11 | MIT OR Apache-2.0 | |
| 热更新 | `arc-swap` 1.9 | MIT OR Apache-2.0 | ClientKeyIndex，Phase 6 复用于 RouteSnapshot |
| CLI | `clap` 4.6 | MIT OR Apache-2.0 | |
| 日志 | `tracing`、`tracing-subscriber`、`tracing-appender` 0.2 | MIT | |
| benchmark | `criterion` 0.8 | Apache-2.0 OR MIT | |
| 其他 | `serde`、`serde_json`、`uuid`、`thiserror`、`anyhow`（仅 bin）、`rand` | MIT/Apache | |

其他决定：

- Phase 1 schema 只含 `settings`、`clients`；账户、凭证等表在 Phase 3 通过新 migration 加入。
- 端口被占用时不自动换端口。
- CRT 静态链接（`+crt-static`），减少用户机器上的运行时依赖。
