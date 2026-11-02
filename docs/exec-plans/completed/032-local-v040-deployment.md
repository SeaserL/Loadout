# ExecPlan 032: Local v0.4.0 deployment

Status: COMPLETE

## Goal
将本机 Loadout 与桥接升级到已发布 v0.4.0，保留数据/凭据，确认保护及更新功能；不调用 DeepSeek。
## Context / Existing behavior
Loadout v0.3 从 target/v3/release 运行，自启动开启；桥接官方 2.0.32。v0.4 发布包及远端 SHA 校验通过，CI success。自动测速原值 true，已关闭以防升级后推理探针。
## Constraints
不导出 secret、不执行模型推理、不改账号/路由/client key。配置仅必要修改：关闭测速、开启软件更新、刷新原有自启动路径。
## Architecture impact
无代码变化。主程序迁入默认用户安装目录，原数据目录继续使用。
## Files expected to change
LOCALAPPDATA/Programs/Loadout 程序；桥接五脚本；Loadout 更新设置、自启动和快捷方式。旧开发 binary 保留。
## Implementation stages
备份数据库/配置/脚本与快捷方式；停止并排空；应用发布包；桥接保护启动；重启主程序；读取 IPC、校验配置/凭据引用及文件 hash。
## Tests
仅只读健康/目录/IPC；桥接升级静默窗口内本地429拒绝不触达上游；已部署保护类使用 fake worker 测试；后台自动 GitHub check；无 DeepSeek 请求。
## Performance validation
无热路径代码修改，不重跑模型或 benchmark。
## Security considerations
DPAPI 文件只比较 hash，不解密输出。桥接鉴权仅内存使用。Windows凭据不迁移或重写。
## Migration considerations
标准用户安装，同一数据目录及schema14，保留已有账号/client身份。自动更新安装需要标准路径。
## Known risks
文件锁、安装脚本启动失败、外部客户端请求。失败恢复旧程序与桥接脚本。
## Definition of done
运行0.4及发布桥接脚本、单worker保护确认、updater标准安装支持且自动检查成功、配置保留核验、无模型调用。
## Progress
- 发布包四文件hash核验，GitHub CI success。
- 现有后台自动测速已从true关闭；无OpenCode专用端点自动恢复探针。
## Decisions
- 自动更新检查/下载/安装启用；后续升级会重启网关。
- 官方OpenCode仍保留2.0.32，v0.4仅指Loadout及其桥接脚本。
- 2026-10-19 悉尼时间：完成本机部署。Loadout daemon PID212108、GUI PID148820 从标准目录运行0.4.0；数据目录不变，schema14，无恢复数据库事件。
- GitHub CI39128451148 success。主程序三文件与正式zip逐字节相同；桥接五文件与正式kit相同，官方版本仍2.0.32。
- 桥接health=ok、workers=1。升级静默窗口内本地429 bridge_minimum_interval，Retry-After=87；前后官方session列表相同。没有DeepSeek或其它模型推理。已部署ProtectedOfficial类使用fake worker验证403后30分钟以及重启后剩余1680秒冷却。
- 更新器后台自动检查成功（没有发送updates.check），current_version/latest_version=0.4.0/v0.4.0，phase=current，install_supported=true；checked_at_ms=1792330310733。自动检查/下载/安装均开启；当前无更新包待装，未制造虚假版本触发生产升级测试。
- 配置export中providers（含账号/模型）、pools、clients全部相等：10 providers、21 accounts、236 models、2 pools、15 client keys。数据库secret_ref列完全相等。桥接binding.json、secrets.dpapi、官方exe、setup与key helper hash不变。未导出或重写Windows凭据。
- 只变更benchmark.auto_probe=false（原true）、updates三个开关true、自启动迁至标准目录、开始菜单指向新GUI。桌面Loadout快捷方式指向仓库目录，保留不变。
- 本地回滚资料：%LOCALAPPDATA%/Loadout/backups/upgrade-v040-20261018T133048（before.db、before.json、快捷方式、自启动原值、安装包及验证回执）。桥接脚本备份：%LOCALAPPDATA%/LoadoutOpenCodeBridge/protection-backup-20261018T133113251。旧开发目录程序保留。
- 运行与IPC验证完成；未进行模型可用性或原生GUI视觉验收，不把保护验收视为上游403已解决。
