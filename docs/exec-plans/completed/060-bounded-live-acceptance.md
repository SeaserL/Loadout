# ExecPlan 060 — 当前隔离实例的有限真实验收

## Goal
对059已迁入的真实账号与058.1唯一候选包进行有限真实使用验收，确认既有网关路径，不增加功能或扩大账号范围。

## Context
用户要求默认别名、订阅转发及完整GPT池继续禁用；账号故障单列，不调试客户端，不改正式环境，不提交上传。当前隔离实例059-current已停止，出站仅loopback。

## Existing behavior
沿用Instance生命周期、原生Combo/key管理API、已有Credential Manager引用与WorkBuddy连接器。商汤已设置20秒最小间隔、并发1、每分钟3次与有界等待。058.1已完成集中隔离回归，059已完成真实绑定及备份恢复。

## Constraints
最多3个串行短文本请求：SenseNova deepseek-v4-flash、DeepSeek既有精确别名、WorkBuddy既有精确别名各1次。输出上限256 tokens，每次超时100秒。任何失败不重试、不换模型、不换账号。仅目标隔离vault读取，不复制正式凭据、不登录。CommandCode/OpenCodeGo未获得启用GPT池授权，不调用。

## Architecture impact
无运行时源码、补丁、UI或新候选包变化。验收期间仅通过已有管理API将所测Combo尝试上限收紧到1；临时测试key仅允许已有商汤Combo的首个既有账号。保留原账号列表、模型、节流及冷却记录，结束恢复配置和移除临时key。

## Files expected to change
scripts/fusion/本轮验收编排；development/fusion/acceptance/060脱敏回执；docs/research/验收结果及状态索引。本轮展开包仅存在于新建060临时目录。

## Implementation stages
1. 固定候选hash、已迁入映射、停用项和节流；展开唯一候选。
2. 保存隔离配置、核对原生单次尝试约束；仅允许SenseNova/DeepSeek/WorkBuddy三个既有目标域名。
3. 串行调用，每路最多一次；记录HTTP状态、内容存在、模型、耗时及统计，不记录原始响应、正文或凭据。
4. 恢复配置、移除临时key、停实例和收回出站；核对权限与vault不变；清理本轮展开包并报告未证实项。

## Tests
LOADOUT_LIVE_TESTS=1显式opt-in。真实推理上限3；管理读取不算推理调用。读取禁用别名、完整账号范围及精确模型；逐次记录结果，不把HTTP200当成跨协议、工具调用、长会话或全账号稳定性证明。

## Performance validation
无Router/Gateway代码变更，不重跑benchmark或批量测速；只记录本次短请求耗时。

## Security considerations
凭据只留在既有测试vault及内存；临时key不落盘。结果仅输出结构化摘要。配置恢复与停止在finally执行，失败则保留现场并明确报告；不重试058已被自动审批拒绝的清理。

## Migration considerations
沿用059-current，不重做迁移。默认别名、订阅转发、GPT池保持禁用。正式数据/已安装程序只读核对，不写入。

## Known risks
真实上游可能429/鉴权失败/模型不支持；保留为外部阻塞。三个请求只能证明有限路径，不能证明日常长时稳定。为限制次数临时收紧重试，因此本轮不证明原生failover表现；已有隔离回归提供其证据。

## Definition of done
有限调用完成或明确阻塞；配置、权限和vault恢复/不变；实例停止且仅loopback；单候选hash不变、本轮展开包清理；报告清楚区分真实通过与未验项。

## Progress
- [x] 核对059范围、用户停用决定与已有控制接口。
- [x] 固定058.1候选，3次真实短文本请求全部200/OK；原生usage及call_logs各新增3条，账号/模型符合限定范围。
- [x] 原生配置/key权限恢复、vault逐值不变、实例停止并恢复loopback-only；正文落盘0，包进程0；结果文档完成。
- [x] 原清理拒绝保留历史记录；2026-11-01用户再次授权后，064完成058→064升级及原059隔离数据恢复，已清理本轮1.10GiB展开目录。当前仅一个064候选ZIP。

## Decisions and result
验收主体完成，无新增运行时补丁；3次成功不外推为全账号或长会话稳定。商汤历史429风险、CommandCode未启用池内账号、WorkBuddy自然轮换分别记录。详见[060报告](../../research/2026-11-01-bounded-live-acceptance.md)。最后清理项已在064完成，本计划关闭，不扩展功能或追加真实调用。
