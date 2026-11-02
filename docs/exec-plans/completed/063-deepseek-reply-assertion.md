# ExecPlan 063 — DeepSeek回复断言诊断

## Goal
区分严格格式不符与实际字段错误，保持原验收门槛；不改变网关或为通过测试忽略语义。

## Context
062工具调用正确、结果回传200，但严格字符串断言失败。历史只留元数据，无法恢复原回复。用户允许必要的少量隔离调用，不重试换号，继续冻结功能。

## Existing behavior
复用现有bounded-live-acceptance的原账号/精确模型、单尝试预算、vault、生命周期及healthCheckInterval=0保护。原内容门槛为trim后完整字符串相等，含正确marker、receipt、value及顺序。

## Constraints
仅DeepSeek原账号deepseek-flash，最多2次请求组成一组新工具往返；第一步失败则不执行第二步，不重试、不调用商汤/WorkBuddy/GPT。复用现有包与实例，仅允许api.deepseek.com出站。沿用062相同合成输入、auto工具选择、1024输出上限和100秒超时。

## Architecture impact
运行时/候选包无变化。新增纯测试诊断器，严格pass不变；FORMAT_ONLY只是解释差异，不把原失败改成通过。仅识别分隔符边缘ASCII空白及整段单层inline/fenced code表示；三字段必须逐字、同序相等。其他结构不猜测，字段错误/多余文本不当成格式差异。

## Files expected to change
scripts/fusion现有验收入口063模式与纯诊断函数；tests/fusion离线反例；development/fusion/acceptance/063小证据及docs/research结果。无解包、构建、候选副本或旧清理重试。

## Implementation stages
1. 固定严格判据与独立诊断分类，离线验证错误值、顺序、缺项、多余文本等反例。
2. 核对自动探测关闭并跨过本机启动窗口；最多2次实际调用，仅在内存检查回复。
3. 保存字段相等布尔值、格式类型、长度及结果分类；核对原生次数/范围，恢复配置并停止。

## Tests
离线测试必须覆盖纯格式变体不会通过原strictPass，错误值/顺序/字段数量不获语义认可。真实工具名/参数仍精确校验；原生connection-test计数不变。结束核对原配置/vault、模型/账号范围、正文落盘0、唯一候选hash。历史062失败不因新样本而追溯改判。

## Performance validation
没有Router/Gateway变更；不跑benchmark或重复长时间验收。仅记录2次以内请求耗时和usage。

## Security considerations
不保存原始回复、reasoning_content或凭据；诊断仅布尔值/枚举/计数，不使用包含真实回复的assert错误。只读原隔离vault，默认/订阅/GPT停用保持；finally恢复并停止，网络回loopback-only。

## Migration considerations
无迁移、数据副本或新账号；逐账号健康检查0保持。当前实例及原展开包继续复用。

## Known risks
新样本不一定复现旧回复，因此只能判定本次观察，不能猜旧错误。未识别结构标为无法确认；不能用找到预期词片段、数值转换或字段重排作为通过条件。

## Definition of done
给出可靠分类证据和历史边界；最多2次调用、无重试/换号，原strictPass不放宽，原生保护和收尾核对完成。

## Progress
- [x] 核对062严格断言与历史脱敏记录；确认历史原回复不可恢复。
- [x] 22项离线用例通过：原strictPass不变，错字段/顺序/数字/额外文本不获语义认可；脚本语法通过。
- [x] 一组2次真实请求全部200/原严格判据通过；工具名/参数正确，回复EXACT且3字段正确，本轮未复现062差异。
- [x] 自动探测1417→1417；原生记录2条、账号/模型正确，配置/vault恢复，实例停止/loopback-only、包进程0、唯一候选hash不变，报告完成。

## Decisions
沿用062相同合成内容，phase063只用于证据隔离；不通过改变提示词去追求想要的回复。格式正确性和内容正确性分开报告，合并验收仍要求原严格判据。

结果：当前新样本严格通过，但062缺少原文，仍保留UNKNOWN，不追溯改判、不追加重复调用。见[063报告](../../research/2026-11-01-deepseek-reply-assertion.md)。仅测试诊断变化，无产品补丁；本轮范围已完成。
