# ExecPlan 036: CLIProxyAPI 隔离替换验证

## Goal
验证 CLIProxyAPI 能否承担 Loadout 的协议转换与 HTTP 接入，同时保留 Loadout 的凭证安全、账号选择与失败切换契约；交付明确迁移或保留结论。

## Context
2026-10-29 用户授权隔离验证，明确不动正式环境。唯一社区实现参考为 CLIProxyAPI，固定 `90c447d3a8727d115aa7552ec93ce023736ed492`。现有未提交修改不属于本任务。

## Existing behavior
Loadout 的 RouteSnapshot 决定账号/模型，Gateway 保留首内容前 failover、提交后禁止换路、Responses 状态固定；凭证由 Windows Credential Manager 保存，热路径使用预构建内存 Endpoint。公共 HTTP 接口强制 client key。参照 ADR-002/003/004/005/006/008/010/011/012/015。

## Constraints
- 不启动、停止、更新或配置正式实例；不读取真实凭证、请求正文或账号数据。
- 所有上游为 loopback mock，随机端口、合成凭证、临时目录；不运行 OAuth、订阅推理或真实 Provider 调用。
- 只在 development/experiments/cliproxy-eval 与本计划、research/法务参考记录添加实验资产；不改生产 crate。
- 外部源代码仅在临时目录检出，不复制到仓库。构建可下载公开依赖，运行阶段拒绝非 loopback HTTP 出站。
- 不改变现有 ADR；实验不是上线批准。

## Architecture impact
比较 stock/SDK 的默认风险与一个最小受控适配边界：Loadout 保留账号/模型选择和外层认证，CPA 每次只能执行指定账号，不持有可自行选择的账号池；凭证仅在内存注入，禁止持久化。验证官方 translator/executor 是否可以在不维护上游 fork 的情况下使用。实际 Loadout 路由测试与 CPA 实验须分清集成证据和组件证据。

## Files expected to change
`development/experiments/cliproxy-eval/**`、本计划、`docs/research/2026-10-29-cliproxy-isolated-evaluation.md`、`docs/legal/reference-map.md`、`THIRD_PARTY_NOTICES.md`。

## Implementation stages
1. 固定源码与许可证，检查 SDK/translator/executor 的真实边界。
2. 构建离线 mock 验证器，保留失败与修复前后的证据。
3. 验证协议/工具/流/错误、账号固定、安全、取消与性能；复用实际 Loadout 路由契约进行核验。
4. 汇总不兼容项、适配成本、剩余不确定性；给出结论并清理实验进程。

## Tests
- Chat / Responses / Messages，流式与非流式，tool call 与结果往返。
- 指定账号、429 与错误流、提交后失败、不偷换模型/账号、有状态请求边界。
- 合成 secret 及正文标记在日志/配置/输出文件中不泄漏；禁止隐式保存、非 loopback 请求。
- 取消/超时、连接回收与关停；所有 listener 使用端口 0。
- 必要时用独立 Rust harness 引用实际 Loadout crate；不把模拟选路当成 Loadout 集成通过。

## Performance validation
同机 mock 基线与 CPA adapter 的非流式/首事件延迟；报告样本与分位数，不外推真实 Provider。若未改生产 Router/Gateway，不以重跑全部基准冒充改动验收。

## Security considerations
实验账号和 API key 全部生成，不接触正式 Credential Manager；测试扫描的是本次临时目录。使用具体监听地址约束 HTTP 出站，禁用日志正文/调试持久化、自动更新和管理界面下载。如需要 Credential Manager 验证，仅独立测试前缀且结束删除。

## Migration considerations
生产保持原状；结论必须区分可迁移的组件、不能直接替换的责任与正式迁移所需 ADR/回归。

## Known risks
SDK 文档 v6 与实际 v8 不一致；internal 包不能跨 module 导入；执行器隐式重写/日志/刷新；双层 retry 与状态错位；mock 成功不证明 OAuth 或真实厂商可用。

## Definition of done
固定版本、可重跑的实验、结果文件与安全扫描、明确结论完成；不遗留实验后台服务，不修改正式环境。

## Progress
- [x] 入口、相关 ADR、参考地图、活动计划与现有 Gateway/Provider abstraction 已核查。
- [x] 确认 Go 1.26.5 可用，外部源码克隆到任务专属临时目录。
- [x] SDK 边界审计与许可证检查；公开 translator 可用，独立调用内置 executor 需要 internal 入口或完整 Service 生命周期。
- [x] 组件实验已执行：指定账号成功；错误内容、Debug、状态请求与部分流终止不满足直接替换契约。保留失败，不修改上游。
- [x] 实际 Gateway → CPA → mock 串联14项通过；保留了 Loadout 协议层。QPC 完成非流式和首内容各100次采样。
- [x] 组件55项叶级检查46通过9不满足直接替换契约；go vet、公开SDK/内部包边界、脚本语法检查完成。
- [x] 报告、源码、原始结果与哈希归档完成；未改生产crate、配置或正式实例。运行目录清理，bridge退出码0。
- [x] 结论：保留当前协议和接入主链路；仅把CPA受控适配保留为未来具体新增需求的候选。

## Decisions
- 以可检验的契约而非功能数量作为替换标准。
- 若 SDK 需要改上游核心才能满足安全/选路，不隐藏该成本。
- 实验入口只添加在固定版本checkout，没有改上游核心；此方式仍需维护构建覆盖层和内部API兼容。
- 初期桥接帧封装错误不计为CPA缺陷；Messages未实现的两个集成用例明确排除，原始日志保留。最终14项没有放宽既有断言。
- 不将14项Transport串联通过写成协议替换通过；不将进程内性能写成IPC/生产性能。
