# ExecPlan 044: 旧池、端点等待与重试的原生迁移映射

## Goal
补齐旧Pool、商汤专用端点的迁移描述与原生配置编译，隔离验证顺序、范围、同模型、等待期限及取消；列明外部阻塞待办。不扩大043运行时补丁。

## Context
043固定v3.8.52，官方客户端接线与备份恢复已验证；实际商汤429、CommandCode401和WorkBuddy IPC不可用单独处理。本轮只用loopback mock和假凭证，不再读取真实key或调用Provider。

## Existing behavior
旧gpt池依次为codex-sub/gpt-6-astra、CommandCode/gpt-6-luna、opencode-go/gpt-6-luna；workbuddy-flash为单路由。商汤端点全成员、30秒等待、逐账号立即failover。旧30秒是从请求开始算的deadline，不是每轮再等30秒。

## Constraints
不做UI、不动正式数据/进程/配置、不复制外部登录、不commit/push。冻结043包与432行适配；缺失原生能力给出阻塞结果，不能默改语义或另造路由器。只读源metadata可用于映射。

## Architecture impact
增加离线路由清单/编译器及隔离验收，原生combo、connection范围和resilience设置优先。不存在依赖时保留完整顺序并阻止激活，不静默删掉首选账号。

## Files expected to change
development/experiments/omniroute-phase5/；044报告、reference-map、ADR-019试验结果。复用phase4实例与备份工具，不修改冻结上游。

## Implementation stages
1. 复核旧实现、原生combo/等待配置、实际metadata；冻结输入指纹。
2. 建立映射编译器及安全预览/应用，原生管理API落地。
3. 合成协议测试、等待/取消/认证失败/范围反例；恢复后复验，记录无法等价项。
4. 清理并出维护成本结论和外部待办。

## Tests
有序跨模型显式池、缺成员拒绝、相同模型换号、无隐式换模型、scope不可逃逸、单账号不重复协议尝试；短冷却恢复、超过30秒拒绝、累计deadline、零等待、401不等待、取消不重放；备份恢复和清理。

## Performance validation
不修改Router/Gateway；配置影响通过mock正常请求及429 failover时延测量，不消耗真实额度。

## Security considerations
只读源配置、无secret_ref/账号ID/邮箱写入fixture；逻辑account-N标识，未知策略拒绝。所有凭据为合成值，测试service随机且最后删除。

## Migration considerations
接口可改为原生combo名及限定client key，但账号顺序、范围和显式跨模型必须保留。现有gpt的订阅链路未被本轮授权导入；用占位依赖表达并阻止激活。全Provider成员需重新编译同步，不能把静态快照伪装成自动更新。

## Known risks
原生resilience等待设置可能是全局而不是端点级；累计wait预算不一定等于墙钟deadline；native combo提交点/协议转换行为需实跑。不得以配置保存成功证明语义等价。

## Definition of done
映射可执行且有拒绝路径；各项PASS/FAIL/BLOCKED明确；运行时补丁未扩大；外部用户待办明确；临时凭据/进程清理。

## Progress
- [x] Boot sequence，043证据、旧端点实现及原生配置入口复核。
- [x] 映射编译器/CLI和只读预览：账号身份/顺序指纹、完整成员保留、缺失binding阻止激活。
- [x] 完整后台26项检查通过（含两个不等价反例）；12项编译器检查通过；恢复后路由/OS vault key通过；12次正常请求中位32.89ms。
- [x] 真实推理调用0，新增运行时patch 0；清理回执、报告与三类外部用户待办完成。

## Decisions
- 本轮不进行任何真实推理，不要求用户先解决外部阻塞才能继续本地验证。
- 全局等待开关不能未经测试直接等同于旧专用端点30秒等待。
- 原生failoverBeforeRetry和零单目标/整组重试足以映射旧链路；Chat/Responses/Messages逐账号一次及12成员链均通过。
- global comboCooldownWait会使零等待普通池等待；2秒上游耗时+29秒冷却即使comboTimeoutMs=30000仍在31.14秒后再发一次上游。保持BLOCKED，不扩补丁、不静默改变旧语义。
- 最初30秒Retry-After反例没有复现（margin超单次上限），已保留；最终结论依据29秒+已耗时2秒实测。

## Outcome
旧池和重试映射完成隔离验证；端点等待映射明确不等价且拒绝激活。实际连接binding未建立时不会将mock PASS提升为生产ready。报告：[044原生映射](../../research/2026-10-30-omniroute-routing-mapping-validation.md)；工具：phase5。
