# ExecPlan 007: Release Packaging, Recovery, Hardening and License Bundle

- **Status**: In progress
- **Created**: 2026-09-29
- **Phase**: Harness §26 Phase 10（E2E hardening、Performance、installer/update、recovery）

---

## 1. Goal

- 一条命令构建可分发的 Windows x64 包（zip），内含三个可执行文件、安装 / 卸载脚本、许可证与第三方许可证全文。
- 按用户安装（无需管理员），支持原地升级（ADR-002 升级流程）与卸载。
- 恢复能力：数据库损坏时自动隔离并以空库启动，同时保留每日自动配置备份，可一键导入恢复。
- 并发压力 e2e；性能基线文档；最终用户文档与系统测试清单。

## 2. Decisions

- **安装器形态**：v1 使用 PowerShell 安装脚本 + zip，而不是 NSIS / MSI。理由：个人使用、无需管理员权限；三个可执行文件（daemon / CLI / app）的布局不适合 Tauri 单应用 bundler；不引入额外构建工具。安装位置 `%LOCALAPPDATA%\Programs\Loadout`，数据仍在 `%LOCALAPPDATA%\Loadout`（互不影响）。签名安装器与自动更新作为后续工作。
- **升级**：`install.ps1` 检测到已安装 → `loadout daemon stop`（优雅退出，等待在途流）→ 结束 `loadout-app` → 覆盖文件 → 重新启动 daemon 与 app。数据库 migration 由新 daemon 在启动时执行（只前进）。
- **许可证合规**：`scripts/third_party_licenses.py` 从 `cargo metadata`（Windows 目标、正常依赖）与 `package-lock.json`（前端运行时依赖）生成 `THIRD_PARTY_LICENSES.md`，附带各包的许可证全文；允许列表：MIT、Apache-2.0、BSD-2/3-Clause、ISC、Zlib、Unicode-3.0 / Unicode-DFS-2016、CDLA-Permissive-2.0、BSL-1.0、CC0-1.0、0BSD、MPL-2.0（未修改的依赖，保留源码链接）。出现 GPL / AGPL / LGPL / 未知许可证即失败（reference map §5、§14）。
- **数据库损坏**：打开或 migration 时遇到 `SQLITE_CORRUPT` / `SQLITE_NOTADB` → 把 `loadout.db`（及 `-wal` / `-shm`）改名为 `loadout.db.corrupt-<unix 毫秒>`，新建空库并继续启动；`daemon.status` 与日志报告此事，UI 显示横幅并提示从自动备份导入。其他错误（权限、磁盘满）不做自动处理，按原逻辑退出。
- **Probe 样本保留**：`latency_samples`（只有 probe）保留 7 天，与统计窗口一致；ADR-008 §4 的"30 天 + 按小时聚合"不再需要（被动样本来自 `request_events`，按其 400 天保留）。
- **自动配置备份**：daemon 启动后及每 24 小时导出一次配置到 `<data>\backups\auto\`，保留最近 7 份（配置变化不频繁，体积小）。

## 3. Stages

1. 恢复：损坏隔离、自动备份与轮转、状态字段、UI 横幅；测试（伪造损坏文件启动 daemon）。
2. 打包：`scripts/package.ps1`、`install.ps1`、`uninstall.ps1`、许可证生成与检查；在临时目录执行安装 → 运行 → 升级 → 卸载的冒烟。
3. 硬化与性能：并发压力 e2e、基准复测、release 体积与 daemon 空闲内存，写入 `docs/performance/baseline.md`。
4. 文档：README（安装、快速开始、客户端配置、CLI / UI）、`docs/testing/system-test-plan.md`（用户系统测试清单）、AGENTS.md。

## 4. Tests

- 损坏的数据库文件 → daemon 正常启动、旧文件被保留、`recovered_from` 非空。
- 自动备份：启动后生成文件，超过 7 份时删除最旧的。
- 并发：32 个并发流式请求 × 若干轮经真实 daemon，全部成功，telemetry 条数正确。
- 许可证脚本：允许列表外许可证导致非零退出（自测）。

## 5. Definition of Done

- [ ] §1 全部完成，测试覆盖 §4
- [ ] clippy / fmt / 测试 / 前端构建通过；打包脚本产出 zip
- [ ] 文档完成；本文件移入 `completed/`

## 6. Progress

- [x] Stage 1 — recovery（2026-09-29）
  - `Database::open_or_recover`：`SQLITE_CORRUPT` / `SQLITE_NOTADB`（含 migration 中）→ 连同 WAL / SHM 改名为 `*.corrupt-<unix ms>`，新建空库；`daemon.status.recovered_from`；CLI 状态与 UI 横幅给出恢复步骤。
  - 每日自动配置备份到 `<data>ackupsuto`，保留 7 份；空配置不写，避免恢复后把好的备份轮转掉。
  - 修正：UI 中请求历史保留期写成了 30 天，实际按 ADR-008 为 400 天。
  - 教训：经 Bash heredoc 传给 Python 的反斜杠被吃掉一层，生成了退格字符、字符串续行变成空格；已全部修复并记入 memory，此后含反斜杠的编辑一律用 Edit 工具或 `chr(92)`。
- [ ] Stage 2 — packaging
- [ ] Stage 3 — hardening / performance
- [ ] Stage 4 — docs
