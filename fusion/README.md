# Loadout Fusion — Windows 便携版

新控制台、Tauri 窗口和托盘、OmniRoute v3.8.52 后台和管理工具在同一个便携包里。无需 Node、npm 或开发服务器；需要 Windows 10/11 x64 和 WebView2。没有安装器和自启，不替换已安装的正式 Loadout。

## 启动与退出

双击 `Loadout.exe`（或仓库根目录的快捷方式）。窗口先显示启动状态，后台就绪后直接进入控制台，**不需要密码**：窗口用系统凭据库里的机器密钥换取会话，你看不到也不用管。用浏览器直接打开这个地址没有会话，只会提示从应用打开；其他本机用户同样进不来。客户端调用 `/v1` 仍然需要客户端密钥。

默认数据在包的上一级目录 `loadout-fusion-isolated/desktop-current`，也可以用 `Loadout.exe --instance "D:\某处\loadout-fusion-isolated\one"` 指定。

- 右上角 ×：隐藏窗口，托盘和网关保留。
- 托盘左键或“打开 Loadout”：恢复窗口；重复启动复用窗口。
- 托盘“退出窗口（网关继续运行）”：只退出窗口。
- 托盘“退出并停止网关”，或设置页的“停止网关”：停止后台。`Stop-Backend.cmd "实例目录"` 效果相同。

网关只在客户端请求或你点击操作时才访问上游，空闲时不主动连接；连接过的主机名记在实例目录的 `egress.log`。设置里可以关掉“允许连接上游”回到仅本机。

## 启动与管理

实例目录必须位于名为 `loadout-fusion-isolated` 的目录之下，且在发行包之外。拒绝覆盖已有目录、链接/junction 和生产默认端口8317。请使用 `fusion.cmd`；直接运行上游 `server.js` 会在加载后台前拒绝，避免误用默认数据目录。

```powershell
.\fusion.cmd verify
.\fusion.cmd init D:\Trials\loadout-fusion-isolated\one
.\fusion.cmd start D:\Trials\loadout-fusion-isolated\one
.\fusion.cmd status D:\Trials\loadout-fusion-isolated\one
.\fusion.cmd stop D:\Trials\loadout-fusion-isolated\one
```

首次启动选择空闲 loopback 端口并保存；以后重启保持该端口，占用时失败，不接管其他服务。后台独立于调用终端。异常退出后重新 `start`；不做无限自动重启。修改配置、备份或清理前必须停止。状态控制通道用 OS vault 中随机秘密认证，并检查实例/run身份。`verify`校验全包；启动校验工具、Node和入口等关键文件，不在每次启动重复读取全部依赖。

管理认证密码自动生成并只存 OS vault，CLI 自动登录原生管理 API。凭证保护固定开启，实验接口不存在；关闭正文日志、远程控制和自动发现。可用管理路径限于 `/api/providers`、`provider-nodes`、`keys`、`combos`、`settings`、`resilience`、`rate-limits` 和加密 `/api/loadout/vault`。保留原生鉴权/CSRF。禁止普通JSON导出、key reveal和未审计控制入口。

`admin INSTANCE GET /api/providers` 查看账号；`admin INSTANCE PUT /api/providers/ID` 从私有 stdin 接收更新JSON；`DELETE` 删除。`add-provider INSTANCE` 的 stdin 形状是 `{slug,baseUrl,accounts:[{alias,key}]}`。使用隐藏输入获取 key，再在内存构造JSON送入管道；不要把真实凭证写进命令行、脚本、普通JSON文件或终端历史。`admin` 返回值脱敏，创建客户端key时另存OS vault；`client-key INSTANCE client-ID --show` 仅在明确要求时显示，用于复制给客户端。`add-provider` 的客户端key名为 `client-<slug>`，迁移路由key名为 `route-<路由名>`。

空账号范围在上游表示主池；要禁用子池应禁用该client key，不能把空数组当成“无人可访问”。主/子集合分别使用明确的key权限；改变成员后重新编译映射。模型/账号行为沿用已验证的原生规则。

轮转通过原生 `PATCH /api/settings` 的 `providerStrategies` 指定：`{"providerStrategies":{"<Provider ID>":{"fallbackStrategy":"round-robin","stickyRoundRobinLimit":1}}}`。关闭改为 `fill-first`。点名账号优先、严格固定与client key账号范围仍生效。客户端key权限使用 `PATCH /api/keys/<ID>`，账号更新使用 `PUT /api/providers/<ID>`。

`traffic INSTANCE` 从stdin接收 `{"connections":["连接ID"]}`，复用已验证的保守配置：每账号至少20秒、并发1、每分钟3次，原生有界等待；应用后读回确认。商汤采用此配置仍不保证已有风控解除，不在此命令中触发真实推理。

## 网络与 WorkBuddy

默认 `network=loopback-only`，只允许本地 mock。此包的集中回归不读取真实账号，不运行真实客户端。未来要做单独授权的真实验证，可在停止实例后用 `configure INSTANCE` 的 stdin 设置 `{network:"allowlist",hosts:["明确主机名"],port:端口}`。不读取系统代理或复用生产配置，不自动放开所有出口。

WorkBuddy 连接器已随包，配置为上述对象中的 `workbuddy:{authPath,exe,accountHash,clientVersion,cliVersion,port}`：路径指向官方桌面加密会话与可执行文件；版本必须对应实际安装；accountHash为 `SHA256("loadout-workbuddy-account\0" + uid)`。只读取已绑定账号的 access token，不复制/刷新官方凭证。启动后 `add-workbuddy INSTANCE` 将该loopback连接器登记到原生Provider，桥接key只进入OS vault；模型固定 `deepseek-v4.1-flash`。真实出口需显式允许 `www.workbuddy.ai`。本轮没有验证新的实机登录或上游状态。

保持已验证的首行精确兼容；未知首行原样通过。连接器单并发、请求256KiB、响应1MiB、65秒超时、输出最多2048token，**完整缓冲SSE后转发**，不是实时流式。错误后30秒冷却再允许尝试，账号/模型不自动改变；没有测试用16次寿命限制。上述边界不是无限长会话或全部工具可用性的保证。

## 迁移

### 053 订阅扫描与受控登录

`subscription-scan [CODEX_HOME EXECUTABLE]`复用旧Codex只读扫描：app-server额度优先，session记录回退标记estimated；不导入官方token。该命令是显式查询，不启动推理客户端会话。

`subscription-login INSTANCE`从stdin接收`{alias}`，创建/复用原生Responses节点并返回设备登录地址、代码与会话ID。真实登录需实例网络显式允许auth.openai.com和chatgpt.com；默认loopback仍只可用于模拟服务。本轮不代替用户登录。

`subscription INSTANCE`从stdin接收以下操作：`{operation:"status",id:登录会话ID}`查询结果；`{operation:"cancel",id:登录会话ID}`取消；`{operation:"list"}`列出已登录连接。首次登录后转发默认关闭，同一账号别名重新登录保留已有opt-in；开启使用`{operation:"transport",id:连接ID,enabled:true,confirmation:{risk:"I understand",alias:账号别名}}`；关闭用enabled:false。这保留旧版风险确认流程，token不返回CLI。后台每5分钟检查到期前24小时的刷新；失效标记needs_login，停止重试。修改/删除/备份通过现有安全边界，普通Provider写入不能绕过订阅生命周期。

含订阅的旧池走两阶段迁移：先在隔离实例完成受控登录及其他账号创建，再`map ... --aliases ALIASES_JSON --apply`。订阅binding额外携带`credentialProfile:"loadout-codex-v1"`，应用时核对实际连接的登录/opt-in状态，不能用普通API key伪装订阅凭证。

ALIASES_JSON形状为`{"loadout/default":[{"account":"deepseek/account-1","model":"明确模型"}],"deepseek/main/明确模型":[...]}`。旧`loadout/<池名>`自动生成精确原生映射，其余别名必须明确给出成员。客户端模型白名单和RPM一并迁移，未知策略拒绝。默认虚拟池不能加入订阅账号；显式GPT池可以。这里只改变配置入口，不重写调度器。全量一次性`migrate`遇到订阅会提示使用受控登录和map，不偷偷略过账号。

`preview SOURCE_DB [BINDINGS_JSON ENDPOINT_MODELS_JSON]` 只读旧Loadout SQLite元数据，输出来源指纹、缺失成员和阻塞；不读凭证值。源schema或策略未知时拒绝。实际旧 `gpt` 池成员仍有待绑定项，不因打包而自动解决。

`migrate SOURCE_DB NEW_INSTANCE` 从私有stdin接收：

```text
{sourceFingerprint, config:可选的显式网络配置, endpointModels:{端点名:[模型]},
 providers:[{slug,baseUrl,traffic:"conservative-v1或native-default",accounts:[{ref:"provider/account-1",alias:"a",key:私有输入,models:[模型]}]}]}
```

此版本选择显式安全输入，不自动扫描/复制正式 Credential Manager。一次性migrate要求所有启用账号和池成员；遇到Codex订阅须改用上述受控登录+map流程。其他未支持认证、未知策略和子号池源格式明确拒绝。成员齐备后才创建新实例；来源变化拒绝，普通失败清理新vault并标记失败。客户端key重发，URL统一 `/v1`，池别名与新权限写入无密钥迁移报告。中途断电不保证自动回滚，失败目录保留供检查，不能直接启用。

也可先通过原生管理入口创建账号，再用 `map INSTANCE SOURCE_DB BINDINGS_JSON ENDPOINT_MODELS_JSON` 预览；bindings值为 `{provider,connectionId,models}`。加 `--apply` 才应用完整READY映射；不选择部分成功、不跳过缺失成员。等待采用原生有界语义，不保持旧端点严格30秒总期限。

## 备份、恢复、回滚

```powershell
.\fusion.cmd stop D:\Trials\loadout-fusion-isolated\one
.\fusion.cmd backup D:\Trials\loadout-fusion-isolated\one D:\Trials\one.enc.json
.\fusion.cmd restore D:\Trials\one.enc.json D:\Trials\loadout-fusion-isolated\restored
```

交互隐藏输入至少12字符口令；自动化用 `--password-stdin` 私有管道。AES-256-GCM+scrypt加密DB、匹配vault、启动秘密及配置；先停实例和checkpoint WAL。错误口令/篡改在创建目标和写凭据前拒绝，不覆盖既有目标。普通SQLite副本只有ref，不能独立恢复凭证。

`rollback` 与restore相同：恢复到新目录，保留原实例；只接受匹配发行版本，不逆向改schema。恢复保持原监听配置，启动前先停原实例，或显式改新端口。

`upgrade ARCHIVE NEW_INSTANCE SOURCE_MANIFEST`在053候选仅接受精确052发行`fusion-v3.8.52-isolated-052.1-eb2d7b7aede2`到053.1的适配升级，OmniRoute仍固定v3.8.52。来源manifest必须匹配加密备份内部身份，其他版本拒绝；命令返回的PENDING_START不是启动验收成功。历史052的v3.8.51→v3.8.52证据不覆盖新增订阅适配。回滚使用匹配旧程序和升级前状态，不用旧程序打开已升级数据；Git历史不能恢复运行数据或OS vault。临时旧包结束即删，不证明任意未来版本兼容。

`add-provider`/`migrate`支持显式 `apiType:"chat"|"responses"`（默认chat）；OpenCode Go旧GPT成员使用responses。`preview`包含逐成员inventory与商汤全部5个模型目录，但不创建连接或授予权限。053的map支持原生精确别名、客户端模型白名单与RPM；订阅连接必须先登录并opt-in。accountInventory保留所有源账号，fullMigrationReady区分全量绑定与仅池路由可用；未知/只读账号不能在报告中消失。

测试结束 `cleanup INSTANCE --delete-test-credentials` 只清理本隔离实例OS vault，保留文件。删除后需可解密备份才能恢复。请保管口令；本轮自动化回归的随机口令不会交付为用户灾备材料。

## 已知边界

商汤真实429、CommandCode401、WorkBuddy自然轮换未观察及客户端文件执行策略问题延期，不作为本阶段完成条件，也不继续调试。mock通过不代表这些问题已解除，不批准生产替换。当前路线是受控fork，有上游接缝维护成本。包内manifest校验内容完整性，不是数字签名，也不证明上游依赖安装可从零复现。

## 054 原生只读查询

仅管理员会话，经现有admin命令读取；普通推理key不能读取全局管理统计。下列路径只开放GET/HEAD，POST/PUT/PATCH/DELETE禁用。

```text
fusion.cmd admin INSTANCE GET /api/models
fusion.cmd admin INSTANCE GET /api/provider-models?provider=PROVIDER_ID
fusion.cmd admin INSTANCE GET /api/usage/history
fusion.cmd admin INSTANCE GET /api/usage/model-latency-stats?minSamples=1
fusion.cmd admin INSTANCE GET /api/usage/call-logs?limit=20
fusion.cmd admin INSTANCE GET /api/usage/route-explain/REQUEST_ID
fusion.cmd admin INSTANCE GET /api/usage/combo-trace/COMBO_INVOCATION_ID
```

静态模型目录与自定义模型分开返回；可用标记不等于本机账号实测成功。history是网关已记录的token和估算成本，不是余额/账单；成功非流式响应不含usage时不会记入该用量表，不应当成全部HTTP请求审计总数。call-logs仅开放元数据索引，未开放正文详情或导出。route-explain包含历史元数据与当前状态重算，保留原生confidence/limitations；costActual固定0不代表免费。combo-trace为原生进程内30分钟/2000条短期轨迹，需已知invocation id，重启或过期后404。查询不触发目录刷新或真实Provider探测。

054仅验证从053.1固定候选`fusion-v3.8.52-isolated-053.1-5bb0cd9cc642`升级，使用上述upgrade命令；真实OAuth/外部账号及未来底座版本不在本轮验收内。

原生history在计算较慢时可能缓存最多60秒，并以`x-analytics-cache`标记；查询即时性沿用上游语义，不为测试关闭缓存。

## 055 免费目录与 Radar

继续直接使用原生目录，不维护独立名单。`free-models`是随固定OmniRoute版本发布的静态基线，没有独立更新时间；`radar/catalog`返回基线与已验签缓存的合并视图，`radar/status`返回各feed的本地状态。

```text
fusion.cmd admin INSTANCE GET /api/free-models
fusion.cmd admin INSTANCE GET /api/radar/catalog
fusion.cmd admin INSTANCE GET /api/radar/status
fusion.cmd admin INSTANCE GET /api/radar/settings
```

`generatedAt`为上游数据生成时间，`fetchedAt`为本机接受该版本的时间；字段缺失/null不补造日期。`available:true`只说明有缓存，不保证新鲜或平台可用。社区feed按固定上游文档可能有约30天延迟，本轮未验证托管服务当前供给或任何平台的真实免费额度。

默认opt-out，不自动下载。仅启用Radar原生scheduler，其他后台服务继续关闭。以下操作沿用现有admin POST，JSON通过stdin传入（PowerShell示例）：

```powershell
'{"optIn":true}' | .\fusion.cmd admin D:\Trials\loadout-fusion-isolated\one POST /api/radar/settings
'{}' | .\fusion.cmd admin D:\Trials\loadout-fusion-isolated\one POST /api/radar/sync
'{"optIn":false}' | .\fusion.cmd admin D:\Trials\loadout-fusion-isolated\one POST /api/radar/settings
```

开启opt-in会立即执行原生检查，之后每小时检查：catalog在fetchedAt后24小时到期，referrals使用独立1小时窗口。重启恢复opt-in后也会检查。页面打开时的6小时规则属于原生UI，本后台不启动它。关闭opt-in保留缓存，后续tick/手动同步跳过；已发出的下载不会被主动撤销。同步不是账号授权，不创建账号、模型推理路由或combo。

原生同步有30秒网络timeout、10MiB响应上限、Ed25519验签、schema及版本检查。`updated`才更新缓存日期；`stale`表示同/旧版本而不是“本地数据已过期”。相同版本、验签/schema/大小/网络错误保留原缓存；相同版本不会刷新fetchedAt，因此到期后可能每小时再检查。旧缓存仍可读取，应自行根据两个时间理解时效，不承诺过期自动隐藏。

本轮仅社区feed；settings只允许`optIn`，拒绝supporterKey。付费offers/intel、sync-all、本地模型覆盖等管理入口未开放。原生scheduler会同步referrals，无supporter key时不下载付费feed。

默认feed与公钥沿用OmniRoute，网络仍受实例的loopback-only/hosts白名单约束。正式托管下载需要显式配置`network:"allowlist"`与`hosts:["radar.omniroute.online"]`；这只是配置方式，本轮没有实际访问托管服务。

自托管或隔离测试可在实例config设置`radar:{feedUrl:"http://127.0.0.1:PORT",publicKey:"BASE64_ED25519_SPKI_DER"}`，映射到原生RADAR_FEED_URL/RADAR_FEED_PUBKEY。公钥是公开验证材料；不接受私钥、URL内凭据或任意环境变量。非loopback地址要求HTTPS，另受既有出站限制；不绕过签名验证。

055仅接受精确054候选`fusion-v3.8.52-isolated-054.1-7f8bb48c76cd`的同底座upgrade；普通restore仍要求同一发行身份。当前包仍是隔离后台，不安装或切换正式桌面端。

## 056 原生价格目录与同步

价格表、合并规则、同步及计价沿用OmniRoute，不维护独立名单。只开放管理员只读目录及手动同步：

```text
fusion.cmd admin INSTANCE GET /api/pricing?includeSources=1
fusion.cmd admin INSTANCE GET /api/pricing/defaults
fusion.cmd admin INSTANCE GET /api/pricing/models
fusion.cmd admin INSTANCE GET /api/pricing/sync
```

```powershell
'{"sources":["litellm"],"dryRun":true}' | .\fusion.cmd admin INSTANCE POST /api/pricing/sync
'{"sources":["litellm"]}' | .\fusion.cmd admin INSTANCE POST /api/pricing/sync
```

`dryRun`仍下载但不写入。同步源为原生固定地址`https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json`；需要实例出站白名单允许`raw.githubusercontent.com`，本轮仅使用隔离模拟下载，没有验证该远程源当前可用性。读取目录不会触发下载。价格编辑/删除、同步数据清空入口不开放。

优先级：用户覆盖 > models.dev已存层 > LiteLLM同步 > 内置默认。sourceMap标记模型级最高优先级来源，并非每个字段的溯源；字段可由多层合并。本轮不启用models.dev同步器，现有覆盖数据仍保留。token价格为USD/百万token，图片/秒等字段按原生单位保留；这些字段存在不表示本轮开放了对应推理协议。

`lastSync`是本机成功写入的时间，没有上游价格生成日期；相同内容再次同步也会刷新它。默认自动同步关闭，手动同步仍可用。需要自动同步时，停机后在原有完整config中加入`pricingSync:true`再configure/start：原生启动立即同步，之后默认每24小时尝试一次。关闭需停机改为false再启动。`nextSync`是lastSync加间隔的展示值，不是实际timer队列；手动同步、失败或重启后不保证与实际触发时刻一致。不开放任意URL、环境变量或自定义短间隔。

原生下载有30秒timeout，但没有Radar式签名/版本校验或响应大小上限。网络/JSON错误保留已同步层；HTTP成功且有效空对象会清空该层并记为成功，然后回退其他层。这里保留原生语义，不伪称空数据被拒绝。

成本沿用原生计价器；未知价格可能呈现0，不代表免费。缺少上游usage的成功请求不保证进入用量统计；按当前价格估算的历史可能随同步改变，已存上游明确cost优先。订阅的API等价价值与实际账单不同，不把这些统计当成余额或真实账单。统计缓存可能延迟可见。

056的upgrade只接受精确055候选`fusion-v3.8.52-isolated-055.1-4d0b0a9c19c8`，普通restore仍要求同一发行身份。仍为隔离后台，不切换桌面端。


## 原生账号配额、预算与模型同步（057）

所有命令通过 `fusion.cmd admin INSTANCE METHOD PATH` 调用原生 API；POST/PATCH 的 JSON 从 stdin 输入。管理会话独立于推理 key。

| 操作 | 原生路径 | 含义 |
|---|---|---|
| 账号限流观察 | GET `/api/usage/quota?connectionId=ID` | 由响应头、冷却或队列压力推导；`quotaTotal=null` 是未知，原生 `percentRemaining=100` 不能解释为余额充足。 |
| Provider 配额缓存 | GET `/api/usage/provider-limits` | 保留 `source`、`fetchedAt`、上游特有窗口；不主动刷新。旧缓存不等于当前真实余额，无缓存不等于零。 |
| 本地费用预算 | GET `/api/usage/budget?apiKeyId=ID`；POST `/api/usage/budget` | POST 如 `{ "apiKeyId":"ID", "dailyLimitUsd":10 }`；按原生重置周期统计，不是上游账户余额。 |
| 日/周使用限额 | GET `/api/keys/ID/usage-limits`；PATCH `/api/keys/ID` | PATCH 如 `{ "usageLimitEnabled":true, "dailyUsageLimitUsd":10, "weeklyUsageLimitUsd":50 }`；返回已花费、重置时刻、未知价格标志。 |
| 指定账号发现 | GET `/api/providers/ID/models?refresh=true` | 会调用该账号上游，返回原生 `source` 和目录；默认网络隔离仍生效。 |
| 指定账号同步 | POST `/api/providers/ID/sync-models`，body `{}` | 原生按账号持久化目录和差异；手动触发，无独立名单或全局自动任务。 |
| 已同步目录 | GET `/api/synced-available-models?provider=PROVIDER` | Provider 聚合视图；目录不是账号授权。推理仍受 allowedConnections、池和精确模型规则约束。 |

预算沿用原生语义：已完成请求的成本估算进入统计，超限拦截后续请求；没有并发预占款，因此单次及在途请求可能超额。`usage/budget` 与 key 日/周使用限额是两套原生控制，窗口、边界及订阅计费例外并不相同，不应混称一个钱包。未知价格在使用限额路径中会令后续请求 fail closed，不能依赖价格表缺项获得免费调用。新价格也可能改变未存固定成本的历史估算。

同步失败保留上次有效目录；删除的模型不自动替换成另一个模型。保留原生默认关闭的自动修剪 Combo 步骤和自动写 Codex/Claude 配置。没有开启真实账号配额轮询或真实 Provider 验收。

原生 key 限额 API 和 budget POST 均拒绝负值。key API 的零值归一化为 `null`（无该窗口限额），budget 的零值也表示不限额；配置后应通过 usage-limits 读回实际生效值。不要用零表示禁止调用，应禁用该 key。
