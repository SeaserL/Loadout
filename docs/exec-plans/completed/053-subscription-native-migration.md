# 053 订阅行为移植与原生别名迁移

## Goal
移植已有订阅扫描与受控登录行为，使用OmniRoute原生映射补齐旧别名和账号池；交付一个隔离候选包。

## Context
用户授权依052继续，优先少维护。不做UI、不动正式环境、不提交上传。依据ADR-012、ADR-019及融合功能清单。

## Existing behavior
Rust codex只读扫描app-server/会话用量；codex_subscription与core/oauth完成自有device-code登录、vault、刷新和逐账号opt-in。融合052只有API-key profile及WorkBuddy连接器，拒绝未绑定认证与旧客户端策略。

## Constraints
不读正式secret，不自动登录，不运行真实客户端。使用loopback模拟认证与推理、独立数据及loadout-test vault。保留全部未提交源文件。外部账号问题延期。

## Architecture impact
将已有行为翻译为Node/TypeScript小模块，复用原生Provider持久化、vault写入生命周期和Responses执行器；不引入第二网关/整套Rust Core。受控登录的认证类型不得冒充官方客户端token。别名使用原生model-combo-mappings，映射和权限一并预检。

## Files expected to change
fusion扫描/CLI/迁移工具；当前OmniRoute loadoutPhase1适配、必要executor接缝与管理入口；development/experiments/omniroute-phase13测试/证据；来源记录、操作说明及状态清单。

## Implementation stages
1. 移植扫描、登录/轮换/失效处理与请求规范化；保持原有操作语义。
2. 原生别名映射及受限client key/旧池迁移，不丢成员。
3. 定向模拟验证、最终包集中回归及性能测量。
4. 仅留当前候选，清理临时展开与构建缓存，记录差异及未验收项。

## Tests
扫描RPC与日志回退；登录pending/批准/过期；双确认、刷新串行/失效停重试；凭证更新删除和加密恢复；三协议与工具正文保持；别名权限和同模型/显式跨模型池；未知映射拒绝；现有8组回归。

## Performance validation
最终包mock HTTP基准；不将mock结果称为真实模型性能。不得为认证每次推理启动CLI。

## Security considerations
认证响应和token只在内存/vault；id_token丢弃，错误输出固定分类。原生写入生命周期先存新秘密、切引用、再清旧。实例网络隔离不变。

## Migration considerations
旧只读账号须明确作为只读清单保留，不制造推理连接。受控登录绑定必须显式，别名映射不得自动选择未知默认目标。正式数据只读亦非本轮必要输入。

## Known risks
Rust与Node运行时不同，不能宣称二进制复用；以原行为和用例为准。原生别名权限检查顺序需实测。052旧包的验收不覆盖新增OAuth。

## Definition of done
对应行为在最终同一隔离包通过；旧池/策略有完整映射或具体阻塞证据；一个当前候选；测试凭据及进程清理；未触及正式环境。

## Progress
- [x] 核对ADR、已有Rust逻辑和原生接缝。
- [x] 移植订阅扫描与受控登录；扫描/规范化4项、最终包订阅与别名18项通过。
- [x] 原生别名及受限客户端配置已实现；严格点名、显式跨模型池与8RPM验证通过。
- [x] 最终包120/120、052→053升级/回滚4项、锁竞态5项通过；35实例凭据/所属进程均0。
- [x] 清理36个临时目标约15.40 GB；只留396 MB候选，项目约6.70 GB；[验收报告](../../research/2026-10-31-subscription-native-migration.md)与来源清单更新。

阶段发现：首次登录被现有凭证元数据白名单拒绝，修正为明确列出的托管订阅字段；未开放任意metadata。诊断构建仅用于定位，最终验收必须重新从源码构建，不交付修改过的诊断产物。

最终回归发现：原生key创建不保存rateLimits，外围迁移改为随后调用原生PATCH；18项专项已通过。052→053实际包升级/回滚4项通过。第一轮集中回归在停机备份遇到ENOENT，补充反例证明status/assertStopped/takeLock均存在“检查存在后读取锁文件”竞态；只在自有启动工具对锁文件消失的ENOENT作正常退出处理，损坏锁和活跃锁仍拒绝。工具重封包后重验，不改上游Router或SQLite。

## Decisions
只迁移已实现行为，不重新研究/设计登录；原生能力优先。本轮不扩展Radar、UI和外部账号排障。
