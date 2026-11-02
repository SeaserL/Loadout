# ExecPlan 041: OmniRoute 原生SQLite路线重新评估

## Goal
以少维护为首要目标，评估接受OmniRoute原生SQLite状态模型、保留应用凭证保护与账号路由语义的路线；纠正040将实现约束作为迁移否决条件的决策前提。

## Context
用户明确要求先评估，不因旧架构退回全量自研。039/040已有固定版本源码、完整包升级和故障注入证据，本轮优先复用，不将历史测试冒充新测试。

## Existing behavior
v51→v52融合实验包升级可用；新版核心adapter201行、8个上游接缝，旧补丁前向6处不能直接应用；SQLite查询几毫秒但受控写锁阻塞。凭证完整生命周期、多协议和并发仍未全覆盖。

## Constraints
仅文档与只读源码/证据评估。不启动服务、不读取正式凭证、不改运行时代码、不重新构建、不消耗Provider额度、不做UI、不部署、不提交推送。

## Architecture impact
提出候选路线对RouteSnapshot、选路前禁SQL、Rust内存清零及固定延迟预算的替代评估；ADR-019保留Proposed，正式Accepted ADR不被评估文档直接改写。

## Files expected to change
本计划、新评估报告、ADR-019补充、040报告前提提示、reference-map及可迁移需求说明。

## Implementation stages
1. 核对需求与039/040证据。
2. 复核原生SQLite配置、凭证/导出路径与账号语义差异，查官方SQLite并发/耐久性说明。
3. 给出风险、最小适配边界、后续验收和明确推荐，交叉链接避免旧结论被无条件复用。

## Tests
本轮不新增运行测试。检查文档相对链接、引用行、证据hash和diff格式；新发现以源码观察或推断标注，不能标记runtime PASS。

## Performance validation
复用040 mock耗时及500ms写锁实验；明确故障注入不等于实际发生率，尚无并发/长时间/故障恢复完整验收。

## Security considerations
区分应用层普通持久化不含秘密与OS保护存储；不承诺物理磁盘/内存转储无痕。凭证生命周期与权限故障应拒绝、不扩大范围；保留现有鉴权/日志脱敏配置，不建立第二套网关。

## Migration considerations
接受上游schema和migration；推荐停服务备份与旧版本+旧数据回滚；OS凭证与DB不是同一事务，必须另验更新/恢复一致性。

## Known risks
同步锁阻塞事件循环、NORMAL断电耐久性、明文导出/未知凭证字段、缓存撤销及多协议绕过、补丁升级漂移；均需区分实测/源码/推断。

## Definition of done
明确回答新路线是否更合适，说明剩余真实风险和最小验收；不以旧技术约束否定，也不把小样本通过说成可立即生产迁移。

## Progress
- [x] 需求与历史证据复核。
- [x] 固定版本SQLite、凭证导出、字段映射及账号约束源码复核；官方SQLite资料核验。
- [x] 写出推荐、适配边界与验收范围，并修正文档的引用前提。
- [x] 文档相对链接、历史证据hash与diff格式检查；完成评估交付。

## Decisions
- 首选评估方向为OmniRoute原生状态模型与小范围安全/账号适配；不维护Loadout完整运行时或私人RouteSnapshot分叉。
- 本轮只记录候选路线，不把“值得继续验证”升级为生产迁移授权。

## Outcome
推荐接受原生SQLite作为后续首选路线。同步锁与NORMAL耐久性按个人网关运行风险验收，不再因不符合RouteSnapshot否决；秘密导出、OS store/DB双存储生命周期、空集合及多协议账号规则仍需验证。未制作新adapter，不宣称代码量已下降；未运行新测试。正式运行代码和参考checkout未修改。

交付：[041评估报告](../../research/2026-10-30-omniroute-native-sqlite-assessment.md)。040数据保留，结论加前提更新，ADR-019保持Proposed。
