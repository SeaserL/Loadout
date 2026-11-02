# ADR-019: Product Fusion Boundaries

- **Status**: Accepted (2026-11-03)
- **Acceptance (2026-11-03, 067/068)**: 用户验收 067 的编译版，要求作为大版本 1.0.0 上传到 GitHub。从此产品主线是 OmniRoute 运行时加 Loadout 控制面，下面 067 记录的决定全部生效。接受的是产品方向和这些边界，不包括：替换本机已安装的 0.4（`loadoutd`、8317 不变）、安装器和开机自启、0.4 内置更新器升级到 1.0、上游 OmniRoute 换版本。这些需要另行决定。本节以下的 “Proposed”“未批准切换” 都是当时的记录。`crates/` 和 `apps/desktop` 的 Rust 线继续受原有 ADR 约束；受影响 ADR 对 1.0 主线不再生效的部分，以本 ADR 为准。
- **Product rebuild authorization (2026-11-02, 067)**: 用户否定 066 的界面、登录密码和功能取舍，要求编译版可直接使用。保留 Loadout 原版功能（模型分组与拖动、账号排序、号池、总 API、一键导出 API 等），接入 OmniRoute 特色能力，前端用 Magpie 设计语言。决定：①控制面放在后台 `src/lib/loadoutPhase1/control`，读 SQLite、写原生 REST，个人状态存 `key_value`，无新 schema；②去掉用户可见的登录密码，但不去掉管理鉴权：桌面壳经私有管道取得会话 cookie 并以 HttpOnly 写入 WebView，loopback 上其他本机用户和网页仍进不来，`/v1` 仍需 client key；③应用实例用 `network: direct`，测试实例仍是 loopback-only，自动探测保持关闭；④WorkBuddy 连接器放开实验期上限（真实流式、不再截断 max_tokens、多模型），用户明确授权以其真实调用验收；⑤桌面壳独立成 `apps/fusion-desktop` crate。正式环境与 8317 不变；本 ADR 仍为 Proposed，正式切换未授权。
- **Desktop redesign authorization (2026-11-01, 065)**: 用户要求重做为桌面软件，明确借鉴Magpie丝带流向与OmniRoute细分管理。复用Tauri/WebView2但新增独立隔离入口/config/profile，禁止进入旧正式daemon Bridge。开发期loopback静态预览和受限管理API代理不进入推理链；发布仍使用后台同源页面。064技术验收不作为视觉或桌面产品验收，正式切换仍未授权。
- **Console trial authorization (2026-11-01, 064)**: 用户授权自主补齐后端并开始 Magpie 风格 UI，可放弃旧布局。隔离控制台同源复用原生管理 API、cookie/CSRF 与 SQLite；不引入第二个网关。自动凭证健康探测使用原生独立开关永久关闭。正式环境不变，默认别名/订阅转发/GPT 池仍停用；Proposed 不代表批准生产切换。
- **Date**: 2026-10-29
- **Scope**: 1.0 主线的架构边界；不改变 Rust 线的实现与它的 ADR。
- **Trial authorization (2026-10-30)**: 用户授权ExecPlan 039隔离验证独立后台、凭证和账号规则；允许仅在实验副本验证替代实现，不授权生产迁移。Proposed状态保留至结果评审。
- **Extended trial (2026-10-30)**: 用户授权ExecPlan 040的完整后台包、实际跨版本升级与SQLite热路径实验；仍不做UI、不动正式环境，Proposed状态不变。
- **Reassessment (2026-10-30, 041)**: 用户明确优先少维护而非原实现，要求先评估接受原生SQLite、保留凭证与账号规则的路线。当前推荐以下述原生状态模型方案替代“必须保持RouteSnapshot”的候选前提；仅评估，未部署、未修改正式Accepted ADR。
- **Native-state trial authorization (042)**: 用户授权继续隔离验证原生SQLite，免除实验中的RouteSnapshot/零SQL实现约束；补齐凭证生命周期、实际协议账号规则和跨版本成本。允许新实验副本开发与mock运行，不做UI、不部署、不访问正式凭证；正式实现仍遵守其既有ADR。
- **Design**: [三项目融合架构](../architecture/loadout-fusion-design.md)
- **Trial evidence (2026-10-30)**: [039隔离验证](../research/2026-10-30-omniroute-phase1-validation.md)。主要行为可用小补丁实现；真实融合包、内存热路径与低维护升级门槛仍未通过，因此保持Proposed，不批准生产替换。

## Context

2026-11-01 ExecPlan 066：用户授权收口完整隔离桌面候选包及窗口、托盘、启动退出验收；不新增功能、不动正式环境、不提交上传。继续采用已编译 OmniRoute 后台与静态同源控制台，桌面启动复用既有 Instance 管理工具。关闭窗口隐藏至托盘，退出控制面不停止独立后台。原生交互被审批阻止时必须记录未验收，不以浏览器或后台测试代替；候选可保留待人工验收状态，不批准正式切换。


2026-10-31 ExecPlan 052：用户授权继续实际旧池映射与包含全部适配的跨版本隔离升级验证。允许只读正式路由元数据；不读取正式secret、不执行真实推理或客户端调试。仅保留一个最终候选包，旧版本只作临时升级/回滚基线。官方最新仍v3.8.52，采用v51→v52重验当前完整组合，未来版本仍未验收。无可用安全Transport的成员保留显式阻塞，不自行删除、替代或扩大认证适配。

2026-10-30 ExecPlan 050：用户授权将已验证组件整理为统一隔离后台包，补正常启动/管理/迁移备份和最终包回归。候选构建凭证保护固定启用，移除实验控制接口；薄管理工具复用原生鉴权API。独立Node监护进程使用Windows隐藏、detached及忽略继承stdio的启动方式，不引用Rust生产进程实现。仅隔离便携目录和测试vault，不新增安装/自启/UI，不读正式账号；保留外部阻塞和客户端环境未验收状态。接受WorkBuddy有界缓冲转发作为本轮已声明能力，不扩大提示词改写。正式ADR状态不变。

用户要求把Loadout已经验证的需求与个人使用习惯放到完整开源产品上，接受更换界面与操作方式、优先少维护；选定OmniRoute和Magpie进入融合设计。简单串联三套网关会留下三份账号、路由、凭证、协议与升级责任，违背目标。

源码表明OmniRoute可提供主体，但默认凭证、客户端key持久化、quota preflight和Electron生命周期并不自动满足Loadout契约。Magpie流向页依赖自身插件/DOM/trace，不是可直接加载的React组件。

## Decision

提议以OmniRoute派生代码为唯一推理运行时和管理应用主体；Loadout提供安全实现的必要子集、账号规则与底座无关验收；Magpie提供流向视图、交互与真实轨迹模型的选择性借鉴/移植。

1. 只保留一个实际选择账号、维护健康和发送推理请求的执行链。自有policy模块只编译约束，不是第二个router。
2. OS凭证存储、数据库secret_ref、client key hash-only和默认不落盘正文保持为迁移门槛。拟采用窄Windows原生绑定；不得因为Node集成困难而默认改用明文或DB可逆密钥。
3. 可视化只消费脱敏runtime事件，不访问凭证、不调用Provider、不重新决定路由；事后recompute必须标记为分析，不是历史事实。
4. 免费目录、规则证据、本账号可用性分别建模。Radar可选，目录更新不能改变账号授权或免费专用模式。
5. Windows用户级后台独立于GUI。拟使用本地Web管理API替代Named Pipe专属管理，但需完整的管理鉴权、Host/Origin/CSRF与会话保护。
6. 原有Rust/Tauri主体、内部canonical类型、RouteSnapshot具体机制的替代，需要阶段B/C证明契约与维护收益；不能靠文档宣布兼容。
7. 源码脚手架只是参考，不直接进入构建、发布或父仓库Git索引。实现另立计划和受版本管理的工作区。

受影响ADR：001/002/003/005/006/007/010/011/015/018。ADR-004中的Rust内存清零能力在Node字符串中不自动成立，需专门评审；不以本提案默许降低该保证。持久化与日志底线不放宽。

## Alternatives

- 保留Loadout全控制面并代理到OmniRoute：会持续维护两套路由和账号状态，不选作最终架构。
- Magpie做主体再嵌OmniRoute：引入双运行时和双路由，且其当前PR政策不利于私人增量维护。
- 一次性复制两个仓库的大量模块：失去上游升级路径，重造大型私有产品。
- 继续完整自研Loadout：保留稳定版作回滚，但不作为本轮主发展方向。

## Consequences

新增的主要责任是凭证绑定、有限policy接缝、metadata事件适配与React视图。它们不是已经存在的公开插件API，补丁规模需要验证。

当前请求的交付是设计与源码准备。提案接受后仍需依次完成安全/生命周期、账号契约、可视化、资源新鲜度、两版本升级与迁移验证。若必须重写上游协议/路由，或需保留完整第三运行时，重新评估，不以已投入工作为由继续扩张。

## ExecPlan 040 evaluation (2026-10-30)

完整后台包与v3.8.51→v3.8.52数据升级可运行，核心安全/账号adapter仍小。但SQLite写锁实验证明选路前熔断器持久化及选路中的租约清理会同步阻塞；请求内缓存不满足现有内存快照边界。按当前契约和“少维护”目标，040报告建议不继续三项目深度融合，不进入UI或正式迁移。本ADR保持提案状态，未放宽ADR-004及热路径规则。详见[040验证报告](../research/2026-10-30-omniroute-package-upgrade-validation.md)。

## Native-state candidate after reassessment (041)

041修正上述推荐的前提：不再把RouteSnapshot、选路前零SQL、旧语言/IPC/命令/寻址语法或原延迟预算当作候选底座必须复刻的条件。接受OmniRoute原生SQLite、schema/migration及状态生命周期，通过实际请求延迟、并发、锁恢复与正确性判断是否适用。候选只维护集中凭证边界及缺失的账号语义，Magpie呈现不阻塞主路线，不增加第二套Router。

应用普通持久化与导出不带可恢复凭证（OS保护存储除外）、client hash-only和账号范围/同模型/首选与固定规则仍需验证。Node不提供原Rust drop清零承诺的差异应明确记录，而非为追求形式一致重建Rust运行时。041发现原生已认证JSON导出、凭证删除/撤销/恢复生命周期及多协议覆盖仍需补齐；没有把201行实验模块当作正式完整实现。

**推荐继续最小适配的隔离验收，不因旧架构约束退回全量自研。** 本节仍为Proposed，正式Loadout继续遵守既有ADR；未来实际采用须修订相应约束。评估详见[041报告](../research/2026-10-30-omniroute-native-sqlite-assessment.md)。

## Native-state validation (042)

原生SQLite路线完整后台目录实跑132/132通过；凭证修改/删除/加密导出/恢复、三个客户端协议账号规则、v51→v52实际数据升级及配套vault回滚已覆盖。新版自有适配430行，14个上游文件+91/-17；旧补丁不能无冲突直套新版，仍须维护受控fork。独立原生队列配置对照28/28通过，350ms主要来自默认主动限速，改变原生设置即可降到测试单请求61–80ms，无需重构数据库。

建议继续OmniRoute为唯一运行时的融合路线；正式迁移仍未授权。支持范围为受限API key profile、单实例、loopback mock；OAuth、stateful会话、真实客户端/Provider及其他管理旁路不能据此宣称通过。详见[042报告](../research/2026-10-30-omniroute-native-sqlite-validation.md)。本ADR保持Proposed；040依旧保留为旧约束下的历史结论。

## Real-client trial authorization (043)

用户授权固定042已通过的v3.8.52基线，验证真实客户端与Provider并开发迁移、备份、回滚工具；商汤/CommandCode、WorkBuddy与OpenCode桥接同等重要。允许只读源配置与按ref读取选定Loadout API key，复制到独立测试OS vault，执行明确opt-in的有限合成推理；不修改正式环境、不复制外部管理OAuth、不绕过桥接冷却/风控、不做UI。迁移、恢复与回滚仅在新隔离实例执行。本授权不等于正式切换或放宽凭证安全底线。

043结果：仍保留OmniRoute为候选唯一运行时。自定义Provider仅需增加prefix/nodeName两项白名单；官方客户端协议和工具mock通过，免费桥接真实文本及加密恢复后真实文本通过。CommandCode 401、商汤429、WorkBuddy缺少官方IPC及未迁移的池/端点行为阻止生产验收。原生executor的同URL重试与全局requestRetry不同，需在正式迁移前对齐。详见[043报告](../research/2026-10-30-omniroute-real-client-migration-validation.md)。不因外部链路阻塞退回全量自研，本ADR仍为Proposed。

044保持运行时冻结，仅开发离线映射与mock验收。原生combo可保持旧手工Pool顺序、限定账号和立即failover；商汤旧端点级等待与请求起点30秒deadline不能由全局comboCooldownWait等价表达，已由反例确认。迁移工具对此保持BLOCKED，没有新增私有等待循环、改全局规则或要求正式切换。详见[044报告](../research/2026-10-30-omniroute-routing-mapping-validation.md)。

## Bounded native wait authorization (045)

用户明确接受OmniRoute原生全局等待及累计sleep预算，不再要求旧端点从请求起点严格30秒deadline。候选可通过显式native-bounded-v1配置启用有界等待；账号scope与精确模型边界不变，旧计划不可静默升级。仅隔离验证，正式ADR-015与正式环境不变。用户确认WorkBuddy已登录、商汤额度正常，并授权排查CommandCode401；限量官方调用用于诊断，不扩展认证权限、不绕过风控。

045结果：核心17项与编译器16项通过；追加429后节流反例失败（配置20秒minTime、Retry-After2秒时实际2127ms后同账号重试）。接受原生等待不意味着接受失效的节流承诺，保留该独立迁移门槛；运行时补丁增加0。CommandCode官方直连401/EXPIRED，WorkBuddy IPC已恢复但推理400/code11128。继续隔离融合，不正式迁移。详见[045报告](../research/2026-10-30-omniroute-bounded-wait-provider-validation.md)。

## Narrow pacing and WorkBuddy connector trial (046)

用户授权优先检查上游429节流修复，必要时仅在隔离副本评估最小patch，不重写路由；CommandCode由用户处理。随后明确指定尝试AidenOrz/dsh-workbuddy-connect解决WorkBuddy连接错误。允许在独立诊断进程复用该项目的本机凭证只读解密与请求头构造，仅在内存使用当前登录账号的access token，固定官方域名、原绑定账号、原模型及合成输入；不安装DSH、不改桌面端、不复制凭证到磁盘、不自动刷新或写回、不启用提示词指纹改写。此范围替代043对本次实验的外部OAuth读取限制，不改变正式运行时授权。对失败只记录状态/错误分类，禁止原始凭证、响应或用户正文落盘。

046结果：候选新增rateLimitManager一个接缝+25/-1，相关56单测、完整包23项通过，429后同账号间隔恢复20秒且25秒hint不缩短。WorkBuddy指定项目的内存凭证/请求头组合经冻结后台与独立bridge真实返回200/LOADOUT_OK；不启用其刷新落盘及提示词改写。建议继续窄适配；候选不自动替换冻结包，真实商汤和WorkBuddy完整客户端能力仍待验收。报告见[046](../research/2026-10-30-omniroute-pacing-workbuddy-connector-validation.md)，本ADR仍为Proposed。

## Daily-use isolated trial authorization (047)

用户授权继续实际客户端、工具、长会话和凭证轮换验证，以及修复节流规则下少量商汤真实调用。实验可将WorkBuddy旧文本bridge替换为固定账号/模型的薄HTTP relay，工具/协议映射交给既有OmniRoute；允许复用上游内存key provider观察同账号加密文件更新。不得修改正式bridge或强制刷新真实OAuth，不改提示词、模型、路由/凭据底线，不以mock代替真实轮换证据。采用046完整候选，商汤单账号最多3次、WorkBuddy最多16次，拒绝后关闭对应出口；正式环境保持原授权边界。

047结果：Claude Code/OpenCode受限文本和只读工具、三轮149KB完整history通过；Codex真实渠道拒绝，商汤首请求429停止。17项离线凭证/relay/加密文件轮换通过，但真实token未轮换。后台新增hook0，原046patch不变，仍不批准正式替换。详见[047](../research/2026-10-30-daily-client-credential-provider-validation.md)。

## Exact first-line compatibility trial authorization (049)

048的15次隔离对照定位Codex首条system身份开头触发11128；仅该行应用OmniRoute既有规则，完整实际请求重放成功。用户随后明确授权实现这个最小范围并验证真实Codex文本、工具和长会话。本次允许实验WorkBuddy连接器显式启用**第一条system首个文本部件的已知首行精确匹配**，输出固定为048已验证替换结果；其余指令、用户消息、工具schema/参数/结果必须不变。此授权仅替代046/047对该隔离适配的禁止提示词改写限制，不允许全body替换、模糊匹配或扩大词表，不变更正式运行时。遇到新阻塞保留失败证据，不为测试扩大适配。

049结果：27个非空行纯函数、relay显式开关；22项范围/工具验收检查、原12项凭证relay回归通过。真实Codex文本、持久get_goal、三轮同session204686 B长会话通过；文件读取因本地执行策略blocked by policy仍BLOCKED，未放宽权限。真实WorkBuddy共12次，后台hook新增0，正式环境未改。建议保留窄适配，但不将有限客户端通过提升为生产迁移批准。详见[049报告](../research/2026-10-30-workbuddy-first-line-client-validation.md)。

## Unified isolated distribution result (050)

2026-10-31：固定v3.8.52的便携隔离包完成，凭证保护固定启用、lab移除，正常原生管理API和启动/迁移/加密备份恢复工具随包。仅新增settings及proxy两个上游运行时接缝，总计17个；不更换SQLite、不新增Router。管理入口增加本地Host/Origin要求，WorkBuddy保持精确首行和有界缓冲。最终包120/120检查通过，凭据与进程清理通过；真实Provider调用和真实客户端启动均0。

受支持的完整合成迁移通过不等于实际旧池已全部映射；下一版包含全部增量的升级验收、外部账号可用性及真实自然轮换仍单列。继续窄融合隔离试用，不批准正式切换，本ADR保持Proposed。详见[050交付与证据](../research/2026-10-31-isolated-fusion-package.md)。

## Full adapter upgrade result (052)

后续范围澄清（2026-10-31）：用户要求直接移植既有订阅用量扫描与Loadout受控登录逻辑，保持已有行为，不重新设计认证流程；三项外部账号事项延期，不作为当前后台融合完成条件。继续融合应优先复用OmniRoute原生功能，先核对[功能清单](../architecture/fusion-backend-status.md)，不把源码存在等同于已开放/验收。不做UI、不动正式环境、不提交上传；本次范围记录未改变运行时。

2026-10-31：实际旧池结构和5模型×12账号商汤端点已进入显式迁移inventory；订阅凭证profile及受限客户端虚拟别名仍阻塞全量激活。以完整适配重验已发布v51→v52，193→209条原生migration及匹配旧包回滚、三协议路由/凭据/8RPM共15项通过；最终ZIP120/120通过。新增内容仅在外围工具：协议参数、完整模型覆盖、限定版本升级、旧版字段兼容、包内migration路径固定。当前上游运行时17接缝未增加。旧版401延迟一项失败保留，未来正式版本尚未验收。建议继续窄融合，仍不批准生产切换；不扩展UI/客户端调试。详见[052报告](../research/2026-10-31-full-adapter-upgrade.md)。

## Existing subscription behavior port authorization (053)

2026-10-31 ExecPlan053授权：直接移植既有codex只读扫描与codex-sub受控登录行为，允许在当前隔离底座增加窄订阅生命周期和Responses规范化接缝，使用原生model-combo-mappings接旧别名。订阅access保存在原生apiKey字段的vault引用中，refresh使用原生refreshToken引用，credentialProfile区分托管登录；并不把官方CLI token作为普通API key导入。保留逐账号opt-in、默认并发2、后台刷新和失效停重试。无新schema、无第二网关；正式ADR状态不变。

053结果：Codex扫描/受控登录行为移植、原生别名与受限key迁移在最终包通过；120项集中回归、18项专项、4项适配版本升级回滚通过。运行时接入点17→19，无新schema/Router。自有工具补原生权限PATCH及停机锁文件竞态处理。真实OAuth及新增适配跨未来上游版本未验证，仍不批准正式切换，详见[053报告](../research/2026-10-31-subscription-native-migration.md)。

## Native read-only management authorization (054)
用户授权在固定隔离底座接通模型目录、用量和路由诊断。仅开放选定原生接口GET/HEAD，保持管理员会话边界、正文不落盘和原生统计模型；不更改正式ADR-008，不开放目录写入/同步或详细正文。解释重算不得当成实际trace，目录与成本不作为真实可用性/账单承诺。

054结果：最终同包只读20/20、订阅18/18、集中回归120/120、053→054同底座升级/回滚4/4通过。保留原生SQLite、统计写入和路由，运行时仍19个上游接入文件，本轮仅在既有proxy增加12行，另限定外围admin查询参数和升级版本对。用量不覆盖缺少usage的成功非流式响应；route-explain含重算、combo trace重启失效。未知未来底座兼容与正式环境仍未验收，本ADR保持Proposed。详见[054报告](../research/2026-10-31-native-readonly-observability.md)。

## Native free catalog and Radar authorization (055)
用户授权接通原生免费目录和Radar更新状态、过期及同步机制。继续固定隔离底座，复用原生签名feed、SQLite缓存、日期、merge和scheduler，不维护第二名单。允许仅启用原生Radar后台调度，通用后台服务继续关闭；默认opt-out。仅社区feed，不新增supporter凭证生命周期；允许公开feed URL/验签公钥配置，保留出站白名单。用loopback签名mock验证，不触碰真实账号/正式环境，无UI/提交上传。缓存可读不等于新鲜或平台可用。

055结果：同一最终包Radar23、只读20、订阅18、054→055升级/旧包回滚4、集中回归120项通过；原生Radar118项单独通过。上游运行时文件19→20，新增settings严格输入限制，其余为既有proxy及自有启动/外围工具接线；无新schema、Router、名单或同步器。原生社区feed文档描述约30天延迟，超过24小时的缓存仅触发刷新检查而不自动隐藏；真实托管服务未测。继续窄融合，不构成生产切换授权，本ADR保持Proposed。见[055报告](../research/2026-10-31-native-free-catalog-radar.md)。

## Native pricing catalog and sync authorization (056)
用户授权接通原生价格目录、来源、更新时间和成本衔接。沿用原生合并、LiteLLM同步与计价器；只读管理入口及手动同步，自动同步默认关闭并通过停机实例配置显式启用。无第二价格表，不开放手工改价/删除，不启动其他后台服务。固定隔离底座和出站限制不变；测试替换只在实验进程网络边界，发行包不含测试feed开关。无UI、正式环境变更、提交上传。

056结果：同一最终包价格25、Radar23、只读20、订阅18、升级4和集中回归120项全部通过；原生价格53项另列。上游运行时文件仍20个，仅扩展既有入口，自有启动/工具连接原生同步。lastSync不等于上游发布日期，空有效数据回退、未知价为0及部分历史重算均保留原生语义；真实源和实际旧池价格覆盖未验。继续窄融合，不构成生产切换授权，本ADR保持Proposed。见[056报告](../research/2026-10-31-native-pricing-sync.md)。

### 057 原生配额、预算与模型同步（隔离授权）

用户授权直接复用原生配额只读、预算与限额执行，以及模型发现/同步。区分 Provider 缓存、限流观察与本地成本；接受原生预算窗口和事后统计限制。仅手动按账号同步，不启用全局后台任务、自动修改客户端配置或自动修剪池。仍不批准正式切换。

057最终包232项通过，另34项原生单测通过，3项旧fixture因强制vault初始化不兼容单列；056→057保留预算/配额缓存/目录并可回滚。仅扩展入口，无新业务补丁；不代表所有Provider或下一上游版本已验收，亦不批准正式切换。见[057报告](../research/2026-10-31-native-quota-model-sync.md)。

### 058 发布收口与实际迁移预检

用户授权冻结新增功能、统一构建/打包/回归并只读核对实际旧配置。058未改变上游运行时补丁；统一入口和既有测试迁到稳定路径，固定实际Node24.22.1并校正许可证。最终包240项通过，057→058同底座升级回滚通过，21个测试实例审计无凭据/进程残留。实际旧库21账号/2池/1端点仍待新后台真实绑定及显式别名，预检不读取正式vault、不应用迁移。仅一次失败解包目录清理遭自动审批拒绝，例外单列。外部账号事项延期、无UI/正式切换/提交上传，本ADR仍Proposed。见[058报告](../research/2026-11-01-fusion-release-preflight.md)。

### 059 真实绑定与用户选择

2026-11-01：用户授权在隔离实例只读迁入旧API-key凭据、按原账号和模型映射精确别名，并明确默认别名先禁用、旧订阅转发保持停用及完整GPT池暂不启用。通过原生API与既有compile/applyReady对明确收窄投影应用，其余来源和整池保留在完整迁移记录；不绕过缺成员门禁、不复制正式订阅refresh token。058包无变化，16绑定/8路由及真实状态备份恢复通过，网络保持loopback-only、实例停止。无真实推理或正式环境变更。见[059报告](../research/2026-11-01-real-binding-migration.md)。
