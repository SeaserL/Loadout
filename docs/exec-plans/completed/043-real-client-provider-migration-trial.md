# ExecPlan 043: 固定底座、真实客户端及迁移恢复隔离验证

## Goal
固定042通过的OmniRoute v3.8.52及其补丁，使用本机真实客户端和Provider验证可迁移性，交付可重复运行的隔离迁移、加密备份及恢复/回滚工具。三类入口同等重要：商汤/CommandCode、WorkBuddy、OpenCode免费桥接。

## Context
用户明确授权下一阶段实际使用链路验证，仍不做UI、不切正式环境。真实Provider只做有限合成任务，显式LOADOUT_LIVE_TESTS=1；不启动全模型扫描、批量测速或无限重试。

## Existing behavior
042基线132项通过；430行自有模块、14个上游接缝。原生SQLite与请求队列保留。真实数据只读盘点发现商汤12账号、CommandCode、两个本地桥接，另有外部管理/订阅账号。Codex、Claude Code、OpenCode官方可执行文件已定位；正式客户端配置保持原样。

## Constraints
正式DB仅readonly读取，正式credential仅按选定ref读取，不写不删；复制的API key只进入随机隔离OS vault。外部管理OAuth/浏览器登录不复制。独立目录、端口、客户端home和worker；不修改正式安装、自启、配置或活动会话。桥接沿用限速/冷却保护，不绕过403、登录或配额限制。无commit/push。

## Architecture impact
接受原生SQLite，不添加第二路由器。固定已验收tag而非自动追最新。迁移工具位于运行时之外；账号/路由映射优先通过原生管理API，不能静默降级不支持的policy。原生普通备份仍不带secret；可移植备份使用口令加密。

## Files expected to change
development/fusion-baseline.lock.json；development/experiments/omniroute-phase4/工具、隔离证据；ADR-019授权范围、reference-map/notices及报告。仅必要且经反例证明时调整实验上游副本，正式Loadout源码不改。

## Implementation stages
1. 固定版本/补丁，盘点实际使用元数据、客户端版本及桥接可隔离条件。
2. 实现新建隔离实例、只读迁移预览/应用、加密备份、新目录恢复/回滚；对不支持的行为明确报告。
3. 先用假凭证及mock验证工具拒绝正式目标、错误口令/篡改/失败恢复和账户scope；再迁移选定真实API key到测试vault。
4. 官方客户端独立进程→OmniRoute→真实Provider，短文本、流式及有限工具往返；桥接独立进程或报告不可隔离原因。不得把HTTP模拟客户端当成官方客户端通过。
5. 用备份恢复后的实例重复关键链路、核验正式配置未变、清理测试凭据/进程并形成维护成本与迁移建议。

## Tests
路径/实例所有权、空目标、只读源、重复迁移、未知policy拒绝、加密错误/篡改、恢复失败不覆盖原实例、源凭证不变；客户端真实版本与协议、上游精确模型、工具参数与结果、输出不落网关普通存储、取消/超时、范围隔离。所有失败保留。

## Performance validation
真实场景报告延迟/请求次数及失败类别，不做额度消耗型benchmark；未改运行时则沿用042性能证据。若改Router/协议接缝，追加相应mock回归和性能检查。

## Security considerations
任何真实key不进入命令行、配置文件、日志或工具输出；传输仅内存/受控stdin与loopback认证管理API。客户端测试key可临时内存注入，隔离home不得携带真实订阅登录。预算采用每条链少量请求、固定小任务、失败停止；官方客户端生成的合成测试会话与网关正文存储分开记录。

## Migration considerations
源账号/顺序/子集/端点转为OmniRoute连接、scope和模型选择；不保留旧内部实现。不可迁移项有显式拒绝清单。恢复至全新测试实例，旧实例保留直到验收；不原地改正式数据库。升级成本以固定基线、补丁接缝和可运行验收衡量，不凭行数宣称无维护。

## Known risks
官方客户端配置隔离能力、Provider认证/额度/工具差异、桥接依赖已登录官方客户端、历史403保护、模型斜杠别名、原生管理后台旁路与日志。实际不可用时记录BLOCKED/FAIL，不为了通过切模型或重启正式服务。

## Definition of done
基线可定位，工具可复验，真实客户端/Provider逐条有明确结果，恢复关键链路有证据，正式配置不变，测试进程/credential清理，形成是否可以进入后续迁移的明确判断。

## Progress
- [x] Boot sequence及042证据复核，读取实际metadata和客户端安装位置。
- [x] 用户确认三类链路同等重要。
- [x] 固定v3.8.52；自定义Provider反例要求新增prefix/nodeName两项元数据白名单，完整后台已重建。
- [x] 工具mock验收：14/14通过，含有斜杠模型、错误口令/篡改、独立vault恢复、原实例回退和清理。首轮失败证据保留。
- [x] 实际客户端文本与Claude/OpenCode工具mock 5/5；免费桥接真实文本及新实例恢复后文本通过。CommandCode 401、商汤429、WorkBuddy IPC缺失明确BLOCKED，非生产验收通过。
- [x] 真实13账号API key及测试桥接key备份恢复，源credential/配置不变，明文扫描0；隔离凭据和进程清理。报告给出继续候选但不切换的结论。

## Decisions
- 新版指上一轮已验收v3.8.52，不未经验证跳到最新默认分支。
- 默认不调用独立DeepSeek付费账号；近期使用的商汤、CommandCode及桥接优先。任何额外真实Provider扩展须有明确必要性并记录预算。
- 未知路由policy不得被默认为无限制；迁移预览必须暴露差异。
- 实际源有两个Pool及一个专用端点，当前工具只做显式partial迁移并重发client key；不声称原URL/等待语义已完整迁移。
- 真实验证通过固定上游的loopback计量relay约束模型、请求数和输出token；它只属于测试设施，不进入融合运行时。
- 原生executor在requestRetry=0之外仍有同URL两次429重试。首次商汤共3次请求，失败记录保留；测试relay已添加首次拒绝后关闭出口，5/5反例通过，不再次消耗上游验证此修正。
- 免费worker首次启动失败由Python大小写环境变量过滤漏传SYSTEMROOT导致，修正测试脚本后通过。OpenCode工具fixture修正v2的path字段及Location相对读权限，无运行时协议改动。

## Outcome

本轮隔离验证完成，验收状态为PARTIAL/BLOCKED而非生产PASS。继续OmniRoute原生SQLite与432行最小适配候选；不做UI、不切正式环境、不退回全量自研。后续需正常恢复外部账号/官方会话，对齐池/端点/重试语义，再验收多轮真实coding。运行命令、失败证据和完整限制见[043报告](../../research/2026-10-30-omniroute-real-client-migration-validation.md)与工具README。
