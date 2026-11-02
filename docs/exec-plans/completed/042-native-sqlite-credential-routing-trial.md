# ExecPlan 042: 原生SQLite凭证生命周期与协议路由隔离验收

## Goal
接受OmniRoute原生SQLite，补齐凭证修改/删除/安全导出/恢复及实际协议账号规则，量化最小适配和两版本升级成本。

## Context
用户已授权041建议的后续隔离验证；不做UI、不动正式环境。039/040原始证据和工作树保留。

## Existing behavior
040包可升级，但凭证删除/导出/恢复未闭环，账号规则仅Chat串行；原生SQL模型已被用户接受。

## Constraints
新phase3工作树、独立home/data/随机测试service、loopback mock。无真实Provider/凭证/额度，无生产配置、安装、自启和UI变更。不提交推送。

## Architecture impact
本实验免除RouteSnapshot/零SQL与Rust清零实现约束；采用上游状态和协议实现。保留应用普通存储无秘密、client hash-only、账号范围与同模型规则。ADR-019记录试验授权。

## Files expected to change
phase3实验、两个上游独立工作树的凭证/账号接缝、报告/ADR/reference-map/notices；正式Loadout运行时不改。

## Implementation stages
1. 固定v51/v52、复用既有构建依赖与源码补丁建立新实验副本。
2. 集中凭证写入串行化、孤立ref清理、删除和带口令加密的凭证备份/恢复；阻止普通明文导出。
3. Chat/Responses/Messages验证账号规则，尽量共用上游入口；有状态请求无法保证时明确拒绝。
4. HTTP生命周期/流式/取消、mock基准、实际两版本升级与必要补丁统计。
5. 安全扫描、清理、冻结证据并给出继续条件。

## Tests
原生管理API修改/删除/账号权限；拒绝普通秘密导出；加密备份错口令/篡改/恢复；写入失败和OS store失败。三个客户端协议、空集/子集/新增/首选/strict/同模型failover/轮转、流式工具和取消。未完成项明确记录，不冒充PASS。

## Performance validation
保留原生SQL；串行及1/5/10并发mock，观察适配前后实验版本及锁释放恢复，不以SQL数量或旧<10ms预算否决。没有无适配原生包的严格对照，不声称精确增量开销。

## Security considerations
仅测试service；凭证操作串行；应用DB/WAL/log/export扫描；Node字符串无清零承诺。普通DB备份只有ref；删除旧secret后恢复须配套受口令保护的凭证备份，不静默保留已删除秘密。

## Migration considerations
同一合成数据v51→v52，旧包+旧数据+对应加密凭证备份回滚；冻结两个版本补丁与实际前向冲突。无正式数据迁移。

## Known risks
原生导出/导入旁路，OS store与SQLite非原子事务，异步管理操作缓存，多协议复制Request丢约束，native打包与上游漂移；不扩大为重写执行器。

## Definition of done
生命周期与协议结果有可复核证据，运行升级与补丁成本明确；失败/限制保留；测试进程和凭证清理；给出最小适配是否值得继续的判断。

## Progress
- [x] 阅读041/040、相关ADR/Harness及原生入口。
- [x] 新建v51/v52隔离副本，移除SQL探针/实验memo并接入集中生命周期适配。
- [x] 凭证、协议和并发验收。
- [x] 包运行升级、成本与安全扫描。
- [x] 清理与最终报告。

## Decisions
- 根据仓库已记录的Codex/OpenCode/Claude Code消费者覆盖Responses、Chat和Messages；只用mock协议请求，不启动或改动真实客户端。
- 单一上游执行链，不加RouteSnapshot或私人SQL缓存。凭证删除实际删除OS条目，回滚需匹配加密凭证备份。

## Execution notes
- v52第一轮独立完整包实跑58检查，57通过；Chat/Responses/Messages的账号和流式工具、取消、不重放均通过，原生管理修改/删除、加密备份、错口令与篡改拒绝也通过。失败为缺少OS凭证时仍发出无有效鉴权的请求；mock接受了它，保留失败记录`lifecycle-1793341435441`。
- 已将缺失引用由null改成明确错误，增加启动时按原生DB引用清理中断写入的孤立OS条目；正在重新构建两版复验。没有为此改变SQLite或建立快照Router。
- 原生管理的启用/停用标记作为已审计非秘密metadata放行；仍限定API key认证和已审计字段，OAuth/复杂cookie不宣称支持。
- 完整两版第二轮`lifecycle-1793342989514`为132项、129通过；升级、回滚、缺失vault拒绝、三个协议和并发/锁恢复通过。反例：v51轮换/删除client key仍被缓存接受，v52更新秘密后返回处理把明文误读作ref而报500。独立诊断`lifecycle-1793343216141`确认v51旧key确实还能发出请求，v52原生撤销有效；不能归因于测试状态码。
- 修正限于v51试验关闭该鉴权缓存TTL、v52写后返回先转换ref再读取；接受并校验原生apiKeyHealth/selectedKeyId非秘密状态，避免安全字段限制误阻断健康持久化。新测试让两版分别换成不同的新凭证，增强跨版本覆盖。
- 第二轮10并发正确，但单请求约350ms；凭证适配原先对普通状态更新也遍历OS store。改为仅在已加载/暂存ref失去DB引用时遍历删除，成功无秘密变更的更新只查询原生ref集合。没有新增Router缓存；正等待最终发布包复验。
- 最终完整链`lifecycle-1793344267847` 132/132通过，migration记录193→209、旧包/旧数据/匹配vault回滚通过，无秘密/正文哨兵泄露，健康持久化错误0。新版430行自有模块，14个上游文件+91/-17；各自tag补丁重放通过，直接向新版11路径不能应用，三方日志报告8处冲突及重复回移文件失败。
- 缩小OS清理未显著改善350ms；CPU采样与原生队列设置对照`lifecycle-1793344698872` 28/28通过，确认默认主动间隔350ms为主要原因。仅原生PATCH设置为0，单请求中位数77.1ms、10并发228.5ms，无SQL重构。保留直接DB写设置未同步runtime的早期诊断。
- `--v52-only`的末尾删除测试曾对已停止服务请求而fetch failed，修正测试分支；最终完整链不受影响。所有失败均保留，不以改断言掩盖产品错误。
- 独立复查8个本轮测试service凭证总数0，实验包进程0；参考checkouts clean。脚手架保留，未重试此前SWC缓存删除拒绝。未改正式Loadout运行时/安装/凭证/端口/自启，无真实Provider或UI工作。
- [最终报告](../../research/2026-10-30-omniroute-native-sqlite-validation.md)：值得继续原生SQLite的受控小fork；不批准生产迁移，不返回全量自研。
