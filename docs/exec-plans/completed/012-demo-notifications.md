# ExecPlan 012: Demo — Windows Toast Notifications

- **Status**: Completed (demo)
- **Created**: 2026-10-04
- **Background**: docs/research/2026-10-04-feature-landscape.md §5「通知（P1）」

---

## 1. Goal

daemon 在后台检测用户需要知道的状况，托盘应用以 Windows Toast 显示。不涉及热路径（ADR-003）：检测只在独立的后台任务里每 60 秒运行一次。

## 2. Design

- **daemon**（`crates/core/src/notify.rs`）：`spawn_restartable("notify")` 每 60 秒读取设置与状态，调用纯函数 `evaluate(inputs, settings, now)`，再经内存 `Dedup` 过滤后，通过 IPC 事件 `notify` 广播 `Notification {kind, key, title, body, subject, detail, value}`（定义追加在 `crates/ipc/src/api.rs` 末尾）。
- **条件**（`kind`）：
  - `quota_low`：任一用量窗口剩余低于 `notify.quota_percent`（默认 10，0 关闭）；已过 reset 时间的窗口忽略。
  - `balance_low`：USD 余额低于 `notify.balance_usd`（默认 1，0 关闭）。
  - `auth_failed`：路由处于 `AUTH_FAILED`（401/403），按账号合并。
  - `login_expiring`：Loadout 管理的 OAuth 登录（`credentials` 中 `oauth`/`access` 的 `expires_at`）在 23 小时内过期或已过期。刷新器提前 24 小时续期，留出 1 小时避免对即将自动续期的登录误报。
  - `client_cost_limit`：启用的客户端当日花费达到 `daily_cost_limit_usd`（与网关判定相同：`cost >= limit`）。
- **去重**：同一 `(kind, key)` 6 小时内最多发一次；内存表，daemon 重启后清空。出错时跳过本轮而不是重启任务，以免丢失去重表。
- **开关**：`notify.enabled`（默认 true）。三个新设置注册在 `crates/core/src/settings.rs`，桌面端设置页自动显示（中文描述在 `i18n.tsx` 的 `// Demo: notifications` 块）。
- **桌面端**（`apps/desktop/src-tauri/src/notifications.rs`）：Rust 侧后台任务经 `Bridge::subscribe` 订阅事件（daemon 不在时每 5 秒重连），用 `tauri-plugin-notification`（Apache-2.0 OR MIT，许可证检查通过）显示 Toast，并把托盘提示改为最近一条标题。窗口隐藏时同样生效。文字按 UI 语言本地化（`set_tray_language` 同时记录语言）；英文直接用 daemon 文本。插件只在 Rust 侧调用，因此不需要给 webview 增加 capability。

## 3. Verification

- `cargo test -p loadout-core notify`：阈值、关闭开关、按账号合并、过期窗口、客户端上限、6 小时去重、设置加载。
- `cargo test -p loadout-desktop`：Toast 文本本地化。
- `cargo fmt`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`npm --prefix apps/desktop run build`、`python scripts\third_party_licenses.py --check` 均通过。

## 4. Limitations

- 未在真实桌面上手动验证 Toast 弹出（避免在用户使用时做 GUI 自动化）。
- 安装版的 AppUserModelID 为 `dev.loadout.desktop`，但 `install.ps1` 创建的快捷方式没有设置该 ID；部分 Windows 版本对未注册的 AUMID 可能不显示或显示为原始 ID。正式版需要在快捷方式上写 `System.AppUserModel.ID`。
- 去重表不持久化；daemon 重启后仍成立的状况会再通知一次。
- 未实现：连续 failover、daemon 崩溃通知、Webhook（研究文档 §5 的其余项）。
- 余额只比较 USD；其他币种忽略。
