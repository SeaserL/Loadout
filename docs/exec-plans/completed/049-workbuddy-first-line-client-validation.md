# ExecPlan 049：WorkBuddy 首行兼容与真实 Codex 验证

## Goal
实现048已验证的最小首行适配，隔离验证真实Codex文本、工具与长会话；证明范围外指令、用户消息和工具参数不变。
## Context
用户明确授权首行适配，不扩大替换范围、不做UI、不动正式环境。048首行应用固定OmniRoute规则后完整请求重放成功，但实际客户端闭环尚待验证。
## Existing behavior
复用phase8薄relay、内存凭证reader和phase4完整候选Instance/客户端隔离抽象。OmniRoute负责原生协议转换；自定义Provider没有启用原生全body替换。048使用的精确客户端身份行已经固定。
## Constraints
只改实验连接器并显式开启；仅第一条system的第一个文本内容的已知首行。未知首行保持不变，无模糊匹配、不扫描后续system、不改用户或工具。固定账号和deepseek-v4.1-flash。live显式opt-in，最多16次实际上游请求、单并发、65秒、2048输出token；某用例上游拒绝关闭该用例，401/403/429关闭全出口；不扩大预算以求通过。商汤/CommandCode零调用。
## Architecture impact
一个Provider内纯函数和relay显式开关；后台/路由/SQLite/凭证存储零变更。默认关闭，已有phase8行为不变。适配位置为原生转换之后、WorkBuddy发送之前。
## Files expected to change
phase10适配、测试、真实客户端harness和回执；phase8 relay的显式调用点；ADR019授权、来源notice、验证报告与本计划。
## Implementation stages
1. 精确首行函数与逐字不变回归。
2. loopback relay实走、性能成本、默认关闭/非目标输入回归。
3. 完整候选加隔离Codex文本、只读文件工具、多轮长会话。
4. 包/秘密/正式配置及清理审计，记录失败和实际维护面。
## Tests
字符串/text数组、CRLF/LF、无换行、未知/重复/后续身份句、非system/非text、多模态、用户路径、工具schema/arguments/result、开关关闭、幂等与不修改输入。实际工具必须有真实工具调用及结果回传，不能仅凭marker判定。长会话要求同一客户端会话多轮保留早期marker，记录真实累积字节与消息数。
## Performance validation
适配纯函数在小/150KB输入上benchmark；relay loopback对照。未改Router/Gateway后台，不重跑Rust路由benchmark；记录实际客户端耗时，不冒充TTFT。
## Security considerations
官方auth/binding只读，access token只在内存，无refresh；请求正文不写实验日志。真实客户端独立HOME/workspace；若会话需要本地保存，只含合成内容及临时client key，不能保存Provider凭据。结果仅元数据、哈希、合成marker与计数。
## Migration considerations
不替换冻结包或正式bridge；适配显式开关可关闭。不将隔离成功视为生产迁移批准。
## Known risks
工具/长会话可能暴露独立上游限制；客户端版本变化可能不再匹配精确首行。任何失败记录BLOCKED/FAIL，不扩张匹配、删工具或改其他消息。
## Definition of done
实现限定适配且范围外不变测试通过；三个真实场景有结果或明确阻塞；清理审计完成，输出继续/停止的范围化结论。
## Progress
- [x] Boot sequence及048来源/代码/回执复核。
- [x] 精确首行函数、默认关闭的relay入口；21项范围/原生等价/loopback检查及原有12项凭证relay回归通过，纯函数benchmark已记录。
- [x] 真实Codex文本、持久会话get_goal、同session三轮长会话通过；文件读取明确BLOCKED于本地执行策略，mock复现同hash错误。未扩大替换或放宽权限。
- [x] 12次真实推理；1224文件当前token扫描0命中，21732候选文件校验通过，52历史实例凭据/后端PID零残留；报告与来源文档完成。
## Decisions
- 首行按048观察到的完整句子精确匹配，替换结果固定为048原生规则结果，不运行全body递归替换。
- 初轮6次上游请求：文本及同一session ID三轮长会话通过（最大204686 B）；文件工具失败。追加2次真实诊断确认上游exec_command名称/参数正确；本地mock复现同hash工具错误，定位为隔离Codex执行策略blocked by policy。未放宽sandbox或重写工具参数。
- 在总16次预算内补测只读get_goal（最多4次），区分工具协议/分派能力与被禁止的本地命令执行；不把它当作文件读取通过。
- get_goal首轮ephemeral返回需要持久会话，修正过宽的验收判定并保留FAIL回执；新增负例，持久测试会话重跑后收到真实goal=null结构，PASS。新增测试不改变首行适配或命令权限。

## Outcome

保留27个非空行的首行纯函数和relay显式开关，继续窄融合；真实文本/状态工具/长会话已获得证据，文件工具因本机策略仍BLOCKED，不宣称全部日常工具可用、不切换正式环境。详见[049报告](../../research/2026-10-30-workbuddy-first-line-client-validation.md)。
