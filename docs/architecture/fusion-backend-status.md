# 融合后台功能清单与原生复用顺序

## 1.0（当前）

1.0.0 由 067 的产品化重做而来，ADR-019 已接受。下文 065 及更早的条目是历史记录，其中“隔离”“候选”“未开放”“未批准”等说法都是当时的状态。控制面在 `backend/src/lib/loadoutPhase1/control`，前端复用原版页面，经 `POST /api/loadout/rpc` 调用，方法名与原 IPC 一致；号池、总 API（`/e/<name>/v1`）、`provider/account/model` 寻址在 `policy.ts` 收窄账号范围后交给 OmniRoute 执行。过程与取舍见 [ExecPlan 067](../exec-plans/completed/067-fusion-product-rebuild.md)。下文 065 及更早的条目是历史记录，其中“未开放/只读/当前候选 064”等说法已被取代。

## 065 桌面重做状态（开发预览）

064的后台技术验收仍有效，但其界面不符合用户的桌面产品预期。065在源码中增加独立Tauri隔离入口，并重做丝带流向/侧栏：价格目录、配额缓存和预算编辑现在有独立页面；模型同步、Radar日期与同步、账号修改继续使用原生API。账号池、别名、客户端授权保持只读，本轮没有扩展路由编辑或订阅功能。没有新的后台补丁，没有新增推理调用，唯一064候选ZIP未重打。桌面窗口/托盘实跑被自动审批拦截，用户暂不便手动启动；不可将浏览器回归记作桌面验收。见[065记录](../research/2026-11-01-fusion-desktop-redesign.md)。

## 064 隔离候选验收（未正式部署）

用户已授权继续后端和 UI 开发，以下旧文中的“冻结功能/不做 UI”只描述 058 历史范围。
新增同源 React 控制台源码：原生登录、用量/调用元数据、模型到账号的累计 token 流向、
凭证更新、路由/客户端授权查看、模型目录与显式同步。Magpie 仅贡献视觉思路，
OmniRoute 继续提供 API/SQLite/CSRF/目录；没有第二套网关或统计服务。
已接入原生独立健康检查环境开关，并定位新建账号还会自动测试，增加服从同一开关的窄条件。
064 当前候选已完成实际浏览器交互自审、同包247项回归与058→064升级/回滚。
原059隔离数据已用现有加密工具升级，16账号、8路由、9密钥、3映射及凭据逐值一致；仍停止且仅本机出站。
本轮未新增后台补丁。具体证据见[064报告](../research/2026-11-01-fusion-console-progress.md)。

更新：2026-11-01。本文以当前源码、058候选最终包隔离验收及060–063有限真实验收为准。冻结新增功能，优先完成日常使用与维护收口；后续先查本表和原生实现，避免重复建设。

固定底座：OmniRoute v3.8.52，`ad6f33f66bda34eba98645d3bfdabbde294bc843`。当前唯一候选：`fusion-v3.8.52-isolated-064.1-c03260835d4c`。正式Rust/Tauri桌面端仍未替换。下文058–063条目保留历史验收边界。

059补充：真实隔离迁入已完成15个API key与1个WorkBuddy绑定，8条路由及精确权限/8RPM通过加密恢复核对，7组检查通过。按用户决定禁用默认别名、保留订阅转发停用并暂不下发完整GPT池；其余来源明确保留。候选仍058，未改运行时、未调用真实推理，隔离实例已停止且出站只允许loopback。见[059报告](../research/2026-11-01-real-binding-migration.md)。

060补充：沿用059实例，商汤原池内单账号、DeepSeek精确别名、WorkBuddy精确别名各一次短文本真实请求，3/3返回200/OK，原生记录确认账号与模型未越界。配置及vault核对恢复，实例停止且出站loopback-only，无运行时补丁；默认/订阅/GPT继续禁用。长会话、自然轮换及CommandCode未据此通过；本轮临时展开目录1.10GiB清理遭自动审批拒绝，ZIP仍只有一个。见[060报告](../research/2026-11-01-bounded-live-acceptance.md)。

061补充：约22KB历史的工具首请求3条分别得到商汤429、DeepSeek思考/强制工具选择冲突400、WorkBuddy未分类400，未重试；工具往返及真实重启续聊未通过。持续观察发现30秒后原生凭证检查自动扫描16账号，立即停止；全局间隔0不可靠，改用原生逐账号healthCheckInterval=0，保持全局60分钟，并在仅本机条件下验证停止重启前后各35秒无新增扫描、配置/vault恢复。当前实例保留该16项关闭设置并已停止；未改候选或新增副本。不要把后台服务总开关等同于禁用全部探测，也不要将连接验证401当成真实账号401。见[061报告](../research/2026-11-01-conversation-restart-acceptance.md)。

062补充：只改验收输入为原生auto工具选择、实际assistant字段原样回传和真实多轮history。WorkBuddy工具往返、约21KB上下文、重启后续聊4步均通过；DeepSeek工具调用通过，结果回传200但严格内容断言未通过，未追加调试。主动请求6次，自动探测新增0；配置/vault恢复，原实例停止且loopback-only。商汤429不复试，默认/订阅/GPT停用；未改候选、未新增副本。当前只对WorkBuddy限定链路形成完整本轮证据，不将DeepSeek断言差异归为账号故障。见[062报告](../research/2026-11-01-native-tool-conversation-acceptance.md)。

063补充：为区分格式与字段错误，仅新增测试诊断，原严格pass不放宽；22项离线反例通过。DeepSeek原账号一组2次工具往返均通过，第二次回复EXACT且3字段逐字同序正确，未复现062差异；062原文缺失仍UNKNOWN，不追溯改判。自动探测新增0，原配置/vault核对不变，实例停止/loopback-only，候选无变化。DeepSeek长会话与重启续聊仍未补验，其他外部项保持单列。见[063报告](../research/2026-11-01-deepseek-reply-assertion.md)。

## 如何理解“已经合并”

融合版采用OmniRoute后台源码作为主体，并非逐个复制上游功能。因此必须区分：

- **已接入并验收**：当前候选有可用入口，且有对应隔离用例。
- **部分接入**：核心行为已验，旧配置、认证类型或其他分支尚未覆盖。
- **底座已有、待开放验收**：源码已有实现，不能据此宣称当前候选可用。
- **暂不纳入**：有意不作为当前后台阶段任务。

当前`src/proxy.ts`只放行auth/login/logout/csrf、providers、provider-nodes、keys、combos、settings、resilience、rate-limits及loadout/vault、loadout/subscription、model-combo-mappings，以及054新增的models、provider-models和限定usage只读管理路径。新增查询仅允许管理员会话的GET/HEAD。055另开放free-models、radar/catalog/status只读入口，以及radar/settings的GET/HEAD/POST和radar/sync的POST；设置仅允许optIn。057另开放usage/quota、provider-limits和keys/:id/usage-limits只读、usage/budget的GET/HEAD/POST、providers/:id/models发现及sync-models的POST、synced-available-models读取。大量原生管理入口被候选白名单拦截；其后台内部逻辑是否运行需逐项核对，不能把“入口未开放”推断成“整个模块未执行”。不应一次性开放所有`/api/*`，也不应为每个已有API新建平行实现。

## 已接入与部分接入

| 功能 | 当前状态 | 实现来源与边界 |
|---|---|---|
| 独立后台启动、停止、状态、端口与实例隔离 | 已验收 | OmniRoute headless + `fusion/supervisor.mjs`/`runtime.mjs`薄工具；终端退出后继续运行，无安装、自启或UI。 |
| Chat、Responses、Messages协议与工具/流式处理 | 已验收限定路径 | 沿用OmniRoute；三入口、取消、部分流输出后不重放已验。当前候选明确拒绝stateful Responses，不等于上游全部协议/模态已验。 |
| Provider节点、API-key账号增删改、chat/responses上游 | 已验收 | 原生Provider管理与执行器；自有工具只传配置。并未逐一验收上游所有Provider。 |
| 多账号顺序failover、轮转、指定账号和范围限制 | 已验收限定规则 | 原生combos、策略和client key权限 + 窄范围适配；不另写Router。空子集通过禁用key处理，不能用上游空数组表示无人可访问。 |
| 有界等待、重试、限流与429后节流 | 已验收 | 原生等待/重试/限流 + 保持节流下限的补丁；接受原生语义，不复刻旧端点严格30秒总期限。 |
| SQLite与schema升级 | 已验收限定版本对 | 原生SQLite和migration；v3.8.51→v3.8.52完整组合验证，不保留RouteSnapshot技术约束。 |
| 凭证修改、删除、加密导出和恢复 | 已验收当前认证范围 | Windows OS vault/keytar + 原生账号模型；数据库仅引用，禁用普通秘密导出。已接回Loadout托管订阅登录、刷新、失效、删除与加密恢复；真实OAuth服务尚未验收。 |
| 管理鉴权、client key权限/撤销/8RPM限制 | 已验收限定路径 | 原生鉴权、CSRF、权限与rateLimits；自有Host/Origin约束与入口白名单。057补充费用预算与key日/周使用限额；原生其他配额分配功能未开放。 |
| 迁移预览、配置映射、加密备份恢复与回滚 | 部分接入 | 自有`fusion/{inventory,mapping,migrate,backup}.mjs`调用原生接口；受支持配置通过，未知策略拒绝，不静默略过账号。 |
| 原生只读模型目录、用量统计与路由诊断 | 已验收只读范围 | OmniRoute原生目录、history/latency/call-logs/route-explain/combo-trace；仅开放现有管理入口，无新采集库或Router。成本为估算，解释含重算，实际trace为短期内存数据。 |
| 原生账号配额与本地预算 | 已验收隔离范围 | 复用usage/quota、provider-limits缓存、usage/budget与keys/:id/usage-limits；费用预算和日/周限额实际拦截、未知价格保护已验。不是实时上游余额或严格预付费钱包。 |
| 原生按账号模型发现与同步 | 已验收隔离范围 | 复用providers/:id/models、sync-models与synced-available-models；更新/失败保留、账号范围和精确模型已验。不启用全局后台同步或自动改客户端配置。 |
| 原生价格目录、来源与同步 | 已验收隔离范围 | 原生四层价格、LiteLLM同步、计价器；只读目录及手动同步开放，自动同步默认关闭。lastSync不是上游发布日期，未知价不代表免费，真实节点价格覆盖未验。 |
| 原生社区免费目录、Radar日期和同步 | 已验收隔离范围 | 原生free-models与Radar签名缓存、状态和scheduler；默认opt-out，管理员可手动同步。生成/获取时间分开，过期缓存仍可读；真实托管feed与平台额度未验。 |
| 实际旧GPT池、WorkBuddy池、商汤端点 | 部分接入 | 成员顺序、模型与12账号×5模型目录已进入清单并按真实结构mock验证；订阅接线、原生别名和受限客户端策略已在mock中通过；真实绑定及未知默认别名仍需明确输入。 |
| WorkBuddy窄连接器及首行兼容 | 已验收限定路径 | dsh-workbuddy-connect选择性复用 + 精确首行函数；固定绑定账号/模型、有界缓冲SSE，非实时逐token流。自然轮换外部状态暂缓。 |

当前证据：[057报告](../research/2026-10-31-native-quota-model-sync.md)、同一最终包232项、配额/预算/模型同步22项、集中回归120项、056→057升级回滚4项。原生单元测试34项另列，另3项不兼容vault的原生fixture未计通过。056→057是同一v3.8.52底座的适配升级；053–057新增适配尚未跨另一个OmniRoute底座版本验收。mock不证明真实账号或外部数据源可用。

## 原生能力接入边界（不是自动待办清单）

以下源码路径相对于固定OmniRoute工作副本。存在实现只证明复用入口，不代表已通过我们的凭证、账号与数据边界验收。

| 功能块 | 上游实现位置 | 当前融合状态 | 后续做法 |
|---|---|---|---|
| 模型目录、自定义模型、能力与上下文覆盖 | `src/app/api/provider-models/route.ts`、`src/lib/providerModels/` | 054只读目录；057已接通按账号发现和手动同步；自定义能力覆盖写入仍未开放 | 保留原生每账号缓存，不新建模型注册中心。 |
| 模型名/别名映射到池 | `src/app/api/model-combo-mappings/route.ts`、`src/lib/db/repositories/sqliteModelComboMappingRepository.ts`、`src/sse/services/model.ts` | 管理入口与精确映射已接通；旧`loadout/default`必须由输入明确指定目标，不能自动猜测 | 优先验证精确映射和受限key权限交集；不能用`auto`默认策略悄悄改变旧范围。 |
| 用量、token、成本、延迟与历史查询 | `src/lib/usage/`、`src/lib/usageAnalytics.ts`、`src/app/api/usage/history/route.ts`、`src/app/api/usage/model-latency-stats/route.ts` | 054已开放history、model-latency-stats及call-logs；真实mock产生统计、重启/恢复可读 | 保留原生缓存和估算语义，不另建统计库；不开放正文详情。 |
| 价格目录与同步 | `src/app/api/pricing/`、`src/lib/pricingSync.ts` | 056已接通只读价格、来源、状态、dryRun/手动同步及默认关闭的自动同步；价格专项25项、升级4项通过 | 原生LiteLLM源及四层合并，无第二价格表；lastSync仅为本机时间，部分历史按当前价重算，未知价不等于免费。 |
| 账号配额、预算、额度窗口 | `src/app/api/usage/{quota,provider-limits,budget}`、`src/app/api/keys/[id]/usage-limits`、`src/domain/costRules.ts`、`src/lib/usage/apiKeyUsageLimits.ts` | 057只读配额与预算、日/周限额已验收；/api/quota批量分配仍关闭 | 按来源分开，未知/陈旧缓存不冒充真实余额；不新增轮询或账本。 |
| 免费模型目录、Radar更新与状态 | `src/app/api/free-models/route.ts`、`src/app/api/radar/`、`src/lib/radar/` | 055已接通社区目录、状态、optIn与手动/后台同步；签名loopback feed专项23项通过 | 直接沿用原生名单、缓存、验签和scheduler；无第二名单。真实托管服务与免费额度未验，付费key及相关入口不纳入。 |
| 路由解释、健康状态与实际调用轨迹 | `src/lib/usage/routeExplain.ts`、`src/app/api/usage/route-explain/`、`src/app/api/usage/combo-trace/` | 054已开放route-explain和combo-trace，沿用已有resilience | 解释含当前状态重算；真实trace内存TTL30分钟、重启失效。未来视图直接复用这些数据。 |
| 更多策略、自动组合与成本优化 | `src/lib/combos/`、`open-sse/services/combo/` | 当前只采用已验证的顺序、范围、轮转等规则 | 有明确需求再启用原生策略；不为保留旧内部算法移植Rust调度器。 |
| 缓存、语义缓存、上下文压缩 | `src/lib/cacheLayer.ts`、`src/lib/cache/`、`src/lib/compression/`及对应API | 不列为当前交付能力；未逐项审计内部运行状态 | 暂缓接入。需另验正文存储/改写与账号隔离；不自研同类功能。 |
| MCP/A2A、自动配置CLI、远程控制/隧道等 | `src/app/api/{mcp,a2a,cli-tools,tunnels}/` | 不在当前管理入口白名单 | 保留上游能力，当前阶段不扩展工具控制、客户端环境或远程部署范围。 |
| 管理界面、桌面/PWA与Magpie流向图 | OmniRoute原生API；Magpie视觉参考 | 064同源控制台已验收；桌面/PWA未做 | 原创React薄界面，复用同一后台；没有复制全上游UI或另建统计服务。 |

### 免费目录的日期与有效性

固定版本的Radar已区分`generatedAt`（上游目录生成时间）和`fetchedAt`（本机下载时间），见`src/app/api/radar/status/route.ts`。`src/lib/radar/scheduler.ts`在功能开关和opt-in允许时检查过期并触发同步；`sync.ts`已有版本/同步校验。055已接通上述原生字段与调度，无需另做更新服务。后台每小时检查，catalog在fetchedAt后24小时到期；同版本不刷新fetchedAt，旧缓存仍可读取。

但`free-models`静态接口本身不返回这些日期；Radar部分feed也没有保存生成时间。需按实际来源展示，不能补一个假的“全部最新”日期。目录更新不等于本机账号可用或平台永远免费，本机最近验证结果应与目录时间分开。签名loopback feed的真实HTTP同步、重启续接及恢复已验；外部托管服务和平台可用性未测。固定上游文档说明社区目录约延迟30天，不应把刚获取解释为当天最新。

## 旧订阅逻辑直接复用

用户已明确：现有逻辑做过，直接移植，保持行为。该项已按既有Rust行为移植为Node/TypeScript，复用原生存储和执行器；没有引入第二套Core。

| 既有功能 | 已有Loadout源码 | 融合边界 |
|---|---|---|
| 官方Codex登录状态与额度只读扫描 | `crates/providers/src/codex.rs` | 沿用后台app-server查询及session记录回退；不把扫描到的登录状态当成推理凭证，不为每次推理启动CLI。 |
| Loadout自己的受控登录与订阅Transport | `crates/providers/src/codex_subscription.rs`、`crates/core/src/oauth.rs` | 复用已有device-code登录、vault保存、刷新/失效处理、逐账号转发开关；不复制或刷新官方客户端的token。 |
| 行为与验收依据 | [ADR-012](../adr/ADR-012-subscription-transport-opt-in.md)、[订阅Provider说明](../providers/codex-subscription.md)、现有OAuth测试 | 保留原行为与用例，仅适配必要运行时接口；不为复用带入第二套网关或整个旧Core。原Rust用例通过不能冒充融合实现已验收。 |

OmniRoute也有OAuth能力，但本项不重新选型或替换用户要求保留的登录行为。053已完成上述接线。扫描仅返回登录/用量元数据，受控登录单独创建自己的凭证；默认不开启转发，逐账号双确认后启用。含托管订阅的迁移采用先登录、再map的两步流程；一次性API-key迁移仍明确拒绝，不能导入扫描到的CLI token。当前扫描是显式命令，不代表已接入常驻全Provider扫描调度。

## 已完成的实施顺序（历史）

1. **已完成本轮隔离验证**：Codex扫描/受控登录、刷新、失效、删除和备份恢复；真实登录未执行。其他订阅扫描器未在本轮一并迁入。
2. **已完成本轮隔离验证**：原生模型到combo映射、受限key与8RPM、实际旧池元数据计划。完整真实迁移仍需逐账号有效绑定及明确默认别名；不会静默删除未支持成员。
3. **054已完成隔离验证**：原生只读模型目录、用量/延迟/成本与路由诊断；没有新增数据库、指标服务或Router。
4. **055已完成隔离验证**：接通社区免费目录/Radar状态与同步，复用原生日期、过期、签名缓存和scheduler；不扩展UI。
5. **056已完成**：原生价格目录、来源、同步与成本衔接；同一最终包210项通过，原生价格53项另列；055→056属于适配版本升级。未来若更换OmniRoute底座，需对当时完整适配重新验证，不能用本轮替代。继续只保留一个候选，不自动扩展其他原生功能。
6. **057已完成**：原生配额观察/Provider缓存只读、费用预算及日/周限额执行、按账号模型发现/手动同步；最终包232项和056→057升级回滚通过，无新业务层。

## 当前结论：功能冻结，转入日常使用验收

057已经具备后台主链路和日常管理所需的主要能力，可以准备有范围限制的隔离试用；还不能据232项mock检查宣称能完整替换正式环境。当前不缺下一批原生功能，缺的是实际配置闭环、最终包实际使用证据与可重复维护流程。

### 尚未接通、但不应默认补齐

- 原生全Provider OAuth/网页认证、其他订阅扫描器、常驻全Provider扫描；当前只验选定API-key profile与Loadout托管Codex生命周期。
- 真实Provider余额主动刷新、完整quota池/组/分配管理；现有配额是只读观察/缓存，费用预算是网关统计。
- 手工价格编辑、自定义模型能力/上下文覆盖写入、全局模型自动同步；当前手动按账号同步足以支持首轮使用。
- 更多自动/成本优化路由策略、语义缓存、上下文压缩、MCP/A2A、远程控制/隧道、自动修改客户端配置。
- 图像/音频/视频/embedding等其他模态的完整融合验收，以及stateful Responses（当前候选明确拒绝）。源码或路由存在不等于已交付，也不能反向推断所有相关内部逻辑都已关闭。
- UI、Magpie流向图、安装器、登录自启和自动更新。UI继续排除；手动启动便携后台可以作为第一轮日常试用方式，安装器/自启并非默认前置条件。

上面仅在真实使用暴露明确缺口时选一项复用，不作为本轮实施清单。3项旧原生fixture与vault初始化不兼容、全量上游typecheck未覆盖，也保持证据限制；不为与实际使用无关的Provider/界面追求全库通过，更不通过关闭vault来解决测试。

### 下一步顺序与完成条件

| 顺序 | 必要工作 | 完成条件与范围 |
|---|---|---|
| 1 | 发布与构建流程收口：已完成主体 | `scripts/fusion-release.ps1`一次完成构建、封包、240项回归、同底座升级、只读预检和成功运行清理。依赖/vendor/测试已归位；只留一个ZIP。首次失败解包目录1.10GiB因自动审批拒绝删除而待清理，非主动保留基线。 |
| 2 | 指定范围配置迁移已完成 | 059已绑定15个API key和1个WorkBuddy账号，8路由与精确权限/8RPM完成真实状态恢复验证。默认别名、订阅转发与完整GPT池按用户决定停用；2只读及2未支持来源保留。无需为本轮再补功能；未来启用默认/GPT时另行明确目标和受控登录。 |
| 3 | 最终包有界日常验收 | 对真正要用的三类链路分别记录少量文本/工具/会话结果；沿用可运行客户端或标准协议探针，不继续修客户端安装环境。再做一次固定时长的隔离运行，观察资源/日志增长、断网或上游失败恢复、重启后凭证与池状态。现有备份恢复测试已通过，不另开发备份服务；仅验证这份实际配置也能恢复。 |
| 4 | 当前完整适配的上游升级演练 | 056→057仅是同v3.8.52底座升级；旧v51→v52证据不覆盖053–057新增适配。选一个合适已发布上游目标，届时核实版本，重放全部当前适配与状态升级/回滚，记录必须手工修改的位置。没有合适目标时可固定版本受限试用，但不能声称未来升级成本已证明；不连续追每个版本。 |

商汤、CommandCode和WorkBuddy同等重要，分别记账；一条成功不能替代另一条验收。外部账号故障继续DEFERRED，不在上述收口中扩大排障；若它使某条链路无法实测，该链路标记未验收，其余链路可以单独评价，但不宣称三类已全面日常可用。

步骤1–3达到后，可评价固定版本的隔离个人试用；步骤4补足长期少维护的依据。正式采用还需另行决定运行目录/凭据命名/端口与现有桌面端共存或替换方案，并修订Proposed架构决策；本轮不解除隔离保护，不动正式安装、数据或凭据。运行数据和凭据的加密备份与代码版本回滚是不同问题，Git不能替代数据恢复。

058已重新完整构建并在最终包跑过240项（既有232+预检8），21个测试实例无残留凭据或活跃进程；057→058升级回滚通过。正式旧库仅只读元数据预检，未真实调用Provider、修改正式环境、做UI或提交上传。详见[058收口报告](../research/2026-11-01-fusion-release-preflight.md)及[操作入口](fusion-release-operations.md)。

每一步先验证已有接口和默认行为，再决定是否有必要加代码。只为明确的凭证保护、账号范围或旧配置转换缺口添加窄适配；不得把上游全部功能变成本阶段必做清单。

商汤风控、CommandCode401、WorkBuddy自然凭证轮换标记为**DEFERRED**：本阶段不排查、不作为后台融合完成条件，后续实际使用再报错时单独修。客户端执行环境也不继续调试。延期不代表故障解除，更不代表正式环境可以自动切换。

053订阅行为见[订阅与原生迁移报告](../research/2026-10-31-subscription-native-migration.md)，只读接入见[054报告](../research/2026-10-31-native-readonly-observability.md)，目录/Radar见[055报告](../research/2026-10-31-native-free-catalog-radar.md)，价格接线见[056报告](../research/2026-10-31-native-pricing-sync.md)，最新配额与模型同步见[057报告](../research/2026-10-31-native-quota-model-sync.md)。本次修改隔离候选源码与工具；正式安装、正式数据及凭据均未改，无提交或上传。

057新增入口与证据见[配额、预算及模型同步报告](../research/2026-10-31-native-quota-model-sync.md)。原生管理API原样复用；命名账号默认允许同模型范围内failover，严格固定用allow_account_failover=false；key限额零值表示不限额。
