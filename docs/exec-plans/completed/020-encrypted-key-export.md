# ExecPlan 020: 一键导出 / 导入全部 API 账号（加密）

- **Status**: Completed
- **Completed**: 2026-10-17（v0.3.0）
- **Created**: 2026-10-17
- **Decision**: [ADR-004 §6](../../adr/ADR-004-secret-storage.md)（含 secret 的导出必须用用户口令加密并显示警告）
- **Related**: ExecPlan 006（不含 secret 的配置导出 `backup.export`）

---

## 1. Goal

项目所有者（2026-10-17）：一键导出所有 API 账号（含 API key）并在另一台电脑一键导入，用于和朋友共享同一套账号。所有者明确知晓并接受 key 外泄风险，要求导出做二步确认，并对文件加密。

## 2. Context / Existing behavior

- `backup.export` 永不导出 secret；导入后需逐个重新输入 key。
- ADR-004 §6 已规定：包含 secret 的导出必须使用用户提供的口令加密并显示警告。本计划是该条款的实现，无需新 ADR。

## 3. Decisions

- **范围**：所有 `api_key` 类型凭证的账号（及其 Provider 的 kind、slug、base URL、设置、账号别名 / 显示名、手动添加的模型）。不导出 OAuth、官方 CLI 托管、网页会话账号（无法共享，或违反 ADR-009 / ADR-012 / ADR-014），也不导出 client key。
- **加密强制**：没有明文导出选项。口令至少 8 个字符，输入两次。
  - KDF：PBKDF2-HMAC-SHA256，600 000 次迭代，16 字节随机 salt。
  - AEAD：ChaCha20-Poly1305，12 字节随机 nonce；文件头（格式、版本、KDF 参数、salt、nonce）作为 AAD，篡改即解密失败。
  - 实现用已在依赖树中的 `ring 0.17`（Apache-2.0 AND ISC），不引入新 crate。
  - 文件为 JSON 外壳（`format: "loadout-keys"`），密文 base64；扩展名 `.loadout-keys`。
- **二步确认**：
  1. 第一步：风险说明（文件含全部 key、任何拿到文件和口令的人都能使用这些账号并消耗额度、key 泄露后应在平台上吊销），勾选"我了解风险"；
  2. 第二步：设置口令（两次）并输入确认词 `EXPORT`。daemon 端也校验 `confirm == "EXPORT"`，CLI 同样要求输入。
- **审计**：每次导出 / 导入写一条 `warn` 级日志（账号数量、路径），不含 key 与口令。
- **导入**：缺失的 Provider 新建；同名账号默认跳过，勾选"覆盖已存在账号的 key"时更新 key；新账号写入 Credential Manager 后立即同步模型（复用 `account.add` 流程）。口令错误或文件被改动统一报"口令错误或文件已损坏"。
- 口令与 key 在 IPC 参数中使用脱敏 `Debug` 包装类型，解密出的明文用后即丢，不写日志、不进错误信息。

## 4. Files expected to change

- `crates/core/src/{keys_export.rs,dispatcher.rs,lib.rs}`、`crates/core/Cargo.toml`（`ring`）
- `crates/ipc/src/api.rs`、`apps/desktop/src-tauri/src/bridge.rs`
- `apps/loadout/src/bin/loadout/backup_cmd.rs`（`loadout backup export-keys` / `import-keys`）
- `apps/desktop/src/{api.ts,i18n.tsx,pages/Accounts.tsx}`

## 5. Tests

- 加解密往返；错误口令、篡改密文 / 文件头均失败；口令过短拒绝。
- 导出文件不含任何 key 明文。
- 导出 → 新库导入：Provider / 账号 / key（mock SecretStore）一致；同名跳过与覆盖。
- `confirm` 不为 `EXPORT` 时拒绝。

## 6. Performance / Security / Migration / Risks

- 不涉及热路径。600k 次 PBKDF2 约 0.2–0.5 秒，在阻塞线程池中运行。
- 无 schema 变更。
- 风险：文件与口令一起外泄 = key 外泄。界面明确提示"口令请通过另一渠道告知对方"。

## 7. Definition of done

桌面端与 CLI 均可导出 / 导入；二步确认生效；测试、clippy、fmt、`npm run build` 通过。

## 8. Progress

- [x] 2026-10-17 加密格式与导出 / 导入逻辑 + 测试
- [x] 2026-10-17 IPC / CLI
- [x] 2026-10-17 桌面端
- [x] 2026-10-17 验证：fmt / clippy / cargo test --workspace（383 通过）/ npm run build / 许可证检查通过；benchmark 只有噪声级波动（Router 未改，子号池只在构建路由表时过滤）
- [x] 2026-10-17 项目所有者实机测试（v0.3.0）
