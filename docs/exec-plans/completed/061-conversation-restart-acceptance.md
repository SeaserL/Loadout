# ExecPlan 061 — 有界会话、工具及重启验收

## Goal
沿用058.1现有展开包与059-current隔离实例，验证较长历史、工具往返和正常停止重启后的恢复；不扩展产品功能。

## Context
060三次真实短文本通过。用户继续授权有限持续使用验收，不新增候选副本、客户端调试、UI或正式变更。默认别名、订阅转发和完整GPT池保持禁用。

## Existing behavior
复用060验收脚本的Instance/native API/vault与单次尝试约束，直接调用Chat Completions。无状态会话由客户端重新携带完整历史；不声称网关持久化聊天。既有工具协议由OmniRoute处理，WorkBuddy仅沿用当前relay。

## Constraints
真实请求总上限9次，每链路最多3次：长历史要求工具调用、固定本地工具结果回传、正常停止重启后完整历史续聊。仅SenseNova account-1/deepseek-v4-flash、DeepSeek与WorkBuddy原精确账号/模型。每次max_tokens=512、100秒超时；商汤串行并至少25秒间隔。任一语义或HTTP失败停止该链路，不重试、换账号或改模型。只发送约20KB合成history，工具为纯内存固定值，不执行模型提出的其他操作。

## Architecture impact
无运行时补丁。延用060原生临时收紧尝试预算、商汤单账号key与三域名出站配置；完成后全部恢复。只扩展已有验收编排，默认060路径仍拒绝重复执行。061独立回执禁止重复消耗额度。

## Files expected to change
scripts/fusion/bounded-live-acceptance.mjs（加入061模式）；development/fusion/acceptance/061小型脱敏回执；docs/research/报告与状态索引。直接使用runs/060-live-acceptance/extracted，不复制/生成包，不重试被自动审批拒绝的清理。

## Implementation stages
1. 核对现存包hash、账号/模型/节流与停用项，固定9次预算。
2. 对三条链路分别执行长历史工具请求及工具结果；错误即停止该链路。
3. 前半段至少观察3分钟（只做本机状态检查），正常停止重启，核对端口、配置、vault和原key恢复，再做成功链路续聊。
4. 读原生usage/call_logs对账；恢复原配置并停止，记录数据增长、明文凭据/正文检查，保留一个候选ZIP和原展开目录。

## Tests
LOADOUT_LIVE_TESTS=1；工具名/参数逐值比较，工具返回后与重启后文本须同时包含历史开头marker和工具结果。配置/vault跨重启相等、后台PID/run变化、仍同instance id和端口。原生记录限定账号/模型及次数；正文日志保持关闭。未测试的客户端/协议/stateful Responses/自然轮换明确不计为通过。

## Performance validation
不改Router/Gateway，故不追加benchmark。记录有限调用耗时、请求字节和usage、进程工作集/数据文件增长；几分钟窗口不能证明长期资源稳定。

## Security considerations
复用既有测试vault，原始响应和tool arguments只在内存校验；只存布尔结果、数值及脱敏摘要。未开启订阅登录/刷新；不读写正式key或DB。finally恢复原生配置/key后停止和恢复loopback-only；失败留证，不为验收打补丁。

## Migration considerations
不重新迁移或恢复数据副本。正常重启恢复的是既有隔离SQLite配置、OS vault和客户端key；会话历史由测试客户端内存提供，无网关存储会话的新增承诺。

## Known risks
模型可能不调用指定工具、截断输出、返回429/鉴权拒绝；这些均记录而不扩大调试。较长合成历史不等于真实长期会话或上下文极限。当前补丁前向升级维护成本仍按原计划单独验收。

## Definition of done
在<=9次真实请求内给出每条链路结果与重启证据；原配置/vault恢复、实例停止、无新候选副本；外部问题/未验范围单列，报告完成。

## Progress
- [x] 读取060证据、原生配置与既有会话/连接器抽象；确定9次预算和25秒商汤间隔。
- [x] 执行3次长历史工具请求：商汤429、DeepSeek思考/工具选择冲突400、WorkBuddy未分类400；按停止规则跳过其余6次，工具结果/真实续聊未通过。
- [x] 发现30秒后16项自动连接检查，立即停止；仅本机恢复，原生全局0无效且快速重复，撤回后改为逐账号healthCheckInterval=0。
- [x] 逐账号关闭跨停止重启核对通过，前后各超过35秒无新增检查；配置/vault保持、实例停止及loopback-only。运行时补丁0、候选新增0，结果/外部项完整记录。

## Decisions
仅延用Chat Completions标准协议探针，不启动或修复客户端；不因某链路失败把预算挪给其他账号。无论剩余次数多少，都不追加失败重试。

本轮验收结果为部分通过；工具与长会话未通过，不能以本机重启通过替代。因自动探测改变后续计划：只在loopback-only下验证原生关闭配置，最终保留16个账号healthCheckInterval=0，其余授权配置恢复。已无继续执行的本轮步骤，后续真实重测另定有界预算。详见[061报告](../../research/2026-11-01-conversation-restart-acceptance.md)。
