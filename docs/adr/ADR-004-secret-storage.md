# ADR-004: Secret Storage

- **Status**: Accepted
- **Date**: 2026-09-29

## Context

Loadout 会接触 API key、client key，以及（只读的）官方 CLI 登录状态。Harness §13 规定 SQLite 只能保存 `secret_ref`，secret 必须进入 OS credential storage；官方 CLI 管理的凭证应标记为 external-managed 而不是复制。

## Decision

### 1. Loadout 自有 secret → Windows Credential Manager

- 每个 secret 存为一条 Generic Credential，Target Name：`loadout:<secret_ref>`，`secret_ref` 为随机 UUID。
- 访问通过 `secrets` crate 中的 `SecretStore` trait，v1 只有 Windows 实现。
- Credential Manager 单条 blob 上限约 2.5 KB。API key 远小于此；如果未来需要存储更大的 secret，使用 DPAPI（`CryptProtectData`，当前用户范围）加密文件，`secret_ref` 指向该文件，仍不进入 SQLite。

### 2. External-managed 凭证 → 不复制

- Codex、Claude Code、Kimi Code、Gemini CLI 等由官方 CLI 管理的登录状态，记录为 `CredentialSource::ExternalManaged`，只保存**定位信息**（CLI 类型、配置目录路径），不保存 token。
- 需要时（Usage driver 后台刷新）按需读取，用完即丢，不缓存到磁盘、不写日志。
- 见 ADR-009。

### 3. SQLite 中允许的凭证字段

```text
id · account_id · credential_type · source · secret_ref · scope · expires_at · created_at · updated_at
```

禁止字段：`raw_api_key`、`access_token`、`refresh_token`、`cookie`、`password`。

### 4. Client key

- Client key 由 Loadout 生成（≥ 32 字节随机数），只在创建时向用户展示一次。
- SQLite 中保存 **SHA-256 哈希**与前缀（用于 UI 识别），不保存原文。高熵随机 key 使用快速哈希即可，无需 KDF。

### 5. 内存与日志

- Secret 在内存中使用带脱敏 `Debug` / `Display` 的包装类型（如 `secrecy::SecretString`），drop 时清零。
- 转发请求时，所有鉴权头（`Authorization`、`x-api-key`、`api-key`、`cookie`）在 tracing、错误信息、route decision 记录中一律脱敏。
- 测试必须覆盖：日志与错误输出中不出现 secret 原文（Harness §27 "Credential redaction tested"）。

### 6. 导出 / 备份

- 备份导出（Phase 9）默认**不包含 secret**；包含时必须用用户提供的口令加密，并显示警告。

## Alternatives

| 方案 | 否决原因 |
|---|---|
| SQLite 加密列（SQLCipher 等） | 密钥仍需存放在某处；OS store 由系统负责访问控制 |
| 复制官方 CLI 的 refresh token | 与官方 CLI 的刷新逻辑冲突（token rotation 会互相顶掉），且扩大泄露面 |

## Consequences

- Secret 与数据库分离，数据库文件泄露不会泄露 key。
- 迁移到新电脑需要重新输入 API key（或使用加密备份）。
