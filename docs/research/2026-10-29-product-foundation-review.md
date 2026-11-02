# Loadout 完整产品底座选型

日期：2026-10-29。结论：**建议转向完整开源产品上的增量维护，首选 OmniRoute 做迁移试点，Magpie 作为备选；当前正式实例暂留作日常服务与回滚。** 这不是继续扩建 Loadout 通用网关的建议，也不是现在就切换生产。

用户接受更换界面与操作方式，优先少维护。筛选目标因此是能够接管账号管理、网关、协议适配、界面、分发升级的完整产品，而不是可以嵌入 Rust 的某个库。Loadout 的价值可以保留为需求、配置和验收规则，不需要以一套独立产品代码继续存在。“缺少独有功能”本身不能证明迁移便宜；决定因素是迁移后能实际删除多少长期责任。

## 决策摘要

1. **主选 OmniRoute**：已发布版本具备 Windows 分发、管理界面、多账号、账号范围限制、优先级与轮转，最接近把现有个人网关整体交给上游。账号子集和路由偏好有原生承接点，且近期存在多个作者合并的修复，增量维护有上游出口。
2. **备选 Magpie**：个人 AI 工具控制中心的产品方向最接近，界面与 agent 接入完整。但项目很年轻、当前拒绝外部 PR，明文凭证持久化需要修改核心。插件可承接偏好，不能假设它能强制执行安全边界。
3. **CC Switch 不作主网关底座**：桌面管理与工具配置更成熟，但聚合模式不提供 failover；按工具切换/故障队列与我们的按模型多账号总 API 不同。为它补齐这些语义可能重新造一个路由产品。
4. **不采用“Loadout 保留全部控制面，再套完整新网关”的长期双层架构**。迁移成功后删除 Loadout 的通用职责；迁移失败则保留当前稳定版，记录具体阻塞，不因此恢复无边界自研。

本轮是官方来源、发布记录、源码与本地历史记录的选型研究。未运行候选程序，未使用真实凭证，未证明候选通过本机可靠性验收。没有发现一个“无需适配、已经证明满足所有底线”的现成产品。

## 候选筛选

版本是此次 API 抽样得到的发布，不代表任意镜像或独立发行仓库的绝对最新版本。源码默认分支与发布标签分开。没有用 star 数或宣传的性能数字排序。

| 项目 | 完整产品与维护信号 | 适配判断 |
|---|---|---|
| [OmniRoute](https://github.com/cheaper-inference/OmniRoute) | 2026-02 建仓；v3.8.52 提供 Windows Electron 安装/便携包；Web 管理、CLI、多账号与组合路由；MIT；近期多人修复被合并 | **首选试点**。凭证保护、日志关闭覆盖率、严格轮转和 Windows 生命周期仍需验收 |
| [Magpie](https://github.com/yetone/magpie) | 2026-09-23 建仓，距调研约36天；抽样 v0.1.2023；Windows GUI/CLI、路由组、agent 管理；MIT | **备选**。高频发版不能替代稳定性记录；当前不接受外部 PR，安全改动可能形成长期 fork |
| [CC Switch](https://github.com/farion1231/cc-switch) | 2025-08 建仓；v4.0.21，Windows MSI/便携包、更新机制与跨平台测试；MIT；近期多人合并 | 工具配置管理强，主网关语义不匹配，不因同为 Rust/Tauri 而优先 |
| [EasyCLIProxyAPI](https://github.com/router-for-me/EasyCLIProxyAPI) | 2025-09 建仓；v0.3.58；Tauri 管理界面、账号/配额/配置/CPA 生命周期；MIT | **第二梯队**。比裸 CPA 完整，但账号策略和安全需要另验；同时跟随桌面壳与 CPA 两层 |
| [ProxyPal](https://github.com/heyhuynhgiabuu/proxypal) | 2025-12 建仓；v0.4.53，Windows 安装/便携包；CPA 桌面管理；MIT | 同类备选，最近发布/推送较慢，不等于弃置；未见足以优先 EasyCPA 的需求优势 |
| [AxonHub](https://github.com/looplj/axonhub) | 2025-09 建仓；v1.0.0-beta11；完整 Web 管理与网关，有 Windows 包；根 Apache-2.0，llm 子树 LGPL | 团队网关候选；个人账号工作流、安全存储需适配，混合许可证需按文件处理 |
| [New API](https://github.com/QuantumNous/new-api) | 2023-11 建仓；v1.0.0-rc.47；渠道、令牌、分组、统计与分发完整；AGPL-3.0 | 候选中历史更长，但偏运营/额度分发。可独立采用，不能按当前 Loadout 许可策略直接搬代码 |
| [Bifrost](https://github.com/maximhq/bifrost) | 2025-03 建仓；完整管理面、路由与插件；Apache-2.0 | 团队/服务端方向。此次未确认原生 Windows 桌面包，不能写成不支持 Windows；需区分开源与企业功能 |
| [Relay AI](https://github.com/jacob-bd/relay-ai) | 2026-06 建仓；v0.16.1；CLI、浏览器管理、网关、OS keychain 路径；MIT | 凭证思路值得关注；完整账号池治理与长期发布证据较弱，不作为主选 |
| [lmgw](https://github.com/lmgw-dev/lmgw) | 2026-09 建仓；Linux/Podman 桌面路线；MIT | 与 Windows 日常使用不匹配，不为迁移先移植平台 |

此次允许扩大候选范围来自用户明确要求，先广筛产品，再深入前三。EasyCPA 等没有逐路径安全审计，不能据 README 宣称通过 Loadout 契约。Bifrost 抽样 release 中有组件标签，不把它们当整产品版本。AxonHub 的 beta、New API 的 RC 也不因 API 的 prerelease 字段为 false 就称为稳定版。

## 为什么 OmniRoute 排第一

### 已发布版有可映射的账号语义

重点检查 **v3.8.52，commit ad6f33f66bda34eba98645d3bfdabbde294bc843**：

- [keyConnectionScope.ts](https://github.com/cheaper-inference/OmniRoute/blob/ad6f33f66bda34eba98645d3bfdabbde294bc843/src/sse/services/keyConnectionScope.ts)：客户端 key 可限制 allowedConnections，并与 quota 范围求交；无可用账号拒绝，避免退化成不设限。
- [auth.ts](https://github.com/cheaper-inference/OmniRoute/blob/ad6f33f66bda34eba98645d3bfdabbde294bc843/src/sse/services/auth.ts)：优先级、fill-first、round-robin 与 provider 级策略覆盖已有实现。它的轮转有批次粘性，默认 stickyRoundRobinLimit=3，session affinity 又先于轮转；不能把名字相同当成 Loadout“每次请求换号”已经等价。
- [accountFallback.ts](https://github.com/cheaper-inference/OmniRoute/blob/ad6f33f66bda34eba98645d3bfdabbde294bc843/open-sse/services/accountFallback.ts)：已有账号与模型故障分类、冷却路径。仍需复现“三个账号，前两个429，第三个成功”，验证重试预算不会提前耗尽。
- 当前开发分支增加了 preferredConnections 文档，但发布版没有该文档。本推荐不依赖这一新功能，先使用已发布版的账号范围与优先级机制。

子号池可尝试用不同 client key 的账号白名单表达；总 API 可尝试用受限入口与模型映射表达。URL、菜单、命令可以改变，**不能把只修改客户端 key 当成全局隔离已经成立**：必须确认所有实际消费者都切到相应范围，旧 key 也不能绕过预期限制。这些是配置候选，不是已验证支持的最终配置。

### 安全差距集中，但尚未证明修补成本很小

[encryption.ts](https://github.com/cheaper-inference/OmniRoute/blob/ad6f33f66bda34eba98645d3bfdabbde294bc843/src/lib/db/encryption.ts) 提供字段级 AES-GCM；未配置 STORAGE_ENCRYPTION_KEY 时原样存储，encrypt 捕获异常后也返回明文。仅把“支持加密”勾上是不够的。[providers.ts](https://github.com/cheaper-inference/OmniRoute/blob/ad6f33f66bda34eba98645d3bfdabbde294bc843/src/lib/db/providers.ts) 的写入调用集中封装，适合作为试点检查边界，但刷新、导入导出、settings secrets 等其他路径也必须覆盖。

当前要求仍是 ADR-004：secret 在 OS store，SQLite 只存引用；client key 原文不可持久化。首选试点采用可替换的凭证读写边界验证 secret_ref + Windows 保护存储，不能仅设置一个 .env 明文主密钥就算完成。

“数据库密文 + 主密钥由 Windows 保护”可以作为另一个设计方案评估，但**它改变当前 ADR，不在本轮默认获准**。若只有放宽该规则才能低成本采用，要把具体差异提交为架构决策，不能暗中降低底线。加密失败拒绝写入、secret 不进日志等要求无论何种设计都保留。

[入口认证说明](https://github.com/cheaper-inference/OmniRoute/blob/ad6f33f66bda34eba98645d3bfdabbde294bc843/docs/security/INFERENCE_AUTH_POSTURE.md) 明确管理界面密码与推理入口认证独立，必须启用 REQUIRE_API_KEY；否则无效 bearer 也可能按匿名请求处理。模型目录另有访问策略，也要测试。

[环境配置](https://github.com/cheaper-inference/OmniRoute/blob/ad6f33f66bda34eba98645d3bfdabbde294bc843/docs/reference/ENVIRONMENT.md) 有 ENABLE_REQUEST_LOGS、APP_LOG_TO_FILE、pipeline capture 等开关。试点应关闭正文相关记录，扫描 SQLite、WAL、日志、错误、导出和崩溃残留；单一开关存在不证明所有路径都不落正文。

### 维护出口比功能数量更重要

近期合并记录包含不同作者的 [#18224](https://github.com/cheaper-inference/OmniRoute/pull/18224)、[#18190](https://github.com/cheaper-inference/OmniRoute/pull/18190)、[#18138](https://github.com/cheaper-inference/OmniRoute/pull/18138)。这支持存在贡献通道，不保证我们的改动会被接受，也不能由五个 PR 推算维护者规模或 SLA。

Windows 发布确实存在，但 [electron-release.yml](https://github.com/cheaper-inference/OmniRoute/blob/ad6f33f66bda34eba98645d3bfdabbde294bc843/.github/workflows/electron-release.yml) 的 Windows packaged smoke 是 best-effort；[release-acceptance.yml](https://github.com/cheaper-inference/OmniRoute/blob/ad6f33f66bda34eba98645d3bfdabbde294bc843/.github/workflows/release-acceptance.yml) 是可失败的 shadow 报告。因此不能以“有 CI”代替 Windows 启停、更新、回滚与稳定运行验收。

## 为什么不是 Magpie 或 CC Switch

Magpie 是最接近个人控制中心的竞品，但 [当前 README](https://github.com/yetone/magpie/blob/fd80ed99c9a97a2f6f96ee62e36eba9eefb16036/README.md) 明确不接受外部 PR。旧 PR 曾合并不改变当前政策。对希望少维护的私人增量路线，这是实质成本。

[provider.go](https://github.com/yetone/magpie/blob/fd80ed99c9a97a2f6f96ee62e36eba9eefb16036/internal/provider/provider.go) 将含 key 的结构序列化到 providers.json，私有文件权限不等于 Windows 凭证隔离。[middleware 文档](https://github.com/yetone/magpie/blob/fd80ed99c9a97a2f6f96ee62e36eba9eefb16036/docs/subsystems/gateway-middleware.md) 已采用进程内 JS 执行中间件，不能沿用旧报告“全部是 Bun”的描述；但自定义策略异常/超时时退回原顺序，onProvider 阶段也不能用 reject 强制阻断。插件可表达排序偏好，不能直接承担账号权限或安全存储。因此它可能需要改动核心，且更难向上游合并。

CC Switch 的 [README](https://github.com/farion1231/cc-switch/blob/ddcb111765be66ed9e7e43e240497fd2c9576688/README.md) 明确聚合模式不提供 failover；代理故障队列按工具组织，退出应用还涉及工具直连配置恢复。[provider_router.rs](https://github.com/farion1231/cc-switch/blob/ddcb111765be66ed9e7e43e240497fd2c9576688/src-tauri/src/proxy/provider_router.rs) 与 [providers DAO](https://github.com/farion1231/cc-switch/blob/ddcb111765be66ed9e7e43e240497fd2c9576688/src-tauri/src/database/dao/providers.rs) 显示这些行为和配置持久化边界。不能假定保留现有 UI 外壳便能低成本得到独立总 API。

以上 Magpie/CC Switch 深入源码是默认分支快照；已记录发布 SHA，但未完整复核其发布版逐项等价。这也限制了跨项目的精确成本比较。

## 迁移后只保留什么

需求与验收定义见 [可迁移需求包](loadout-portable-requirements.md)，可机器读取的假账号场景见 [验收向量](product-foundation-acceptance-cases.json)。

| 现有职责 | 目标归属 | 私人增量范围 |
|---|---|---|
| UI、账号管理、模型目录、基础统计、分发更新 | 上游完整产品 | 配置与少量预设；不重做 Loadout UI |
| HTTP 网关、协议转换、常规 Provider 接入 | 上游 | 不继续维护平行转换器 |
| 账号范围、顺序、轮转、同模型故障转移 | 优先上游原生策略 | 仅补已确认缺口，避免私人路由引擎 |
| secret 保护、入口认证、日志默认值 | 上游能力加个人安全配置/适配 | 一个清晰的凭证边界；覆盖失败路径 |
| 加密迁移与 Windows 启停 | 上游能力优先 | 必要的导入工具/启动封装；不是第二个控制面 |
| 尚在实验中的订阅/网页/官方 CLI 桥 | 后续单独决定 | 不把不稳定接入塞进首轮迁移；已有依赖先保留旧环境 |

形式是“上游发布版 + 个人配置包 + 必要的集中适配 + 独立验收用例”。不是把整个 Loadout 改名后接着开发。暂时可保留旧进程做回滚，但不把每个请求长期串过两个网关。

## 试点顺序与退出条件

1. **先安全可行性**：临时目录、随机端口、假凭证、独立 Windows 凭证前缀，验证所有持久化/日志/导出路径。若无法在集中边界实现当前凭证约束，停止扩大适配；评估 Magpie 的成本或提出明确 ADR 选择。
2. **再核心账号契约**：三个同模型账号，优先顺序、每请求轮转、429/额度冷却、子号池、点名账号和不换模型。点名账号默认仍可同模型failover，禁用failover或stateful固定时才不得换号；2026-10-29据ADR-003/015与源码明确此区别。关闭智能选模、自动降级、会话粘性等可能改变语义的可选功能，再检查实际行为。
3. **再协议与运行**：工具调用、流式取消/中断、有状态 Responses 固定或拒绝；关掉管理界面仍服务；重启、更新、回滚不会丢失策略。
4. **最后检查维护收益**：在两个实际发布版本间移植同一配置与补丁，记录手改文件、冲突和验收回归。能随上游升级，且无需维护私人协议转换器/账号调度器，才进入生产迁移准备。

上述是下一阶段验收，不是本轮已通过的结果。不预估未经试做的工时或“节省百分比”。每项修补要写明原生配置、插件、独立适配或核心 fork；一旦补丁横跨协议、路由、存储、管理界面多个核心层，当前底座就没有达到少维护目标，应换候选而不是继续加壳。

实际切换前仍需另立执行计划、修订受影响 ADR、形成可回滚迁移包；真实账号和流量验收只在明确授权范围内进行。

## 证据边界

- Loadout 子号池与加密导出：019/020 明确记录所有者 v0.3.0 实机测试；账号限流故障见034。
- 每请求轮转：035 有自动化回归，但明确没有真实商汤流量验证，不能将其写成全部实机证实。
- [036 CPA 评估](2026-10-29-cliproxy-isolated-evaluation.md) 的“保留主链路”针对组件替换；本轮完整产品迁移建议并不否认该结果。
- 各候选元数据、发布 SHA、文件证据索引见 [来源快照](2026-10-29-product-foundation-evidence.json)。只保存元数据和原创结论，未复制上游实现。

## 用户补充后的选型重点

2026-10-29：用户关注 OmniRoute 免费平台池能否持续更新，以及 Magpie 丝带状流向可视化的体验；CC Switch 不再作为主网关候选继续投入。以下补充不改变“少维护优先”，也不启动迁移或免费端点探测。

### 免费资源的更新与失效

OmniRoute 确实有更新机制，不只是静态免费平台名单：

- 已发布 v3.8.52 的 [Radar sync.ts](https://github.com/cheaper-inference/OmniRoute/blob/ad6f33f66bda34eba98645d3bfdabbde294bc843/src/lib/radar/sync.ts) 将下次同步设为上次成功同步后24小时；它是启用后的调度机制，不保证客户端每天运行或远端每天完成核验。
- 同版本 [FreeBudgetCard](https://github.com/cheaper-inference/OmniRoute/blob/ad6f33f66bda34eba98645d3bfdabbde294bc843/src/app/%28dashboard%29/dashboard/usage/components/FreeBudgetCard.tsx) 已显示目录 updated 时间。[summary API](https://github.com/cheaper-inference/OmniRoute/blob/ad6f33f66bda34eba98645d3bfdabbde294bc843/src/app/api/free-tier/summary/route.ts) 的日期来自 Radar generatedAt 或内置目录整理日期，不是逐平台成功推理时间。
- 当前源码 [radar-export.yml](https://github.com/cheaper-inference/OmniRoute/blob/5332fe88e800187f0d0112afa4085b5455d6ef04/.github/workflows/radar-export.yml) 配置每日目录导出和相关改动触发；导出文件生成不等于对全部平台做调用测试。
- 当前 [免费目录说明](https://github.com/cheaper-inference/OmniRoute/blob/5332fe88e800187f0d0112afa4085b5455d6ef04/docs/reference/FREE_TIERS.md) 有免费方案结束、模型退出和历史数字修正记录，且并非所有条目都具备同等新鲜的证据。README 的定期复核声明不能代替每条记录的核验时间。
- [Radar 说明](https://github.com/cheaper-inference/OmniRoute/blob/5332fe88e800187f0d0112afa4085b5455d6ef04/README.md#-omniroute-radar) 区分社区目录与支持者 live 内容；不能假定所有实时更新权益都属于免费基础版。免费额度由各平台及账号资格决定，OmniRoute 不统一发放或保证额度。

所以可以判断“有人维护、支持同步、已展示目录日期”，不能判断“每个免费平台持续可用”。用户给出的微信小程序短链未能通过公开网页核验，未评价该小程序自身更新质量。

免费资源验收增加四类信息：官方来源与规则核验日期、目录同步日期、当前账号/地区的最近成功调用及失败原因、额度重置/活动结束时间。过期或未知应明确显示；免费专用池不得在免费资源耗尽后静默转为付费。自动探测需要独立选择与额度预算，不在每次请求选路前执行。

### 流向可视化

用户偏好 Magpie 的丝带状流向呈现，应保留为产品体验目标，而不是要求移植原页面。[routing/client.js](https://github.com/yetone/magpie/blob/fd80ed99c9a97a2f6f96ee62e36eba9eefb16036/internal/gui/assets/plugins/routing/client.js) 有 source/hub/目标布局、SVG flow、token/延迟/费用指标以及重试明细，支持它不仅是静态装饰的判断。本轮未运行页面，不宣称已验证所有视觉交互。

目标是从客户端到网关、模型和上游账号看清用量及故障转移。聚合图可用桑基图表达，实时请求轨迹可另显示；带宽映射 token、请求数或费用时应标明口径。输入、输出、缓存读取/写入分别统计，不能让视觉暗示它们严格守恒。只需 metadata，不要求保存 prompt 正文。

### CC Switch 定位修正

从网关候选短名单移除。准确定位是 AI 编程工具的配置/供应商管理器，附带本地代理；它不等于多 agent 协作编排器，也不是没有网关能力。排除原因是产品主线与本次“统一、多账号、持续服务的个人网关”不一致。


