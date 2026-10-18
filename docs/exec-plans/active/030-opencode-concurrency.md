# ExecPlan 030: OpenCode 并发与连续 user 消息

Status: BLOCKED_EXTERNAL (isolated implementation; not deployed)

## Goal
隔离修复并验证多请求并发及连续user消息，通过真实编码后更新正式环境。
## Context
用户授权隔离先行、通过后部署。029已正式验收；手机热点可用。
## Existing behavior
单Official全局MCP目录与控制状态，非阻塞锁导致并发429；parser拒绝相邻user。
## Constraints
官方binary不改，保留原生工具、权限ask，不执行上游工具；请求正文不落盘。真实测试opt-in，独立数据/端口/pipe/secret prefix。
## Architecture impact
Provider内部桥接改为两个独立常驻官方worker、有界队列；不改Router。各worker隔离workspace/config、内存DB、MCP和session状态。控件/MCP按session或独立secret找到worker。连续user保留原内容与顺序导入。
## Files expected to change
opencode_bridge.py、protocol、Start脚本、probe support、tests、ADR017及provider docs。
## Implementation stages
1. 独立worker池、生命周期及消息顺序修复。
2. 离线队列/取消/隔离/协议测试。
3. 并发不同工具schema、参数、连续user及双编码任务实测。
4. 备份部署、正式验证、失败回滚。
## Tests
worker独占、排队/溢出/取消、控制/MCP路由，连续user完整历史；真实2并发有重叠且互不串结果；编码独立测试。
## Performance validation
测并发重叠与完成时间、常驻worker数；Provider admission 从1改10，补跑 route_selection benchmark，Router/Gateway 算法不变。
## Security considerations
独立worker MCP secret；控制Basic只匹配活动session；取消只清理所属worker；队列有界、等待超时，失效worker不复用。
## Migration considerations
binding增加official_ports，保留official_port向后兼容；生产备份binding及脚本，旧版本回滚需停所有新增worker。
## Known risks
资源增加、上游限制、相邻消息官方语义、进程清理与schema串用。任何未通过不部署。
## Definition of done
两项限制已隔离修复并完成真实并发编码与生产验收，部署可回滚。
## Progress
- 2026-10-18 11:17 UTC：官方无插件 CLI 单次 HELLO 同模型返回403 Detect Abuse Traffic；随后候选隔离桥接首个连续user请求的原生事件也为同一403。WLAN仍为用户手机热点。无法确定IP、频率或其它上游策略，不能归因并发实现。停止后续真实并发/编码验收，不部署。
- 已完成24项Python离线测试及1项官方hook Node测试；Provider两项Rust测试、fmt、clippy通过，独立target/release构建成功。覆盖连续user原文顺序、worker独占、队列溢出/超时/取消、失效worker及control/MCP隔离；这些不替代真实并发编码证明。
- 最新回执保留stage与两个worker原生错误，失败时不丢中间结果。两个隔离worker均session=0/MCP=[]，隔离进程清理；正式029进程和脚本不更新。
- 待上游官方单次调用恢复，再完成2并发同名不同schema工具往返、3请求排队、取消隔离及双真实编码任务；通过后才可执行生产备份/部署/验收。
- 首次实测被Provider account_concurrency=1挡在bridge之前，连续user已成功。需同步更新Provider声明为10（2执行+8等待）并独立构建，保留旧失败回执。此改动位于Provider，不改Router算法，但补跑路由benchmark验证并发配置回归。
- 完成入口、现有状态及代码检查；采用独立常驻worker避免共享MCP目录串用。
## Decisions
- 初始两个worker、最多8等待；超载明确报错，不承诺无限并发。
- route_selection复测中位估计：provider/model 523.93ns，virtual/session 7.3875us，exact 236.02ns，web gates 527.25ns。首轮web gates 732.26ns较旧基线回退，停止其它构建/实测后复跑回到527.25ns；不将一次忙时波动归因本改动，也不将离线路由性能代替上游并发吞吐证明。
- 正式IPC确认daemon PID176208仍运行；官方216256/Python220232/GUI210668均未替换，五个部署脚本SHA与029一致。新增worker只存在隔离测试，无遗留测试官方进程。

## Low-frequency diagnostic follow-up
2026-10-18 用户要求继续诊断热点恢复后再次403。只运行官方无插件、独立空目录/内存DB的同模型HELLO，最多3次、相邻开始至少120秒，不做桥接或并发请求，不改网络或正式配置。记录本机UTC和结果，并读取正式日志metadata检查背景流量。此实验可观察短期恢复，不能单凭403或恢复证明频率/IP/并发因果；官方客户端内部HTTP重试次数和其它设备流量没有完整观测。

### 低频官方对照结果（2026-10-18）
固定官方2.0.32、deepseek-v4.1-flash-free、无plugin、独立空目录/内存DB，prompt仅HELLO；没有运行桥接/并发测试或修改正式服务。UTC 11:20:03.756失败403（1.962s）；间隔168.109s，11:22:51.865失败相同403（5.213s）；再间隔134.882s，11:25:06.747成功HELLO（3.238s）。手机热点连接名未变，但没有公网出口IP前后记录，不能排除运营商出口变化。当前正式日志最后OpenCode请求仍为11:06:11.732，daemon及官方/Python进程保持原PID。其它桌面客户端、其它设备流量及官方CLI内部HTTP重试次数未完整观测。

结论：本轮观察到无需本地改动的恢复，持续拒绝并非必须有同时在途请求；与暂时冷却/风控一致，也与上游间歇波动一致，不能证明频率或并发是触发因。历史正式15个DeepSeek请求在41.610秒开始跨度内全部成功，不支持从现有数据推导固定RPM阈值。第三次成功后停止探测，不立即重启并发测试，不部署030。真实双Agent验收仍待完成；状态从纯上游不可用变为恢复单次但稳定性/并发未验收。

证据：docs/research/opencode-low-frequency-20261018.json。官方仓库用户报告 https://github.com/anomalyco/opencode/issues/56334 也有间歇及自行恢复描述，仅作背景，非本机根因或官方阈值。公开Zen ipRateLimiter源码展示IP/时间窗计数，但不能直接映射当前inference入口的403 Detect Abuse Traffic。未获得该错误的服务端判定或Retry-After，不能承诺固定等待多久恢复。

### 逐级恢复验证授权（2026-10-18 11:51 UTC）
用户要求保持低频，稳定后才逐步双并发及真实编码；明确本轮不部署正式。升级门槛设为至少3次连续官方单次成功、相邻至少120秒，之后隔离双请求，再真实编码，级间留间隔；任一级403立即停止升级。本轮首个官方无插件单次HELLO（11:51:11.802 UTC，距前次成功约26分钟）已返回同一403，未进入任何并发测试。追加一次至少180秒后的低频复核；若再失败则保留未满足稳定gate，不启动桥接/编码。本轮没有设定无人值守持续重试或定时任务。

### 逐级恢复本轮结果（2026-10-18 11:51–11:56 UTC）
官方无插件单次HELLO结果：11:51:11.802为403；间隔192.699秒，11:54:24.501成功；再间隔120.265秒，11:56:24.765再次403（耗时1.915秒）。错误均Detect Abuse Traffic。仅执行3次串行官方CLI，无隔离双并发、桥接推理或编码Agent请求；未满足连续3次成功门槛，停止升级和进一步请求。上一轮成功后约26分钟首测仍失败，本轮成功后低频两分钟仍失败，不能将冷却后单次恢复当稳定，也不能将本轮复现归因于双并发。仍无法排除累计量/共享出口/其它客户端及上游间歇判定。

本轮回执 docs/research/opencode-staged-recovery-20261018.json；baseline_gate_passed=false，stages=[]。正式环境不更新，后续恢复仍需先通过低频gate。用户本轮明确禁止正式更新，覆盖此前条件部署授权。

### 无模型调用的差异审计（2026-10-18 12:03–12:07 UTC）
用户明确禁止重复模型调用和正式变更。本轮0模型调用；仅官方v2.0.32源码下载、本机日志/连接/Windows事件读取，以及两组curl IPv4/IPv6 GET opencode.ai/cdn-cgi/trace网络元数据查询（非推理）。正式daemon PID176208未变。

网络：两组trace相隔约72秒，IPv4和IPv6各自IP fingerprint不变，均AU/PDX/warp off；没有保留明文公网IP。当前热点有IPv6默认路由，正式官方服务有IPv6外连；宽带历史10:53记录无IPv6默认路由。WLAN事件最后重连在10:54:19 UTC，没有之后断开/重连事件，但不能排除手机蜂窝/CGNAT/IPv6地址变化。此前成功/403没有同步保存出口IP、地址族、响应trace-id，无法补证是否IP变化。当前trace路径/客户端也不等同历史模型连接。

后台请求：正式官方日志每5分钟provider.updated/model.updated，v2.0.32 models-dev.ts对应GET models.opencode.ai/api.json、5分钟TTL、瞬态失败至多2次重试。旧版桌面1.19.3日志每10分钟检查软件更新；与隔离官方2.0.32是不同进程。均不等于DeepSeek推理，不应由此判定推理quota耗尽。未完整捕获其它设备/所有桌面流量。

重要修正：官方无plugin CLI helper没有覆盖title。v2.0.32 session/runner/llm.ts会对未命名根会话异步启动title.generate；title.ts可调用辅助模型，失败可fallback主模型。故之前记录的一次CLI启动不保证只有一次上游HTTP请求，可能有标题与正文重叠；历史临时日志已随清理删除，实际次数不能补证。Loadout正式controls hook及真实编码client profile则在本地返回title，行为存在差异。先前“低频单次”应理解为低频CLI启动，不是已证明单HTTP并发。

重试：v2.0.32 provider-error.ts将HTTP403归Authentication，默认isRetryable=false；x-should-retry=true响应头可覆盖。可重试错误runner上限10次、指数退避2秒到10秒；不能将此说成403必然重试。现有回执未保留响应头/RetryScheduled，不能断言历史绝无重试；没有默认403重试风暴的证据。无需运行模型即可确认此代码路径。

结论：显式上游访问拒绝已确认，不是简单断网；网络身份可能参与服务端策略，所以“网络”与“风控”不是互斥原因。动态IP因果、风控触发维度仍UNKNOWN，不能从目前数据断言频率阈值。后续若另获实测授权，应先记录每次IP族/连接、实际primary/title/attempt数量、RetryScheduled和脱敏trace-id，再做变量隔离；本轮不改配置、不执行该测试。证据 docs/research/opencode-readonly-diagnosis-20261018.json 与 opencode-passive-network-20261018.json。

### 031 发布策略覆盖
v0.4 默认保持单 worker 并加入 120 秒间隔和 403 冷却；030 双 worker 继续为实验原型，未取得真实并发编码验收，不随正式启动启用。连续 user 保序修复包含在发布脚本中，但新保护策略下未新增真实模型调用。
