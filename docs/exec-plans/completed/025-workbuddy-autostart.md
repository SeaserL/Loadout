# ExecPlan 025: WorkBuddy bridge autostart

- **Status**: Completed（实际重启电脑未执行）

## Goal
Windows 登录后自动恢复 Loadout 与桥接，重复启动安全。
## Context
用户要求桥接跟随 Loadout，无需手动脚本；已有国际版登录。
## Existing behavior
WorkBuddy.WorkBuddyAI Run 已注册。Loadout 未注册；桥接仅有手动脚本。
## Constraints
不修改客户端登录或其它进程；secret 不落日志；复用 spawn_detached。
## Architecture impact
部署层用户级守护通过 IPC status 检查 daemon，可用时确保 bridge。不改 Core/Router/Provider 接口。
## Files expected to change
启动/安装 PowerShell、Node supervisor、platform detached launcher example、Provider notes、ADR-016、本计划；安装目录脚本与两个 Run 注册项。
## Implementation stages
检查启动项；实现互斥/健康检查/守护；备份安装；验证重复启动与恢复。
## Tests
语法、并发启动、桥接恢复、生产 exact-model JSON/SSE（显式 opt-in）。不重启电脑。
## Performance validation
只改部署控制面，无 Router/Gateway benchmark 要求。
## Security considerations
DPAPI key 仅私有进程输出与内存；不打印异常 payload。互斥按用户和目录隔离。
## Migration considerations
无 schema 变更。备份脚本和目标 Run 值，保留 WorkBuddy 原自启。
## Known risks
登录 Windows 后才运行。客户端退出或登录过期仍需恢复。依赖 Node/Python/Loadout 安装路径。
## Definition of done
注册启动项；单实例、恢复和真实请求通过；明确没有物理重启验收。
## Progress
- 2026-10-18：完成启动阅读与运行状态归属；开始部署层守护实现。
- 完成启动脚本 Mutex/认证健康/端口占用保护/就绪等待。PowerShell 无控制台直接 detached 会退出，改为已验证可 detached 的 Node 常驻守护，PowerShell 只做隐藏窗口短暂检查。
- 注册 Loadout 与 LoadoutWorkBuddyBridge Run；保留官方 WorkBuddy 自启。原配置首份备份 autostart-backup-20261017T195311670（UTC）。
- 三次重复守护启动只保留 PID 168156；停止旧桥接 PID 136588 后自动恢复为 170984；生产池 JSON/SSE 均 HTTP 200 且 marker 通过。没有停止客户端或 daemon。
- launcher 改为 Windows GUI subsystem，release build、example clippy、workspace fmt、diff check、脚本 parse/Node syntax 均通过；更新安装副本并重新拉起守护 PID 155868，状态 ready，仍只有一个 bridge 和一个 watcher，原 6 个 WorkBuddy 进程全部保留。
- 检查三个目标 StartupApproved Run 值均无禁用覆盖。尚未实际注销/重启；登录过期需要用户在官方客户端重新登录。
## Decisions
独立 Run 守护等待 IPC，兼容 GUI/CLI 启动顺序；Loadout 停止时不强杀 bridge，避免中断在途请求。
