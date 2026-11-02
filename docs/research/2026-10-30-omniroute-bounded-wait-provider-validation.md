# OmniRoute 原生等待与 Provider 隔离验证（045）

日期：2026-10-30。结论：**继续采用OmniRoute原生SQLite与最小适配候选；有界等待/账号/模型契约通过本轮核心验证，但商汤429后的节流、CommandCode认证和WorkBuddy渠道仍未通过，不切正式环境。** 本轮运行时patch增加0，不退回全量自研。

## 新验收前提

用户接受原生全局combo等待与累计sleep预算，不再复刻旧端点请求起点30秒deadline。旧反例仍成立，现为已接受的语义差异。实现只改离线映射、隔离配置和测试工具；冻结底座仍为v3.8.52 / ad6f33f66bda34eba98645d3bfdabbde294bc843，自有adapter432行、14个上游接缝保持不变。本轮没有再做跨版本升级，沿用042/043证据，不能推断未来版本零成本。

phase5新增显式`--native-wait`预览与计划字段。旧计划不会自动解除等待阻塞；新计划的配置、源与binding指纹必须一致。缺账号绑定、未知模型、未知策略继续拒绝。apply不改目标全局规则；先在隔离实例显式configure，再读回核对。

| 配置 | 候选值及边界 |
|---|---|
| cooldown wait | 单次≤30秒、累计sleep≤30秒、最多2次等待 |
| target / set retry | 0 / 0，failoverBeforeRetry=true，无hedging |
| attempts | min(200,3×显式成员数)；单账号实测最多3次，不是全池最多3次 |
| 排队 | maxWaitMs=30000、maxQueueDepth=4 |
| target / execution backstop | 90000ms；配置读回通过，未另做90秒长流计时验收 |
| scope / model | 显式connection与key scope交集；端点仅同模型；旧Pool显式跨模型依旧清单 |

这些是不同层级的预算，不能相加后宣传成完整请求的硬deadline。未知冷却原因、过长Retry-After或认证错误可立即终止；不保证一定等待或一定用满次数。原生全局设置影响普通Pool，这一语义已接受。

## 隔离结果与反例

固定完整后台包、独立HOME/数据/端口、假凭证、loopback出口保护。核心运行17/17（含两项cleanup），编译器16/16；追加节流反例失败，不能说全套通过。

- Chat / Responses / Messages：2秒冷却后恢复，实际两次同账号同模型调用。
- 持续429：单账号三次dispatch、两次等待，约4.19秒停止。
- Retry-After16秒持续失败：约16.13秒后第二次失败即结束，剩余预算不足再等16秒。
- Retry-After60秒、401：一次dispatch后立即终止。
- scope只含首个账号时，健康的第二账号始终不可达；直接模型旁路拒绝；等待/取消后不越界、不补发。
- 改计划或全局profile不匹配时拒绝；加密备份新目录恢复后scope、key及等待/节流设置保留。
- 非节流正常请求12次，中位80.58ms，P95样本96.18ms，仅mock端到端观测；不代表真实TTFT或纯router开销。

第一次“apikey baseCooldownMs=20000即可让无正Retry-After请求在20秒后恢复”的假设失败：收到429后约72ms返回，不自动重试。此结果保留，最终只接受原生可能提前结束的行为，不为复刻20秒恢复加补丁。

## 商汤：有历史成功，仍须修正节流验收

用户确认额度与账号正常。仓库2026-10-23历史记录也有成功推理：同key约2秒间隔的前三次成功，随后`inference exceeds tpm/rpm limit`，约20秒恢复；其他账号当时未受影响。它支持短窗口限流处理，不证明任何固定参数能消除未来风控。本轮商汤真实请求0，没有批量扫描12账号或试探阈值。

通过原生connection的`rateLimitOverrides`配置：间隔20000ms、并发1、RPM3、排队预算30000ms、执行backstop90000ms。正常连续成功调用间隔实测19997ms（调度/计时误差），设置可备份恢复。

**追加反例未通过：429 + Retry-After2秒之后，下一次同key请求仅隔2127ms。** 冻结源码`open-sse/services/rateLimitManager.ts`的429分支调用`evictLimiter`，重建limiter丢失之前的时间基准；因此普通请求限速通过，不代表429恢复路径也保持20秒。已有次数上限阻止无限重试，但仍可能在风控窗口内重复同key。

这属于需要上游修复/回归的具体点，不是账号坏了或必须重写路由的证据。候选configure明确返回`BLOCKED_429_RESETS_PACING_HISTORY`；本轮不通过关闭长Retry-After、修改模型、扩账号范围或加私有循环掩盖。商汤真实迁移验收暂不解除。

## CommandCode：401已定位至上游认证

复核[官方Provider文档](https://commandcode.ai/docs/provider)：origin与Bearer方式仍与现有实现一致；本机未配置覆盖URL。只读选定现有OS vault凭证，在内存检查无外层空白、控制字符、重复Bearer或JSON包装；未发现CMD_API_KEY环境候选或独立CommandCode CLI安装。

直接官方`/alpha/billing/credits`一次读取返回401，错误内容归类EXPIRED；绕过OmniRoute、直接`/provider/v1/chat/completions`固定deepseek/deepseek-v4.1-flash（16token上限）一次仍401/EXPIRED。无重试、无其他模型探测。

因此不能通过修改OmniRoute协议或URL修好；服务端拒绝当前认证。错误类别不足以区分密钥本身过期、账号订阅状态或服务端会话失效，不能猜测。需要在官方Studio检查API资格/密钥状态，必要时重新签发，再经原有安全录入流程处理。不要把key发到聊天里。本轮未替换正式凭证。

## WorkBuddy：登录已生效，推理渠道仍拒绝

重新检查后，官方Named Pipe握手及wb.request/http.fetch可用，之前的IPC缺失阻塞已解除。目录接口仍400；历史记录已有目录400/code12403限制，目录失败不能反推未登录。

隔离OmniRoute→独立bridge→官方客户端IPC，两次各一次固定模型文本请求：上游均400；第二次记录数值code11128，外层bridge映射502。第二次使用历史成功形状（system+user、256token），仍失败。未改变正式bridge、未重启桌面、未抽取登录token。

同一上游代码的[直接调用问题记录](https://github.com/decolua/9router/issues/5005)显示渠道校验拒绝；结合本机code11128，推测本次为同类渠道限制。该归因不是本机保存的完整错误正文证据，仍需区分推测与已观测状态；不能把桌面已登录等同于任意IPC推理可用。没有照搬第三方伪装头或绕过校验，也不再要求用户重复登录。现桥接仍仅文本、buffered SSE，不能宣称coding工具链已通过。

## 边界与后续判断

源路由metadata与044一致；冻结adapter字节一致；测试凭据与自有后台全部清理。没有UI、正式服务重启/配置写入、commit或push。完整依赖树hash仍NOT_COMPUTED。

本轮把“原生等待是否可接受”从语义阻塞变成了通过的有界配置；维护增量仍在运行时外。剩下的工作应聚焦原生429 limiter重建的单点回归和两条外部Provider恢复，不应恢复旧RouteSnapshot或扩大成第二套路由器。正式迁移仍为NO-GO，继续隔离融合为GO。

工具与可复验命令：phase6 README。证据以该目录evidence/index.json为索引，包含失败记录；旧044报告保留为历史契约结论。
