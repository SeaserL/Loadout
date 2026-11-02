# ExecPlan 050 — 统一隔离后台包

## Goal
交付脱离实验目录运行的 Windows headless 隔离包，包含启动/管理、迁移映射、加密备份恢复，以及针对最终产物的集中回归。优先少维护。

## Context
依据 readiness 清单和用户本轮授权。OmniRoute v3.8.52 固定不变，复用042–049成果。外部商汤429、CommandCode401及客户端执行策略保留独立状态，不做真实调用或客户端调试。

## Existing behavior
phase7完整后台含节流补丁；phase8/10连接器分散；phase4/5工具依赖仓库路径、实验控制接口和硬编码Python。凭证保护仍依赖实验开关。

## Constraints
无UI、不触碰生产安装/数据/凭据/客户端。仅新隔离目录、随机测试vault、loopback端口。保留原生SQLite与路由。测试只用合成账号和mock，不读取正式会话。不得对旧池静默丢弃成员。

## Architecture impact
以现有后台为唯一router；薄Node管理工具调用既有鉴权API，独立监护进程提供start/stop/status。安全profile固化于候选构建并移除实验控制路由。WorkBuddy仍是窄连接器；不复制路由。ADR-019补充本轮范围，不改变正式Accepted ADR。

## Files expected to change
`fusion/`可发布源码与说明；`development/experiments/omniroute-phase11/`构建/回归/证据；新隔离上游worktree；ADR-019、reference-map、THIRD_PARTY_NOTICES及本计划。

## Implementation stages
1. 冻结来源，准备新隔离源码，固化安全profile和管理边界。
2. 抽取可发布运行/管理/备份/迁移工具及WorkBuddy连接器。
3. 构建、统一打包、解包到新目录并绑定完整manifest。
4. 最终包mock回归、针对性性能检查、凭证/进程清理，记录边界。

## Tests
启动独立性、重复实例/端口归属、正常停止/异常恢复；未经认证管理拒绝、实验入口不存在、凭证生命周期与无明文；三协议账号/模型/重试/429/工具/取消；迁移阻塞和显式映射、加密备份/错误口令/篡改/恢复回滚；WorkBuddy合成轮换与首行范围。测试通过真实包正常管理入口，不重开lab接口。

## Performance validation
复用原生队列配置，记录mock下最终包请求延迟及429节流反例回归。不声称网络/模型性能或旧Rust预算达标。

## Security considerations
安全默认不可因关闭实验标记而失效。拒绝普通文件存secret、未知认证形态和测试控制旁路。管理秘密从OS vault读入内存；CLI秘密通过隐藏输入/私有stdin。归档AES-GCM+scrypt，只在停止实例后捕获。操作仅限显式隔离归属目录；停止通过经认证的控制通道，不盲杀PID。

## Migration considerations
保留旧生产。迁移预览和映射可单独运行；新目标拒绝覆盖、检查来源指纹和完整成员。实际账号暂不迁入。rollback恢复到新目录，与包版本匹配，不逆向改schema。跨上游新版升级不在本轮授权的统一打包范围内，保留后续门槛。

## Known risks
依赖复用不是洁净联网重装证明；Node字符串无法保证零化；WorkBuddy官方私有接口有上游变化风险；现有mock成功不代表外部账号恢复。普通Windows用户同权限进程不是本工具安全隔离边界。

## Definition of done
最终压缩包可解包独立运行；不引用开发目录；正常管理与迁移/备份入口可用；集中回归有逐项回执和失败保留；测试凭据与进程清理；报告明确不批准生产替换及未验收外部项。

## Progress
- [x] 读取要求、ADR、现有工具与接缝；确认复用原生API/SQLite和现有连接器。
- [x] 新候选源码和安全profile：新worktree，保护固定开启，移除lab端点，管理API允许列表，安全设置不可关闭。
- [x] 可发布工具与连接器：随包Node启动/管理/迁移/备份，WorkBuddy配置与窄relay；默认loopback，真实出口须显式主机允许列表。
- [x] 初版构建、独立解包：21747文件，395614699字节ZIP，manifest绑定050.1-79f84b0aff4a；下述安全修复后由新包替代，旧证据保留。
- [x] 最终包构建、独立解包：21748文件，395616514字节ZIP，releaseId为fusion-v3.8.52-isolated-050.1-45d576e8b7ef。
- [x] 集中回归、性能、清理与报告：120/120，24个本轮实例测试凭据和所属后台进程无残留；报告见2026-10-31-isolated-fusion-package.md。

构建进展：官方backend-only构建退出0，保留typecheck跳过与依赖复用限制；统一目录21747文件，包含随包Node和WorkBuddy模块。已进入压缩/新临时目录解包步骤，尚未将源码检查计为最终包回归。

回归进展：随包首行范围10项、连接器13项通过。首轮core发现测试误用PUT调用原生PATCH key权限接口，保留失败回执；仅修测试方法，不改发行包。正常启动/端口冲突/监护退出恢复、管理安全、三协议文本与工具、凭证生命周期已实跑。集中套件继续使用同一归档/manifest。

第二轮：core与23项节流/恢复矩阵通过。真实发现原生管理允许“有效session+CSRF+外站Origin”，在现有proxy接缝增加本地Host/Origin门禁，重新构建最终包；不另写鉴权。WorkBuddy链路schema差异查明为原生添加空properties及additionalProperties=true，原描述未变；按明确的原生补全结果断言，保留输入输出证据，不修改协议或首行函数。迁移元数据增加只读事务，批量迁移接受与正常配置相同的显式网络配置，默认仍仅loopback。

最终回归：新ZIP全部8组运行；仅Host测试因Node fetch替换自定义Host失败。loopback回显确认测试驱动问题，改原始HTTP后只重跑management组8/8，其余7组沿用同一未变更最终包结果。最终审计记录原始suite和定向重跑来源，重新校验全manifest与ZIP hash，合计120/120。core本地完整HTTP mock 20次p50=76.48ms、p95=81.10ms；非模型性能。真实Provider调用0、真实客户端启动0。

交付完成范围为隔离便携包、受支持迁移工具与集中回归。实际旧池绑定、外部账号及自然轮换、当前全部增量下一版升级未验收，单列在[交付报告](../../research/2026-10-31-isolated-fusion-package.md)。没有UI、正式部署、生产配置修改或提交推送。

## Decisions
- 不运行真实客户端或Provider。正式环境不作为迁移测试源。
- 第一版为便携隔离包，不加注册表自启、安装器或自动更新框架。
- WorkBuddy保持有界缓冲转发，明确限制；去除实验总调用预算，失败使用有界冷却，不自动换模型/账号。
- 迁移凭证使用隐藏/私有stdin输入，不移植Python/CredRead系统扫描器；不自动复制正式vault。旧源不支持的policy/成员明确拒绝，完整映射后才建目标。
