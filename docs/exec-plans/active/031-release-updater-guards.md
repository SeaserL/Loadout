# ExecPlan 031: OpenCode protection and Loadout release updates

Status: IN_PROGRESS

## Goal
整理OpenCode限制，加入连接保护；Loadout自身监测GitHub正式release、下载校验并支持自动安装；完成检查后提交、推送、发布正式版。用户已授权push和release。
## Context / Existing behavior
OpenCode403原因未确定，单CLI可能有标题请求。029已安装，030双worker真实验收受阻；不发布未经验证的双并发默认配置。Loadout目前没有updater，发布zip+sha256，安装器会停服务。
## Constraints
不再调用真实模型、不改正式运行实例。更新源固定SeaserL/Loadout，稳定版，HTTPS和SHA256，禁止任意URL/路径执行。UI通过Core IPC。保留已有未提交集成，发布前检查所有纳入内容及隐私。
## Architecture impact
Provider桥接内部串行+最小间隔+403熔断；Core后台updater，UI管理检查/自动下载/自动安装，独立后台安装助手备份和回滚。
## Implementation stages
1. Guard及离线契约测试，限制文档。
2. ADR018、Core updater/IPC/settings、界面、限定路径安装助手。
3. Mock/隔离更新测试、构建、fmt/clippy/workspace tests/license/benchmark。
4. 版本号、打包、审查发布文件、DCO commit、push、GitHub正式release与资产核验。
## Tests
403冷却及重启保留、并发拒绝、Retry-After；版本比较、错误元数据、下载hash和zip拒绝、安装失败回滚；默认tests无GitHub/模型网络。
## Performance validation
不在Router热路径查询网络，后台6小时检查；按需benchmark，下载有上限/超时。
## Security considerations
固定仓库稳定release资产，HTTPS，hash一致，文件白名单，安装目标限定自身程序目录；不运行release内脚本、不写凭据。SHA256提供传输完整性，不是独立发布签名。
## Migration considerations
复用settings无DB schema变化。updater缓存不含secret。正式安装到LOCALAPPDATA/Programs/Loadout才允许内置安装，开发目录只下载。
## Known risks
GitHub授权/网络失败，Windows二进制锁，重启中断与版本回滚。冷却值是本地保守策略，不是上游阈值。
## Definition of done
代码测试/文档/安全检查/发布资产齐全，远端正式release可验证；未完成项明确记载。
## Progress
- 确认目标Loadout自身；GitHub CLI无独立登录，但git credential manager的现有GitHub授权可用，已只读核验release v0.3.0。
## Decisions
- 下载GitHub release二进制而非对安装目录git pull。默认检查+下载，自动安装为明确可开关选项；手动安装按钮始终可用。
- OpenCode默认单worker，120秒最小间隔、403后30分钟冷却；拒绝而不隐式重试，无法保证避免403。

### Verification progress
- Python bridge/protection 28 tests, installer transaction 5 tests, Node hooks/WorkBuddy 8 tests passed offline.
- Full workspace tests passed on rerun. Initial web-session profile cleanup assertion failed once; targeted rerun and subsequent complete run passed without changing that test.
- Frontend build and clippy passed; found and corrected Windows canonical-path prefix and PowerShell UTF-8 BOM handling during updater review.
- Release build and benchmark are running. No inference calls or production replacement in this task.
- Anonymous isolated GitHub check returned 404: repository is private, not rate-limited. Added read-only default GCM credential access and scoped API asset authentication. Explicit ignored live GitHub download test passed against the existing release, including checksum and byte-size verification in a temporary cache; no installation or model call.
- Final default workspace: 404 passed, 0 failed, 1 explicit GitHub live test ignored; the ignored test was separately opted in and passed. Final updater-only regression suite 5 passed after the auto-install retry guard; installer/bridge kit transactions 7 passed; Node 8, Python protocol/protection 28 passed.
- Full benchmark completed: provider/model selection ~495 ns, exact selection ~429 ns, virtual selection ~7.92 us, gateway skeleton ~48.6 us, converted TTFT ~615 us. Several benches regressed against the historical local baseline while other builds/tests ran concurrently; this is not evidence of no regression. No Router/Gateway algorithm changes were made. All remain below the documented hot-path budget; no remote update work is in inference routing.
- fmt, all-target clippy, frontend build and third-party license policy passed (348 packages, 0 policy violations; 7 lack a bundled license file under the existing policy). Sensitive-field and token-pattern review found no new real credentials in release inputs.
- Bridge protection kit update and startup-failure rollback tested using temporary synthetic scripts only. Existing installed bridge is not changed; users apply the separate kit explicitly. No native GUI visual acceptance is claimed.
- Final publishing is pending. Repository remains private. Current developer runtime remains v0.3.0; GitHub release publication is separate from installing/restarting this runtime.
