# ExecPlan 047：真实客户端与日常可靠性验证

## Goal
沿窄适配路线验证WorkBuddy真实Codex/Claude Code/OpenCode、工具、长会话、凭证轮换；用046修复后的限流包做少量商汤真实调用。给出日常可靠性范围，不为通过测试扩大patch。
## Context
046完整包23/23、相关单测56/56；WorkBuddy窄连接合成文本成功。用户明确授权继续隔离live，禁止UI与正式环境修改。
## Existing behavior
唯一后台为046候选，原生协议转换/工具映射不改。旧bridge主动拒绝工具，046凭证读取是一次性的；使用上游内存key provider和header函数作为既有abstraction。
## Constraints
不改正式数据、配置、凭据、进程或客户端HOME。CommandCode零调用。不自动刷新WorkBuddy凭证、不改提示词、不换模型。最多16次WorkBuddy推理、最多3次商汤推理；任何鉴权/渠道/风控拒绝停止对应路径。商汤只选一个原账号，固定deepseek-v4-flash，20秒minTime、1并发、3rpm；外部拒绝后出口关闭，不轮询整个号池。
## Architecture impact
实验目录窄relay只鉴权、固定账号/模型、限量和透传；协议/工具处理交给OmniRoute。凭证reader复用上游解密器，缓存key helper，仅从本机加密文件读取变化，不写回。原后台patch零新增。
## Files expected to change
development/experiments/omniroute-phase8/脚本与回执，ADR019试验授权，参考/Provider文档及最终报告。
## Implementation stages
1. 本机客户端/参考能力与隔离边界复核。
2. 凭证轮换和薄relay离线验证。
3. 实际客户端文本/只读工具、合成长上下文/多轮验证。
4. 单账号小预算商汤live；恢复、清理和日常可靠性结论。
## Tests
mock凭证同账号换token、账号变化、过期、损坏文件、key helper复用；relay鉴权/模型/限额/取消/错误脱敏。客户端工具只读临时acceptance.txt；长会话完整历史、原始模型和合成marker。live opt-in明确；上游调用数量硬限制。
## Performance validation
记录响应头/总耗时、调用数、账号/模型边界、商汤实际间隔。缓冲SSE不冒充真实TTFT；商汤首请求429，无新增真实间隔样本。沿用046后台不扩大性能测试。
## Security considerations
正文仅合成，凭证明文仅内存；客户端独立HOME、无真实MCP。输出仅类别/状态/计数/marker。只读官方auth和正式metadata快照；不迫使真实token轮换。
## Migration considerations
不替换冻结锁或正式安装，047只用046候选；失败保留、不装DSH整包、不复刻第三方协议栈。
## Known risks
渠道检查可能拒绝实际客户端system；工具或长会话可能不兼容。真实token自然轮换时间不可控；mock通过不算真实轮换通过。有限短时测试不能证明长期稳定性。
## Definition of done
四类WorkBuddy范围都有实测/明确阻塞和证据；商汤有限live有结果；无新增后台patch、临时凭据/进程清理；给出是否能日常使用的明确分项结论。
## Progress
- [x] Boot sequence、046与现有client/bridge能力复核。
- [x] 离线凭证/relay 12项与真实临时加密文件轮换5项通过。
- [x] WorkBuddy实际Claude/OpenCode文本/只读工具、3轮149KB history通过；Codex上游渠道拒绝，离线同形请求可成功。
- [x] 商汤真实一次429后停止；无换号/换模型/新增后台patch。清理与报告已完成，完整候选manifest及源码patch不变。
## Decisions
- 不使用旧bridge的文本限制作为上游工具不支持的结论；仅在实验目录换薄relay，协议由原生后台处理。
- 真实凭证轮换仅观察自然发生；不调用refresh、不改正式登录。未发生则明确UNKNOWN。
- Codex首次本地失败来自stream=false，按指定项目既有语义仅强制WorkBuddy上游SSE；随后仍被上游拒绝。纯mock确认角色已经system、工具为function，无需额外角色映射。不增加提示词/指纹改写。

## Outcome

窄适配仍有保留价值，但尚不能作为完整日常主网关替换。真实WorkBuddy11次（9成功、2渠道拒绝），商汤1次429；真实token轮换NOT_OBSERVED。具体通过范围和保留阻塞见[047报告](../../research/2026-10-30-daily-client-credential-provider-validation.md)。
