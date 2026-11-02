# ExecPlan 038: 三项目融合架构与本地源码脚手架

## Goal
准备 OmniRoute、Magpie 的独立本地源码参考，交付可审阅的融合架构、功能归属、连接契约与分阶段验证计划。

## Context
用户要求结合 OmniRoute 和 Magpie，保留 Loadout 的个人需求与安全规则；先出解耦文档，允许将上游源码抓到 D:\Loadout 内，开发完成后再清理脚手架。优先少维护，允许更换界面与操作方式。

## Existing behavior
Loadout 当前正式实现维持不变；037 已形成整产品选型、需求包和待执行验收向量。OmniRoute 首选运行时，Magpie 偏好流向可视化；免费资源更新时间需分层。

## Constraints
本轮只下载源码、静态阅读、写文档与来源清单。不安装依赖、不执行上游脚本、不启动服务、不访问真实凭证/Provider、不改正式客户端、不提交或推送。上游 checkout 保留 Git 历史、LICENSE 与源文件头，不加入父仓库提交或生产打包。

## Architecture impact
拟议采用一个推理数据面；控制面与显示层通过清晰契约隔离。重大架构选择记录 Proposed ADR，不覆盖现行 Accepted ADR。源码参考不等于已经接入或可直接移植。

## Files expected to change
development/README.md、development/upstreams.lock.json、development/upstreams/ 下忽略的独立Git checkout；.gitignore；docs/architecture/ 下融合设计；Proposed ADR与索引；CONTEXT.md；reference-map与THIRD_PARTY_NOTICES；必要的需求勘误。

## Implementation stages
1. 核对入口、术语、相关ADR、活动计划和现有代码。
2. 创建本地上游 checkout，固定已研究SHA，记录remote/tag/license/工作区状态。
3. 核查凭证、路由、免费目录、telemetry、Magpie可视化的真实依赖和复用范围。
4. 编写模块职责、连接协议、数据所有权、生命周期、故障传播与迁移/清理边界。
5. 校验SHA、许可证、链接、忽略规则与文档一致性，归档计划。

## Tests
仅Git完整性/checkout状态、版本来源、文件级许可证与文档链接检查；不执行上游测试。拟议接口与运行可行性明确标为待验证。

## Performance validation
未修改Router/Gateway，不运行benchmark；设计规定异步遥测及热路径边界，后续运行试点必须验证。

## Security considerations
下载仅来自官方公开仓库；禁用hook执行，不运行安装/自动发现；独立目录不含正式数据。凭证在OS store、DB只保存引用，正文默认不落盘，不采用明文回退。安全适配覆盖刷新/导入导出，不假设仅改一个函数即可完成。

## Migration considerations
不是拼接三个常驻网关；选定一个主运行时，其余按行为、组件或设计复用。旧版本暂留回滚。清理只在源码来源、必要移植、许可证和回滚材料完整后做，本轮不删除。

## Known risks
Go DOM脚本不等于可插入React组件；OmniRoute没有自动满足当前ADR；深层fork可能消除维护收益；部分功能依赖远端Radar而非本仓库；原有需求包可能过度简化exact账号语义，需按代码/ADR核对。

## Definition of done
两份上游源码可本地访问且固定版本；架构文档可回答“什么来自哪里、怎么连接、谁写什么、失败怎么办、将来删什么”；无生产变更。

## Progress
- [x] 入口、产品需求、主要ADR、reference-map与活动计划已核查。
- [x] 本地源码下载与detached checkout：OmniRoute v3.8.52；Magpie fd80ed99（本地tag v0.1.2026）。两者干净、非浅克隆，历史blob按需获取。
- [x] 源码依赖核对、功能归属、数据所有权、5类连接契约、阶段验收与清理边界完成；新增Proposed ADR-019。
- [x] 两仓库HEAD/LICENSE哈希、干净状态、非浅克隆与Git connectivity检查通过；父仓库忽略规则生效，索引无嵌套源码。
- [x] 35个本地文档链接有效；14个选读路径未发现祖先嵌套许可证覆盖；JSON解析与diff空白检查通过；计划归档。

## Decisions
- OmniRoute 为主参考，Magpie 为次参考；不再广搜其他项目。
- 架构草案先审阅，不在本轮开始融合实现。
- 源码确认exact账号默认可同模型failover，需纠正037需求包的绝对“不换号”表述。
- Magpie流向页依赖自身DOM插件和trace API；通过独立metadata契约重新适配，不导入其网关/账户后端。
- OmniRoute客户端key可逆保存、quota preflight和Electron退出行为为明确待适配项；不能假称默认上游满足安全/热路径/后台生命周期契约。

## Delivered artifacts

- [主架构文档](../../architecture/loadout-fusion-design.md)：功能归属、两张图、数据所有权、连接契约、生命周期、开发阶段和清理条件。
- [ADR-019](../../adr/ADR-019-product-fusion-boundaries.md)：Proposed，不替代现行Accepted ADR。
- [本地源码说明](../../../development/README.md) 与 [来源锁定](../../../development/upstreams.lock.json)。
- CONTEXT.md补充资源/首选账号/固定目标/实际轨迹术语；R02/R09及对应验收向量勘误。

没有运行上游安装/构建/测试/服务，没有修改生产模块或正式数据。当前只验证下载来源、静态接口与文档，不证明融合运行通过。

