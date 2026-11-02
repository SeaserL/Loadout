# 旧池、商汤端点与重试映射验证（044）

**旧池顺序、账号范围和“立即换号而非先重试”已用原生配置验证；商汤旧30秒等待仍不能等价迁移。本轮新增运行时补丁为0，未做UI、未动正式环境，也没有再次调用真实Provider。**

新增离线迁移编译器与CLI，继续使用043固定v3.8.52完整后台目录。读源数据库只取路由metadata，不取真实key或OAuth。

## 已补齐的映射

| 来源 | 原生配置 | 验证 |
|---|---|---|
| `gpt`手工池 | priority combo，三个显式connection步骤 | 保留codex-sub/gpt-6-astra → CommandCode/gpt-6-luna → opencode-go/gpt-6-luna；用三个独立合成Provider逐段失败后成功，顺序和模型一致 |
| `workbuddy-flash` | 单步骤priority combo | 保留WorkBuddy/deepseek-v4.1-flash；合成上游通过，真实会话可用性不在此PASS内 |
| 商汤同模型全账号链 | 每个模型一个combo，当前12账号为12步，专用key限制账号范围 | 11个账号429，第12个成功；每号只调用一次，未受默认3次上限截断 |
| 重试规则 | `failoverBeforeRetry=true`、`maxRetries=0`、`maxSetRetries=0`、`maxGlobalAttempts=成员数` | Chat、Responses、Messages均按a→b→c切换；没有上一轮观察到的同URL额外两次429重试 |
| 访问范围 | key的allowedConnections + restricted模型权限 + allowedCombos | 子集只到被允许账号；普通模型名不能绕过combo，直接请求被拒绝且无上游调用 |
| 恢复 | 原生combo/权限在SQLite，client key在OS vault，沿用加密备份 | 新目录、新vault恢复后读取恢复的key，请求仍命中首选模型 |

已有Pool本身是用户显式定义的跨模型链，所以`gpt`的astra→luna被保留；商汤每个combo只有一个上游模型，不增加隐式换模型。

接口改为`/v1`及`pool-gpt`、`pool-workbuddy-flash`、`endpoint-sensenova-1`模型名，使用各自key。没有增加兼容旧URL的代理层。当前商汤模型清单只编译实际使用的`deepseek-v4-flash`；其他模型需显式加入清单。账号增删/排序变化后需要重新编译，`members=null`未被伪装为动态同步功能。

## 等待：两个已复现的不等价点

旧Loadout `crates/gateway/src/routes.rs`在请求开始时计算deadline；最早恢复时间超过剩余窗口时立即返回。这个等待只属于专用端点。

OmniRoute原生`comboCooldownWait`能够正确做到短冷却后重试、长冷却拒绝、401不等待、取消后不再发请求。**但不能直接替代旧规则：**

1. **范围不同。** `open-sse/services/comboConfig.ts::isComboCooldownWaitEligible`对全部combo策略返回全局enabled。反例中零等待普通池第一次429后约2.12秒重试成功；旧行为应立即失败。把该开关打开会改变其他池的行为。
2. **截止不同。** 原生budget是累计等待时间，不包含已花费的上游处理时间。模拟第一次请求耗时2秒，再返回Retry-After=29秒；即使配置`comboTimeoutMs=30000`，实测仍在31.14秒后发起第二次上游并成功。旧端点应判定恢复时间超出请求起点30秒窗口，不再等待。

前一轮假设用Retry-After=30秒也能复现第二点，实际没有复现：原生额外margin使其超过单次等待上限，因此立即返回。该失败假设记录保留；最终29秒+已耗时2秒反例才是报告结论依据。

因此迁移编译器给非零waitSecs端点标记`BLOCKED`，apply会拒绝，不改成零等待、不偷偷把等待推广到全部池，也不通过增加自有路由循环解决。池本身可以表达为READY，但正式依赖未绑定时仍不允许激活。

**建议保留OmniRoute路线，等待行为作为独立取舍项。** 可等待上游提供combo级等待与统一请求deadline，或另行决定接受原生全局等待语义；本轮没有替用户作这种行为变更，也没有扩大私有补丁。

## 迁移工具的保护

- 实际预览保留完整旧池和12账号需求。缺失binding会列出具体逻辑账号，不能自动删掉首选codex-sub后把剩余池标为通过。
- 保存源metadata/账号身份指纹及binding指纹。apply重新读取源并与已审阅plan核对；账户身份、顺序或模型改变要求重新预览。
- 未知client policy、账号集合、未验证模型、默认模型回退、非manual顺序、未验证round-robin combo都拒绝或阻塞，不默认无限制。
- 显式选择READY route后才写原生管理API；重复应用拒绝。合成“第二个key创建失败”反例确认本次已创建combo/key被撤销。
- 新client key只写隔离OS vault，普通plan/结果文件不包含它。迁移是多次管理API调用，未承诺进程强杀下跨API原子性；不能作为正式安装切换工具。

## 隔离验收与维护成本

完整后台验收最终26项检查通过（含清理和两个“成功复现不等价”的反例）；另有12项编译器边界检查通过。**检查通过不表示30秒旧等待通过。**

12次正常请求测得中位32.89ms、P95 53.24ms；三账号failover约81–97ms；12账号场景约504ms。均为本机loopback mock、原生队列间隔明确设0，不是外部Provider延迟或生产SLA。没有修改Router/Gateway代码，因此未重新跑Rust benchmark。

运行时保持043的432行适配、14处上游hook；不新增schema、不重构SQLite、不构建第二路由器。本轮维护资产是离线路由快照、编译器、原生配置验收和操作说明。源码/包锁前后相同。

## 需要用户处理的外部事项

以下来自043的真实观察，本轮未重复探测、未重新登录、未消耗额度。它们与本轮等待语义差异分开处理。

| 对象 | 已观察到什么 | 请处理什么 | 恢复后怎样验收 |
|---|---|---|---|
| 商汤 | 所选账号、deepseek-v4-flash返回429；尚不确定是RPM/TPM、额度还是其他限制 | 在该账号控制台确认模型可用性、剩余额度、限流类型与重置时间；保留脱敏错误码/状态即可，不需要先充值或轮流探测12个账号 | 选定一个已确认可用账号做一次受控调用，再测路由；不通过换号/换模型绕过限制 |
| CommandCode | 唯一账号向Provider API请求返回401 | 在官方控制台确认当前API key是否有效、到期/撤销、是否具有Provider推理权限；如需替换，用本机正常凭证管理入口完成，勿把key发到聊天 | 一次受控认证及精确模型调用；失败停止 |
| WorkBuddy | 官方IPC locator不存在，独立桥接返回503 | 在方便时打开官方WorkBuddy桌面应用并完成正常登录，确认官方会话可用；让应用自行生成IPC状态，不手造endpoint/ticket文件 | 先只读IPC握手，再一次文本请求；现有桥接不支持tools，登录恢复不会自动补齐这个能力 |

另：`gpt`池中的codex-sub和opencode-go仅在本轮做了合成绑定；未因此获得真实Transport验收。CommandCode恢复不等于整个gpt池真实链路全部通过。

## 证据

- 最终完整后台回执
- 编译器检查
- 历史等待假设未复现记录
- 清理与源配置核验

正式环境保持原样，实验服务和OS凭据已清理；脚手架继续保留。没有UI、commit或push。
