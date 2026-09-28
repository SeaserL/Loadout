# ADR-008: Request Logging and Privacy

- **Status**: Accepted
- **Date**: 2026-09-29

## Context

产品文档 §44 与 Harness §23 规定请求正文默认不落盘，只保存 metadata。同时 Usage Center（订阅价值、月度统计）需要足够长的 metadata 历史。日志写入不能增加 TTFT（Harness §20）。

## Decision

### 1. 默认记录（metadata）

`request_events`：timestamp、client、requested model、resolved virtual model / pool、route_target、provider、account、upstream model、protocol（in / out）、passthrough or converted、status、error category、input / output / cache / reasoning tokens、TTFT、TPS、duration、estimated cost、usage confidence。

### 2. 默认不记录

prompt、response、system prompt、tool arguments / results、图片与文件内容、任何鉴权头。

### 3. Debug Payload Logging

- 必须用户显式开启，可按 client 或全局开启。
- 开启时 UI 显示持续可见的警告标识。
- 保留期：24h（默认）/ 7d / 30d / 自定义；到期自动删除。
- 即使开启，鉴权头与 secret 仍脱敏。

### 4. 保留期（默认值，可配置）

| 数据 | 默认保留 |
|---|---|
| `request_events` | 400 天（覆盖年度订阅对比） |
| `latency_samples` 原始样本 | 30 天；之后仅保留按小时聚合 |
| `route_decisions`（debug） | 7 天，默认只记录 failover 与错误请求 |
| `response_id → target` 映射 | 30 天（ADR-006） |
| Debug payload | 见 §3 |

清理由后台任务执行，不在请求路径中进行。

### 5. 写入路径

```text
request → stream immediately
             └→ telemetry event → bounded channel → 单一 writer 任务 → SQLite（批量事务）
```

- channel 满时丢弃 telemetry 并计数，**绝不阻塞请求**。
- 丢弃计数在 UI 与 `health` 中可见。

### 6. 应用日志（tracing）

- 结构化日志默认 INFO，不包含请求正文。
- 写入 `%LOCALAPPDATA%\Loadout\logs`，按大小轮转。
- 所有字段经脱敏层处理（ADR-004 §5）。

## Alternatives

| 方案 | 否决原因 |
|---|---|
| 同步写数据库 | 增加 TTFT，违反 Harness §20 |
| 默认记录正文以便调试 | 违反 local-first 隐私承诺 |

## Consequences

- 出问题时默认无法看到请求正文，需要用户主动开启 debug payload 后复现。
- 高并发突发时 telemetry 可能丢失少量记录；丢失会被计数，不会静默。
