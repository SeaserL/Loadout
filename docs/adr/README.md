# Architecture Decision Records

| ADR | 标题 | Status |
|---|---|---|
| [001](ADR-001-tauri-rust-core.md) | Tauri 2 + Rust Core, Windows-only v1 | Accepted |
| [002](ADR-002-daemon-ui-separation.md) | Daemon / UI Process Separation | Accepted |
| [003](ADR-003-snapshot-routing.md) | Snapshot Routing and v1 Routing Policy | Accepted |
| [004](ADR-004-secret-storage.md) | Secret Storage | Accepted |
| [005](ADR-005-provider-capability-model.md) | Provider Capability Model | Accepted |
| [006](ADR-006-canonical-protocol.md) | Canonical Protocol, Passthrough, and Responses Statefulness | Accepted |
| [007](ADR-007-plugin-isolation.md) | Provider Isolation (No Dynamic Plugins in v1) | Accepted |
| [008](ADR-008-request-logging-privacy.md) | Request Logging and Privacy | Accepted |
| [009](ADR-009-subscription-credential-scope.md) | Subscription Credential Scope in v1 | Accepted |
| [010](ADR-010-model-addressing.md) | Model Addressing | Accepted |
| [011](ADR-011-local-gateway-security.md) | Local Gateway Security | Accepted |
| [012](ADR-012-subscription-transport-opt-in.md) | Opt-in Subscription Transport (Codex) | Accepted |

## Status 含义

- **Proposed**：已起草，等待项目所有者确认。
- **Accepted**：生效，代码必须遵守。
- **Superseded by ADR-NNN**：被替代，保留作历史记录。

## 模板

```markdown
# ADR-NNN: Title

- **Status**: Proposed
- **Date**: YYYY-MM-DD

## Context
## Decision
## Alternatives
## Consequences
```

ADR 与产品设计文档冲突时，以 ADR 为准。
