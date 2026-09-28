# ADR-007: Provider Isolation (No Dynamic Plugins in v1)

- **Status**: Accepted
- **Date**: 2026-09-29

## Context

产品文档把 Provider 类比为"设备 driver"，而不是用户手动安装的插件。Harness §18 / 产品文档 §47 要求 Provider failure isolation：一个坏账号、一个异常 Provider 不能拖慢或拖垮其他 Provider 与 Gateway。

## Decision

### 1. v1 不支持动态插件

- 所有 Provider 模块编译进 `loadoutd.exe`（`crates/providers/<slug>/`）。
- 不加载 DLL、不运行 WASM 插件、不接受第三方 Provider 包。
- 新增 Provider = 新增 crate 模块 + 注册描述符 + 发版。

### 2. 进程内隔离手段

| 风险 | 隔离措施 |
|---|---|
| 后台任务卡死 | 每个 Provider 的 control-plane 任务独立 spawn，所有外部调用有超时（默认：usage 10 s、catalog 30 s、discovery 命令 5 s） |
| 后台任务 panic | 由 supervisor 捕获 `JoinError`，记录错误并按退避重启该任务，不影响其他 Provider |
| Transport 异常 | per-RouteTarget circuit breaker（ADR-003），故障不传播到同 Provider 其他账号 |
| 单 Provider 占用过多资源 | per-Provider 并发上限（后台任务）；per-target 在途请求计数 |
| 子进程（Discovery 用的 `--version` / status 命令） | 仅在后台执行；带超时并强制 kill；禁止在热路径执行 |

### 3. 热路径与控制面隔离

- 热路径不 await 任何 control-plane 任务。
- control-plane 结果只通过快照重建（ADR-003）与原子状态影响热路径。

## Alternatives

| 方案 | 否决原因 |
|---|---|
| 每个 Provider 一个子进程 | 隔离最强，但增加 IPC 跳数与热路径延迟；v1 Provider 均为自研代码，收益不足 |
| WASM 插件 | 为未来第三方扩展预留，v1 无需求 |

## Consequences

- 一个 Provider 模块中的内存安全问题理论上能影响整个 daemon；依赖 Rust 安全性与代码审查缓解。
- 未来若开放第三方 Provider，需要新 ADR（候选：子进程或 WASM 沙箱）。
