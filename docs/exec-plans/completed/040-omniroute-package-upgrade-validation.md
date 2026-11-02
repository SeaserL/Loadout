# ExecPlan 040: 融合发布包、跨版本升级与SQLite热路径验证

## Goal
实际构建并运行带安全/账号补丁的独立后台包；完成两个发布版本之间的隔离升级并量化补丁迁移；测量SQLite依赖的来源、时间与移除成本，明确是否值得继续融合。

## Context
2026-10-30用户授权继续039验证，不做UI、不动正式环境。此前仅原生发布包和适配薄宿主分别通过，不能当成完整融合包。

## Existing behavior
039冻结补丁为7个原文件接缝+2模块；18实验检查及62 focused回归通过；热请求发送前记录197次SQL。相邻旧版静态试套4个文件失败，尚无实际前向升级。

## Constraints
只用development/upstreams下独立worktree、development/experiments/omniroute-phase2及独立临时目录；合成凭证、loopback mock、隔离home/data、测试凭证service。保留039原始源码/证据；不读生产凭证，不改生产配置/注册表，不启动UI，不提交推送。

## Architecture impact
ADR-019继续Proposed。本次允许将实验模块接入真正启动/配置生命周期，并做限范围热路径实验；不授权放宽正式安全和RouteSnapshot规则。

## Files expected to change
本计划、ADR-019范围注记、phase2脚本/补丁/证据/报告、reference-map/notices；上游新建phase2 worktree的构建/启动/安全/路由接缝。正式Loadout运行时代码不变。

## Implementation stages
1. 固定v3.8.51与v3.8.52，独立工作树；按本地官方构建入口准备backend release。
2. 接入OS store预热、账号约束加载、配置和hash-only，真实打包/解包运行。
3. 在旧版建立假账号数据，升级到新版运行；记录每个冲突和实际修改量，验证回滚副本。
4. 对SQL做请求上下文/阶段/调用点/耗时归因，比较配置收缩及集中缓存实验，区分冷启动/热请求/锁竞争。
5. 完成基准、泄露扫描、清理和是否继续融合的结论。

## Tests
真实HTTP发布包的鉴权、主/子集、空集、轮转、首选/strict、429、无跨模型、SSE首字节、OS store与重启。升级保留账号ID、client key和规则；版本输出及迁移表验证。外联禁止，故障注入仅测试数据。

## Performance validation
冷/热、串行/并发mock HTTP，SQL次数/耗时/调用点及选路边界；受控SQLite写锁实验；不给未运行路径下结论，不以SQL条数单独判定必须重构。

## Security considerations
Windows凭证调用串行、service严格限定loadout-test前缀，finally清理；仅测试目录SQLite/备份；源码构建与运行使用不同隔离环境，运行拒绝非loopback网络；清理前验证绝对路径及所属进程。

## Migration considerations
实际v3.8.51→v3.8.52；已知两版存在文件拆分，先建立旧版等价补丁，再升级新版。必须区分源码迁移、数据迁移与可运行完整包；不能用静态patch检查代替。

## Known risks
Next大依赖图与Windows构建、native打包、同步SQLite写入/锁、配置默认功能、凭证cache跨bundle实例、API旁路；若需要重写执行链，保留失败证据并给停止融合建议。

## Definition of done
发布包和升级的真实结果明确PASS/FAIL/未覆盖；补丁可审查、成本可量化；SQL问题有因果证据；明确继续或停止融合，所有测试进程/凭证清理，正式环境保持原状。

## Progress
- [x] 阅读039证据、相关ADR/Harness、上游构建与DB入口；确认远端相邻发布标签为v3.8.51/52。
- [x] 建独立worktree并构建两版完整适配包；ZIP校验、独立解压运行。
- [x] 实际跨版本迁移、重启、旧数据副本回滚与回归。
- [x] SQL归因、请求内memo、两轮写锁因果实验和串行mock基准。
- [x] 测试进程/凭证清理、报告及停止深度融合建议；8个编译缓存删除被自动审批阻断，详见报告。

### 2026-10-30 execution notes
- 已创建v51/v52两个独立worktree。v52完整backend-only构建在6GB失败，8GB完成编译及standalone组装；修复实验依赖缺失（sharp/sqlite-vec/js-tiktoken/better-sqlite3）并将keytar改为可追踪静态导入。等待搬移运行验收，v51正在构建。
- v51安全字段映射未抽出，不能直接沿用v52模块：补回MIT许可的providerSpecificDataSecrets及其读写接缝。前向patch最初4个原文件不能直接应用，不能把行数少等同于无冲突。
- 源码诊断宿主实跑v51→v52：193→209条迁移，账号ID/ref、客户端hash保持，scope/empty/rotation/named通过，旧数据副本回滚通过。此项不代替发布包升级。旧版薄宿主绕过proxy时无效key用例失败，保留原始结果，需完整proxy链验证。
- SQL诊断：20请求平均190.7次、1.634ms；请求内memo降至79.6次、1.034ms。500ms写锁反例：105ms→606ms，主要阻塞在选路前domain_circuit_breakers持久化。还存在lease过期清理与compression默认行写入；不能用缓存成功宣称RouteSnapshot边界满足。

## Decisions
- 使用已授权架构的v3.8.52及相邻已核验v3.8.51做实际前向升级；不更改共享checkout。
- 使用上游backend-only构建、保留完整API/启动逻辑，不将薄测试宿主重新命名为发布包。

## Final results (2026-10-30)

- 上述execution notes保留过程状态；最终v52采用Node内建module API加载原生keytar，原静态/动态导入试验失败均保留。两版最终build成功，未改变上游构建配置。
- 实际完整包目录升级记录`upgrade-1793333366421`；最终ZIP解包升级记录`upgrade-1793334886056`。后者28检查中27通过，唯一失败是v51原生缺少dashboard 404拦截；无fatal、迁移193→209、原凭证/client身份及账号规则保持、重启及备份回滚通过。
- 最终新版修改8个上游原文件，核心模块201行；旧版前向套用需适配6个原文件，另1个辅助文件已由上游提供。该单次升级样本不能外推所有版本维护量。
- 完整SQL探针包含exec后约247.68调用/3.20ms，请求内memo约136.60/2.04ms；exec时间含探针开销。500ms写锁先阻塞熔断器持久化，去重后仍阻塞选路中的租约清理立即事务。SQLite查询速度不是主要瓶颈，现有RouteSnapshot边界仍需更广的状态生命周期改造。
- focused上游回归62/62。发布包没有补跑SSE首字节、并发吞吐/长期压力或所有协议；不将计划覆盖范围冒称已全部执行。正式Loadout Rust未改，不以本轮替代其全工作区验收。
- 测试凭证删除3条、剩余0，哨兵扫描无命中，测试服务进程无残留；8个隔离SWC缓存仍在，自动审批两次拒绝删除（`blocked by policy`），没有绕过。
- 结论：按现有安全/热路径契约与少维护优先级，不继续三项目深度融合，不进入UI/正式迁移。保留稳定Loadout和实验资产，不授权新一轮全功能自研扩张。

交付见[验证报告](../../research/2026-10-30-omniroute-package-upgrade-validation.md)、实验README、最终解包证据。本轮验证完成；缓存清理的BLOCKED状态明确保留。
