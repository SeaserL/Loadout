# ExecPlan 037: 完整产品底座与需求迁移调研

## Goal
寻找可承接 Loadout 个人使用需求的完整开源产品，把维护重心从自研通用网关转为现成产品上的小范围增量；交付候选排序、需求包与迁移建议。

## Context
用户2026-10-29明确要求扩大到完整成熟产品，并确认“接受更换界面与操作方式，优先少维护”。036只验证CPA组件替换，不足以否决整产品迁移。

## Existing behavior
已具备Windows本地网关、账号/模型/子号池、SenseNova专用端点与轮转、密钥保护、状态与流契约。历史实机证据、mock回归与未完成接入需分开；035明确尚未在真实商汤流量验证。

## Constraints
不动正式环境、不读真实凭证、不执行候选应用、不调用真实Provider、不部署、不推送。用户授权广泛候选调研；先用官方仓库元数据/README/发布/维护证据筛选，再深入主要候选，避免重新逐功能造轮子。只写原创研究文档，不移植代码。

## Architecture impact
研究可替换Loadout整体，不要求保留Rust/Tauri/NamedPipe/文件布局。用户行为和安全底线优先于既有实现形式。实际采用前另行修订ADR，不在研究阶段改变当前契约。

## Files expected to change
本计划、docs/research/2026-10-29-product-foundation-review.md、docs/research/loadout-portable-requirements.md、docs/legal/reference-map.md、必要的元数据证据JSON。

## Implementation stages
1. 提取有依据的使用需求并分级，分离实现约束。
2. 广泛筛选完整产品，核查平台、发布、维护与许可证，不用star数代替成熟度。
3. 深入首选和备选的扩展边界、安全与路由，实现需求映射、可删除职责和维护成本比较。
4. 给出主推荐、备用与否决条件，形成可携带的需求验收包；收尾存档。

## Tests
只读源码/现有测试/历史运行记录交叉检查；每个候选注明证据层级。此轮不声称候选已在本机通过实测。文档链接、SHA与事实一致性检查。

## Performance validation
不改Router/Gateway，不运行benchmark。上游性能数字仅作为作者声明，不做跨项目实测排名。

## Security considerations
核查凭证存储、入口认证、请求正文保留及可关闭的扩展。评估补齐底线所需的改动面，而非把底线当营销差异。

## Migration considerations
优先原生配置，其次插件/公开API，再次少量集中补丁；不默认保留Loadout双网关。将本地需求包作为验收，不将既有源码逐模块搬运。版本切换、账户语义、回滚与上游同步需纳入试点计划。

## Known risks
功能多不等于成熟；默认分支可能不稳定；快速发布会增加补丁漂移；许可证/社区贡献政策可能限制增量路线；README与代码可能不同。

## Definition of done
候选有来源、推荐有条件、需求有证据与优先级；明确哪些职责可退休、哪些需少量定制、哪些尚未验证；不只交付功能愿望清单。

## Progress
- [x] 入口、产品/相关ADR、活动计划、036及历史实机/测试记录已核查。
- [x] 用户确认可更换UI/命令，减少维护优先。
- [x] 10个候选广筛；前三深入，OmniRoute发布版源码与Windows CI边界复核。
- [x] 需求包、10个合成验收向量、候选维护成本与推荐已形成；向量尚未执行。
- [x] JSON可解析；10个候选、18个重点源码路径、10个待执行场景；相对文档链接全部有效；reference-map diff检查通过；计划归档。

## Decisions
- 036的“保留主链路”限定为组件替换结论；本轮允许推荐整体迁移。
- 历史自研架构不是筛选门槛；现有已验证行为才是。
- 首选OmniRoute整产品试点、Magpie备选；正式环境暂留回滚。安全存储与Windows运行是前置淘汰条件。
- OmniRoute缺密钥/加密异常会明文回退；不得直接采用默认配置。保留当前OS store规则，替代存储方案需另行ADR。
- 未运行候选应用、未调用真实Provider、未改生产代码或正式环境。

## Delivered artifacts

- [完整产品底座选型](../../research/2026-10-29-product-foundation-review.md)
- [可迁移需求包](../../research/loadout-portable-requirements.md)
- [合成验收向量](../../research/product-foundation-acceptance-cases.json)
- [来源快照](../../research/2026-10-29-product-foundation-evidence.json)

本轮只有文档与研究数据变更，未运行代码测试或benchmark；已有用户改动保持原状。尚待执行的是候选运行试点，不属于本轮研究通过结论。
