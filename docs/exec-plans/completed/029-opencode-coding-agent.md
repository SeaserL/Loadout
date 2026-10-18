# ExecPlan 029: DeepSeek 真实编码 Agent 兼容验证

Status: COMPLETE — 用户授权正式部署029并重启，正式8317能力与编码验收通过。原宽带拒绝原因未精确归因，不属于部署完成证据。

当前用户授权：热点隔离验收通过后，部署已验候选029、重启正式Loadout服务并从正式API验证。保持现有Provider/账号/客户端配置，不部署DESN。

## Goal
真实 OpenCode 客户端通过隔离 Loadout 完成编码任务，验证 max_tokens、tool_choice 与连续工具轮次；通过后才部署兼容修复。
## Context
用户要求从合成工具往返推进到真实编码 Agent，保持官方常驻服务，不部署 9router。
## Existing behavior
028 已支持实时文本与 MCP 外部工具交接；64 KiB / 64 messages；max_tokens 与强制 tool_choice 明确拒绝。
## Constraints
遵循 ADR-017、Harness；官方 binary 不修改；不伪装身份；不执行上游工具；测试 opt-in 且完全隔离；保留生产配置与其它工作。
## Architecture impact
优先官方扩展接口与 Provider 内部兼容修复。若需改变 ADR-017 边界先修订 ADR。不修改公共 Router abstraction。
## Files expected to change
scripts/opencode_*、scripts/test_opencode_bridge.py、新 live probe、Provider notes、ADR-017、本计划与 research receipts。
## Implementation stages
1. 检查官方扩展与真实客户端请求。
2. 隔离复现兼容问题并实现有语义保证的修复。
3. 多轮编码任务、参数边界与回归验证。
4. 仅验证通过的实现部署并复验。
## Tests
默认 mock tests；显式 LOADOUT_LIVE_TESTS=1 实测；编码任务在临时目录，独立测试验证产物；保存不含凭证的结果。
## Performance validation
记录实时增量与任务用时；若修改 Router/Gateway 则运行 benchmark。
## Security considerations
随机临时 client key 与精确模型策略；官方服务仍 ask、不批准；客户端工具仅测试工作区；正文和凭证不进入生产日志。
## Migration considerations
预计无 schema 迁移；部署前保留脚本备份。
## Known risks
官方 Agent 上下文、实验 API、输出参数不可表达、真实客户端 content 格式/工具数量/正文大小均可能不兼容。不得静默忽略有语义参数。
## Definition of done
真实编码客户端任务完成且独立测试通过；连续工具轮次和参数语义有证据；失败如实保留；部署仅在 gate 通过后。
## Progress
- 正式部署完成：备份deployment-backup-20261018-220429，五文件与仓库SHA一致，plugin index.js一致。重启daemon176208/official216256/bridge220232；现有Provider/account/client数据保留，无Rust二进制替换。
- 正式8317验收15请求成功/0失败，SSE17增量、max_tokens JSON/SSE length、none/required/named与三轮工具结果汇总通过；真实编码19.603秒、4工具轮/5请求、精确路由全部通过，独立测试/隐藏检查通过且测试文件未改。临时key已撤销，session0/MCP空/db0，DeepSeek HEALTHY。
- big-pickle正式JSON与SSE marker回归均通过（verify_opencode_bridge.py --production --model big-pickle）。当前为手机热点，原宽带/任意消息序列兼容/持久可用性不作保证。未部署DESN，无系统自启变更。回执opencode-production-029-deployment与acceptance-20261018.json；任务完成。
- 用户授权正式部署：已用正式CLI确认opencode-bridge已注册、精确DeepSeek启用，daemon运行target/v3/release版本。需要部署五个桥接/控制插件文件，无Rust变更/无需重新编译；先备份再重启三个相关服务，随后正式8317端口能力与真实编码验收。
- 热点能力/编码验收完成：DESN 基础和官方前后对照成功，实时39块，max_tokens=16 JSON/SSE length，none/required/named工具选择及连续三轮+随机结果汇总通过；真实编码5工具轮/6请求，14.117秒，独立测试/隐藏检查通过。
- 隔离 Loadout 候选029复验：实时18块，参数三模式及长度限制通过；首个连续工具测试因两条连续user被既有消息顺序限制拒绝（400，非403），保留 initial-failure 回执。调整为支持的单user起始序列，重用本轮已通过的6项检查避免重复流量；三轮工具及结果汇总通过。真实编码4工具轮/5请求，13.765秒，精确route header全部验证，测试文件hash不变，独立/隐藏检查通过。
- 19默认Python测试通过；隔离清理 session=0/MCP=[]/db=0。生产PID128088/211364/213864未重启，四部署文件仍匹配回滚SHA。没有源码功能修复或生产部署；新增验证脚本及独立热点回执。当前能力不等同任意消息序列兼容或长期可用性保证。
- 热点复验启动：复用隔离 daemon/官方 server 生命周期与真实编码客户端，新增独立热点回执以保留旧失败证据。两条路线顺序执行；能力失败不进入编码任务，不使用生产端口或凭证。
- 网络对照准备（10:53 UTC）：系统代理关闭、WinHTTP direct、无 PAC，进程/用户/机器 proxy 环境变量无配置；监听端口/进程未发现本地代理。默认路由仅 WLAN，无 IPv6 默认路由；目标 trace loc=AU/warp=off。官方隔离同 DeepSeek 基线仍 403 Detect Abuse Traffic。当前无第二可用出口，已向用户询问现成代理/第二网络；未切换系统网络，不能据此断言 IP 被封。证据 opencode-network-control-20261018.json。
- DESN131 隔离实测完成：10:49:00–10:49:07 UTC，官方前测 / DESN Chat / 官方后测均 403 Detect Abuse Traffic。固定 b48d93e 原始源码未修改，健康与模型列表成功；基础失败跳过 SSE/参数/多轮条件测试。隔离进程和目录已清理，生产文件/进程保持；回执 desn-opencode-live-20261018.json。BLOCKED_EXTERNAL 不变。
- 用户授权 DESN131/opencode-zen-proxy 隔离实测：固定 b48d93e，官方前后对照，精确 DeepSeek；基础成功后才扩展 SSE/tool_choice/max_tokens/多轮工具。使用原始源码、随机 loopback 端口、无真实上游密钥，不改生产。
- opencode2api 隔离实测完成（10:35 UTC）：官方前测、第三方 Chat、官方后测全部 403 Detect Abuse Traffic；第三方模型发现成功。仅 runtime.go 关闭自动全模型探测，原推理逻辑未改；无代理/模型轮换。基础失败故跳过条件能力测试。回执 opencode2api-live-20261018.json。隔离 PID 208508 与状态目录已清理；生产进程未重启，部署四文件 hash 仍匹配回滚备份。BLOCKED_EXTERNAL 不变。
- 用户追加授权：隔离运行 opencode2api 固定 83d623d，与官方同模型实测对照；允许第三方自身请求路径，不移植/部署。临时副本仅关闭全模型后台 availability probe；无上游账号密钥、无代理轮换/模型回退。先基础文本，成功后再测 SSE、tool_choice、max_tokens、多轮工具。正式环境不动。
- 后续调查（用户要求，禁止更新正式环境）：增加未加载桥接 plugin 的官方 CLI / serve 对照，固定相同 binary/model/prompt，按时间记录；只修改隔离诊断脚本与研究文档。上一轮“免费上游限制”尚未排除桥接形状因素，先重新验证归因。
- 2026-10-18: 完成入口、Harness、ADR、参考地图、现有桥接/测试检查；确认官方 2.0.32 提供 run CLI 与 AISDK language hook，继续验证。
- 真实客户端初始失败：store=false/max_completion_tokens 被拒绝；标题并发与单并发冲突；后续空 reasoning_content 被拒绝。隔离修复逐步验证。
- 官方 session HTTP hook 已确认可传递参数；4 个小 token 上限用例（两字段 × JSON/SSE）均为 length。none/required/指定函数实测通过；较早 required 曾选中内置工具而明确报错，不能称零失败。
- 通过官方 context hook 去掉内置工具的实验收到上游 403 FreeTierError，已撤回，不模拟身份或伪造工具。保留原生工具，添加明确外部执行上下文；真实客户端已执行两次 read，完整任务仍在验证。
- 生产未更新。默认 Python 17 tests 通过。
- 隔离真实编码任务连续通过三次（首次 16.031 秒，增强断言 14.609 秒，最终 client profile 18.131 秒），每次四轮工具调用、五次精确模型请求。独立测试与隐藏迭代器/浮点/大整数检查通过，原测试 hash 不变。
- 最终默认 Python 19 tests 与 Node hook test 通过；隔离 Loadout 实时 28 增量、JSON/SSE 工具回传、反序结果与断开清理通过；参数专项 7 cases 通过。
- 已备份部署脚本到 compat-backup-path.txt 指向目录，仅重启官方服务与 Python bridge；Loadout daemon/GUI 未重启。生产真实编码复验进行中。
- 生产真实编码复验失败（首请求 502）；读取官方事件为 provider.auth / 403 FreeTierError。新建隔离实例也出现模型失败。回滚 028 脚本、重启官方/桥接后，最小文本仍为同样 403。已核对四个部署文件与备份 SHA 一致，未接纳 029 生产升级。
- 最终 session=0、MCP=[]、db 文件=0；临时客户端已撤销；其它 Loadout/桌面进程保留。当前阻塞不归因为配额/封禁/版本变化，具体上游判定原因 UNKNOWN。
- 剩余：上游恢复后重跑参数与真实编码 gate、big-pickle 回归，再部署和生产复验。此计划保持 active，不宣称完成接入。
## Decisions
- 使用独立 OpenCode 客户端，不访问用户已有 Agent 凭证，不调用其它模型。
- 403 对照完成：2026-10-18 10:24–10:26 UTC，未加载 plugin 的官方 CLI 两次、原生 serve 一次、原生 read 任务一次均返回 provider.auth/403，明确消息为 `Forbidden, OpenCode Free Model Gateway Detect Abuse Traffic`；隔离 bridge 原生事件亦为同一错误。已排除“只有桥接路径此刻失败”，未排除历史桥接/测试流量触发共享上游限制。
- 时间线：10:13:06.056 UTC 最后一项完整编码任务成功；10:14:01.568 UTC 已有生产失败事件。旧回滚基线也是 403。此处时间来自保存的 client event timestamp，不是文件修改时间，不宣称精确风控触发时刻。
- 官方公开 inference-proxy 源码将免费资格/认证交由独立 inference 服务；公开材料未给出本次风险阈值、命中维度或恢复时间。不能断言 IP 封禁、配额耗尽或冷却期。停止重复探测，不换 IP/身份/模型，不修改生产。
- 证据：opencode-403-control-20261018.json（binary SHA、固定模型、原生错误）、opencode-403-timeline-20261018.json；脚本 probe_opencode_403.py，显式 opt-in、至多 6 个顺序用例。隔离进程已清理，正式官方 PID 211364 保持未变。
- 2026-10-18 用户指定 opencode2api 参考审查完成：固定 83d623d541c47a17cd832ee4f53bd7c95f3a18fa，GPL-3.0 概念参考。检查身份、上游重试、工具形状、参数转换与 SSE；其模拟身份/补入工具路线不符合 ADR-017，且同名 schema 替换存在 Agent 契约风险。Issue 120/109 未提供当前 403 已解决证据。不执行该网关或追加上游探测，不更新正式环境；BLOCKED_EXTERNAL 不变。具体文件与来源记入 reference-map/provider notes。
- 2026-10-18 用户要求最新生态搜索：查 GitHub 默认分支 commit 时间与真实修复内容，找到 KochC v1.11.1、DESN131 当日新库、ebouther 当日代理池封装。新增两份代码定向检查，证据写入 reference-map。尚无已验证解决 Detect Abuse Traffic 的独立新实现；opencode2api 已测 SHA 仍是最新 main。官方 #56756 有精确错误，#56334 有同 DeepSeek 网络相关对照报告（社区证据，非本机结论）。本轮仅研究，无新 live calls/安装/网络或生产变化。
- 10:55 UTC 用户切换手机5G热点后官方 DeepSeek连续2/2 HELLO成功（3.440/4.527秒），对比10:53宽带403。网络相关性得到支持，但未记录IP/未做A-B-A，尚不能锁定IP封禁或排除随时间恢复。下一步应在可用网络重新跑隔离参数/Agent gate；生产升级仍未授权。证据 opencode-hotspot-control-20261018.json。
