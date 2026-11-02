# ExecPlan 062 — 原生工具语义与续聊验收

## Goal
仅修正验收输入，有限验证工具往返、较长历史及正常重启续聊；复用058.1展开包与059-current实例。

## Context
061强制指定工具触发DeepSeek思考模式冲突，WorkBuddy400未分类；商汤429单列。061已关闭16账号healthCheckInterval并验证跨重启，不再使用全局0。默认别名、订阅转发与GPT池仍禁用。

## Existing behavior
沿用scripts/fusion/bounded-live-acceptance.mjs、原生Combo/权限/SQLite/vault和既有连接器。DeepSeek官方thinking_mode文档要求工具对话完整回传既有assistant消息（含reasoning_content）；原生auto选择，不关思考、不改模型。

## Constraints
最多8次真实推理：DeepSeek、WorkBuddy原精确账号各4次，任一失败停止该链路，不挪预算重试/换号。商汤本轮0次，不允许其域名出站；CommandCode/Go/GPT/订阅不调用。每次max_tokens=1024、100秒超时；仅合成内容和固定内存工具，无客户端进程，无正文/推理内容落盘。

## Architecture impact
无运行时或候选包改动。062测试模式使用tool_choice=auto，原样保留服务返回的assistant消息；不构造虚假assistant思考内容。约22KB背景以新user消息追加，验证真实多轮历史。原生尝试预算临时收窄，结束恢复。

## Files expected to change
现有验收脚本增加062模式、docs/research报告、development/fusion/acceptance/062小型证据。复用runs/060-live-acceptance/extracted，不复制候选，不重试旧清理。

## Implementation stages
1. 核对官方原生语义、现有健康检查关闭及账号/模型权限；固定8次预算。
2. 两链路各做工具请求、工具结果、长上下文三步；每步失败即停止该链路。
3. 同实例正常停止/启动，核对vault/config/key/探测关闭；仅通过链路做第四次完整history续聊。
4. 原生记录按真实connection和汇总层区分对账，恢复权限/配置并停止，记录通过及阻塞。

## Tests
先仅本机启动超过35秒确认自动探测计数不变；每次请求前后检查探测计数，意外新增即中断。工具名/参数精确比较，工具输出纯内存固定值。工具往返/长历史/重启续聊同时回忆原marker和工具结果。后台run/PID变化，账号、模型、健康检查0、端口不变；vault及正文落盘检查。脚本语法检查，不重跑未修改的全量产品回归。

## Performance validation
无Router/Gateway改动，不重跑benchmark；记录请求大小、tokens、耗时与有限资源观察，不据此宣称长期稳定。

## Security considerations
保持三类停用选择及原账号范围。真实凭据、原始响应和reasoning_content仅内存；任何外部错误仅记录分类。出站只允许api.deepseek.com/www.workbuddy.ai。停止收尾时恢复loopback-only，保留原生逐账号自动探测关闭。

## Migration considerations
不新增迁入/恢复副本；会话由验收客户端重新携带完整history，不把重启验证表述为服务端持久会话能力。

## Known risks
原生auto可能不产生指定工具、响应截断、账号/上游仍拒绝；保留失败而不改网关。061仅本机探测造成的健康记录不是外部账号失效证明，不伪造健康状态去清除它。API返回的reasoning字段若未保留，记录协议路径缺口，不编造内容补齐。

## Definition of done
在预算内完成各阶段或明确失败停止；自动探测新增0、配置/vault核对、实例停止且仅本机；无新增包/产品功能，报告真实通过范围。

## Progress
- [x] 读取061结果及本地原生实现，核对DeepSeek官方thinking_mode工具回传要求。
- [x] 修正为auto选择及完整assistant回传；两链路工具名/参数通过。WorkBuddy工具结果和约21KB上下文通过；DeepSeek结果回传200但内容断言未通过，停止该链路。
- [x] WorkBuddy重启后续聊通过；实际6次真实请求（5次内容检查通过），商汤未复试，自动探测1417→1417。
- [x] 原生usage/call_logs各6条，账号/模型范围正确；配置/vault恢复、实例停止/loopback-only、候选hash不变，报告完成。

## Decisions
商汤429本轮不复试；测试工具为lookup_acceptance_record固定内存返回，无文件/命令执行。使用真实assistant返回构成多轮历史，不通过修改上游思考模式来规避输入错误。

结果：WorkBuddy限定工具/上下文/重启续聊通过；DeepSeek第二次200但严格内容断言未通过，未保存原始回复，不能据此判定协议故障。未追加重试，剩余预算作废。本轮工作完成，见[062报告](../../research/2026-11-01-native-tool-conversation-acceptance.md)。
