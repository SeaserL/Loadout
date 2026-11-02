# ExecPlan 045: 原生有界等待与 Provider 隔离排查

## Goal
接受原生全局/累计等待语义，验证等待和重试有限、账号范围不越界、模型不暗换；排查 WorkBuddy IPC 与 CommandCode 401，参考商汤运行历史限制调用。
## Context
用户取消旧端点严格30秒deadline要求；确认WorkBuddy桌面登录、商汤额度正常，并授权自行排查CommandCode。043冻结包与044映射是现有 abstraction。
## Existing behavior
044编译器因旧等待契约阻止端点激活；原生wait全局、累计sleep预算不等于总请求时间。历史商汤短窗口TPM/RPM限制约20秒恢复。
## Constraints
无UI、无正式环境写入/重启、无批量商汤探测；运行时补丁冻结。真实排查必须LOADOUT_LIVE_TESTS=1，固定官方origin，少量请求，失败停止，凭证仅内存/隔离vault。
## Architecture impact
新增显式native-bounded-v1映射选项，通过原生resilience配置，不写私有等待循环。旧计划不自动升级。
## Files expected to change
phase5映射/CLI；phase6隔离脚本和证据；ADR019及验证报告。
## Implementation stages
1. 复核历史、原生循环和认证链。
2. 更新候选ADR与显式映射profile，验证限制及恢复。
3. 排查WorkBuddy IPC、CommandCode认证；商汤只做历史与mock对照。
4. 清理自有测试状态，报告结果与剩余阻塞。
## Tests
短等待恢复、持续429耗尽、预算/单次上限、401不重试、取消、scope交集、同模型、三协议、旧计划和修改计划拒绝、恢复后配置。
## Performance validation
固定包正常请求mock时延；限速等待单列，不把主动等待算Router开销。
## Security considerations
不输出真实key、IPC secret、响应正文或账号身份；只保存allowlist诊断字段。测试凭证与进程清理。
## Migration considerations
原生全局等待影响所有combo，用户接受；不承诺请求墙钟30秒终止。完整账号binding与实际客户端验收仍为迁移门槛。
## Known risks
原生循环是否重置计数须实跑；商汤429原因不能仅凭状态确证；登录不等于IPC可用；401不能靠协议重试修复无效凭证。
## Definition of done
边界可复现、外部原因有证据、无扩大补丁、无正式写入；明确继续或阻塞。
## Progress
- [x] Boot sequence、044及Provider历史复核。
- [x] 显式profile与mock验证：核心17项、编译器16项通过；追加429后节流反例FAIL保留。
- [x] Provider排查：CommandCode直连官方认证/推理401 EXPIRED；WorkBuddy IPC恢复但推理400/code11128；商汤真实调用0。
- [x] 清理33个累计隔离实例的测试vault及自有进程；报告与工具说明完成。
## Decisions
- 接受原生全局等待，选择最多2次等待、单次与累计sleep上限30秒；请求执行时间另计。
- 商汤不重试试探真实风控；历史20秒冷却作为隔离候选，非官方限额保证。
- native baseCooldown配置不等于旧20秒自动恢复；接受原生提前结束，不扩patch。
- 原生limiter在429后重建导致20秒普通节流不覆盖2秒hint重试（实测2127ms）；保留FAIL，商汤调用保护未验收，不靠修改断言或关闭上游hint通过。
- CommandCode上游过期类401没有本地配置修复证据；不伪造或自动生成有效凭证。WorkBuddy不再要求重复登录，当前为上游渠道问题候选。

## Outcome
本轮隔离验证及排查完成，运行时新增patch0。继续融合候选、有界原生等待通过；正式迁移NO-GO，等待429节流回归与外部Provider恢复。见[045报告](../../research/2026-10-30-omniroute-bounded-wait-provider-validation.md)。
