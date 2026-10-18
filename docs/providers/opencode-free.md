# OpenCode Free (`opencode-bridge`)

- Last Verified Date: 2026-10-18
- Decision: [ADR-017](../adr/ADR-017-opencode-official-bridge.md)
- Module: `crates/providers/src/opencode_bridge.rs`

**当前状态（正式部署验收）**：用户授权后已将029五个桥接/控制插件文件部署到正式目录并核对SHA，重启官方server、bridge、Loadout daemon。正式8317端口DeepSeek能力/真实编码15请求全成功，HEALTHY；big-pickle JSON/SSE基础回归通过。当前基于手机5G热点，不代表原宽带恢复或长期可用。

**403 后续归因**：此前宽带下官方无插件CLI/serve/原生任务也返回 `Forbidden, OpenCode Free Model Gateway Detect Abuse Traffic`，热点恢复支持网络相关因素，并非只有bridge失败。尚无A-B-A及服务端判定证据，精确IP/路由/历史流量触发因素仍UNKNOWN。见文末对照。

## Implementation References / Source Licenses

正式接入地址 `http://127.0.0.1:8317/v1`，协议 Chat Completions，模型 `opencode-free/main/deepseek-v4.1-flash-free`，认证使用已有Loadout client key（受其客户端策略约束）。不使用临时测试key。部署回执 `docs/research/opencode-production-029-deployment-20261018.json`，正式验收 `docs/research/opencode-production-029-acceptance-20261018.json`。真实编码19.603秒、4工具轮/5请求，原测试不变且独立/隐藏检查通过；会话0/MCP空/db0，临时验收key已撤销。当前仍单并发/64KiB/64消息/45秒，连续user消息不支持；本轮未新增开机自启。
官方 OpenCode v2 Windows `@opencode/cli-windows-x64@2.0.32`，npm integrity 核对；官方 [v2 API](https://opencode.ai/v2/docs/api)、`packages/cli/src/database-path.ts`。官方程序独立运行，不复制其源码。Python 适配器为原创；DPAPI helper 复用本仓库现有实现。9router 仅先前调研，本实现无其依赖。

## Official Authentication Method / Official API Availability
无需上游账号/key。官方客户端自己的 free Provider 处理认证。本地官方 serve 使用随机 Basic password，bridge 使用独立随机 Bearer key。API 是官方 Agent API，不是官方开放给通用第三方推理的免费端点。

## Subscription Transport Status / Terms-of-Service Risk
逐账号 opt-in、CLI_BRIDGE。技术成功不证明服务条款授权或长期免费。免费资格、额度未知，不能显示无限额度。只列实测 big-pickle 与 deepseek-v4.1-flash-free。

## 能力与语义
支持完整文本 messages：可有首条 system，普通文本 user/assistant 交替；DeepSeek 另支持 assistant tool_calls 与匹配的 role:tool 文本结果，末条可为 user 或完整 tool 结果组。最多 64 条、64 KiB。官方 experimental import 恢复历史角色；assistant 必须带 completed 时间，否则官方静默丢弃。桥接检查导入完整性、过滤旧 assistant。system 是官方 Agent 的 system update，不保证原始 system role 优先级。每请求独立 session；客户端重传历史，服务不保存跨请求会话。官方 Agent 会附带自身上下文。DeepSeek 支持 function tools；拒绝图像、采样参数、stateful。未声明的官方工具返回 422，不执行工具。JSON 与实时 SSE：官方 text.delta 即时转发，工具 name/id 先输出、完整参数在官方 input.ended 后输出，finish 之前不得执行客户端工具。输出长度参数的新增支持见 ExecPlan 029。

## 部署与使用
固定官方 2.0.32 binary；独立目录内有 `opencode.exe`、`spawn_detached.exe`、Python 脚本、`binding.json` 与 DPAPI secret。`Start-OpenCodeBridge.ps1` 显式启动。默认端口 bridge 18789 / official 18790，仅 loopback。没有增加系统自启；重启机器后需重新运行启动脚本。

Loadout kind `opencode-bridge`，slug `opencode-free`，alias `main`。使用精确模型 `opencode-free/main/deepseek-v4.1-flash-free` 或 `opencode-free/main/big-pickle`，不进入 default/fast/realtime。DeepSeek 支持 API 客户端执行工具的循环；客户端附加的其它参数仍须符合下面的明确范围。

```json
{"model":"opencode-free/main/deepseek-v4.1-flash-free","messages":[{"role":"user","content":"你好"}],"stream":false}
```

官方数据库强制 `:memory:`，每次请求清理会话；清理失败后 bridge 停止接受推理，需重启。只保存无正文的部署配置。日志不记录请求/回答，错误为固定分类。

## Validation
`python -m unittest discover -s scripts -p test_opencode_bridge.py`；Provider mock tests。Live 验证需 `LOADOUT_LIVE_TESTS=1`，运行 `verify_opencode_bridge.py` 默认隔离 daemon；`--production` 仅验证已安装配置，创建并撤销临时 client。

## 持续性与重启边界（2026-10-18）
官方 native session 连续调用、30 秒空闲召回、独立 session 隔离通过。初始 6 次均完成精确模型推理；中文精确字符串断言因句号差异为 false，保留原始结果，不计为全通过。

隔离官方进程与桥接同时重启后，旧 session HTTP 404；客户端完整历史重传成功恢复 token（重启前 2.229 秒，重启后 2.260 秒）。这证明可重建多轮上下文，不是服务端落盘持久存储。见 `docs/research/opencode-deepseek-restart-20261018.json`。

历史导入的两次失败保留在 research：缺少 completed 导致 assistant 丢失；自行生成 session ID 导致免费上游拒绝。最终使用官方创建的 session ID 和已完成原生消息格式通过。参考官方 [session transfer 源码](https://github.com/anomalyco/opencode/blob/dev/packages/core/src/session/transfer.ts)，没有复制源码。

64 条消息边界测试最初因官方默认分页触发 history_import_incomplete；显式 limit=100 后，最早 assistant 精确召回通过（1.444 秒）。旧失败和修复后的 JSON 均保留。这是消息条数测试，不代表模型完整上下文窗口或长文本质量已验证。

有限连续测试 364.827 秒、12/12 成功，历史 2→24 条；延迟 min/median/max 为 1.482/1.8755/7.527 秒。期间一次受控桥接重启，官方服务未重启；不是不间断进程 SLA。结束时 session=0、db 文件=0。Loadout 隔离与生产完整历史 JSON/SSE 通过，原 big-pickle 同样通过。结果见 `docs/research/opencode-deepseek-continuity-20261018.json` 与 `opencode-deepseek-loadout-20261018.json`。

复验：`LOADOUT_LIVE_TESTS=1` 后运行 `probe_opencode_restart.py`（独立临时服务）、`probe_opencode_continuity.py`（有限 12 次，间隔 30 秒）、`verify_opencode_bridge.py --loadout <exe> --bridge-dir <dir> --model deepseek-v4.1-flash-free --history`（Loadout JSON/SSE）。有限样本不代表长期 SLA。

## Function tools 与实时 SSE（ExecPlan 028）
DeepSeek 模型 catalog 声明 TOOLS；big-pickle 仅文本/流式，未声明工具能力。tools 仅 function，最多 32 个、名称 `[A-Za-z0-9_-]{1,48}`，parameters 必须是 object JSON Schema。028 原先仅支持 tool_choice=auto/none；029 扩展见下。strict=true 明确拒绝。parallel_tool_calls=false 时若上游实际产生多调用则报错，不悄悄执行额外调用。

每请求用官方 runtime MCP 注册客户端 schema（内存 overrides、codemode=false），权限仍 ask。桥接只把原生 name/id/arguments 映射为 Chat tool_calls，MCP tools/call 不执行任何代码。客户端执行后把原 assistant 调用和 role:tool 结果连同完整历史重传。桥接导入 completed native tool records，以空 synthetic input 唤醒 Agent；模型能读取真实外部结果，不把工具结果拼到用户提示里。服务无需保存跨请求 pending 状态，因此请求之间重启也不会丢失客户端完整历史中的工具结果。

工具目录、session 与 event subscription 在完成/异常/断开时清理。清理失败后拒绝新推理。客户端断开会取消正在生成的请求。事件流丢失、模型不符、工具参数不完整或文本增量与最终文本不一致时返回错误；已开始 SSE 的错误流不输出正常 stop/DONE。只监听 loopback，内部 MCP 使用独立随机内存 key。

工具参数流不是逐 token：官方当前仅可靠提供完整 input.ended 边界；文本是真实实时增量。保留官方 Agent 系统上下文与 system update 的语义，不能称为无损通用 OpenAI 接口。

验证：`LOADOUT_LIVE_TESTS=1` 后 `python scripts/verify_opencode_agent.py`；`--isolated-loadout target/release/loadout.exe` 验证隔离 Loadout；`--production` 验证安装实例并撤销临时 client。基础边界测试为 `python -m unittest discover -s scripts -p test_opencode_bridge.py`。部署时需同时复制 opencode_bridge.py、opencode_protocol.py、opencode_events.py。

### 本次验收结果
生产网关实时文本 20 个增量，TTFT 2.787 秒 / 总 4.917 秒；JSON 与 SSE 各完成一轮工具调用和结果续写，双工具反序回传仍正确。隔离 Loadout 也通过，客户端首段后断开可取消官方会话。Python 15 tests、Rust Provider 2 tests、workspace clippy 与 release build 通过。没有 Router/Gateway/schema 修改，不需对应 benchmark/migration；未进行面板视觉验收。

验收记录：[生产](../research/opencode-agent-production-20261018.json)、[隔离 Loadout](../research/opencode-agent-isolated-loadout-20261018.json)、[原生事件与工具](../research/opencode-events-tools-probe-20261018.json)。这是合成任务可用性验证，不代表所有 Agent 客户端（其可能携带 strict/tool_choice/max_tokens/Responses 特有参数）都兼容，也不代表长时间 SLA。期间一次工具续轮精确字符串断言失败未保存差异详情，原因 UNKNOWN；后续隔离与生产均通过，不能据此称零失败。

备份指针 `%LOCALAPPDATA%/LoadoutOpenCodeBridge/agent-backup-path.txt`，保留上轮脚本和二进制。未添加系统自启。

## 029：真实编码 Agent 验证与当前阻塞

候选支持 max_tokens / max_completion_tokens（二选一，1–32768）、store=false、空 reasoning_content，以及 auto/none/required/指定 function。使用官方 session HTTP hook，内存控制参数、Basic 认证、按 session 校验、一个 primary attempt；不修改官方 binary、身份或原生工具 schema。小预算可能全用于推理，返回 content=null、finish_reason=length；不是代理截字符串。非空推理历史、strict=true、图像、其它采样参数仍拒绝。

真实 OpenCode 2.0.32 的 run CLI 经隔离 Loadout，连续三次完成小型 Python 编码任务。最终 18.131 秒、5 次精确路由请求、4 轮工具调用：read/glob → shell 失败 → write → shell 成功。原测试 hash 不变，独立 unittest 与隐藏迭代器/浮点/大整数断言通过。每轮携带 max_completion_tokens=4096。实时文本、JSON/SSE 工具往返、反序工具结果和断开清理回归通过。默认 Python 19 tests、Node hook test 通过。没有 Rust、Router/Gateway 或 schema 修改。

客户端需加载 `scripts/opencode_loadout_client/` profile，让标题在本地生成，避免标题与主任务争用单并发。测试额外设置 noRetry=true 以限制失败重试；常规使用可不设置。官方支持自定义 Provider 与本地 plugin，但这只证明带该 profile 的 OpenCode，不证明所有编码 Agent 零配置兼容。

候选客户端配置（上游恢复、029 重新验收部署后才能使用）：
```json
{
  "model": "loadout/coding",
  "plugins": ["D:/Loadout/scripts/opencode_loadout_client"],
  "providers": {
    "loadout": {
      "env": ["LOADOUT_API_KEY"],
      "package": "@opencode/ai/providers/openai-compatible",
      "settings": {"baseURL": "http://127.0.0.1:8317/v1"},
      "models": {"coding": {
        "modelID": "opencode-free/main/deepseek-v4.1-flash-free",
        "capabilities": {"tools": true, "input": ["text"], "output": ["text"]},
        "limit": {"context": 64000, "output": 4096}
      }}
    }
  }
}
```
上述 context 是客户端保守配置，不是已验证模型容量；桥接仍为 64 KiB/64 messages/45 秒/单并发，长仓库任务可能超过正文上限。

最终生产 gate 未通过：首次生产编码任务 502，官方事件为 403 FreeTierError；新建隔离服务也返回模型失败。回滚 028 后一次最小文本调用仍得到同样 403，故目前不能归因于候选兼容修复，也不能称生产可用。未伪造身份、未采用筛掉原生工具后的绕过方案、未切模型。所有临时 client 已撤销，session=0、MCP=[]、db 文件=0。生产 daemon/GUI 未重启。

证据：[编码任务](../research/opencode-coding-agent-20261018.json)、[参数专项](../research/opencode-controls-20261018.json)、[生产失败](../research/opencode-coding-agent-production-failure-20261018.json)、[回滚基线](../research/opencode-coding-agent-rollback-20261018.json)。历史失败记录同目录保留。备份指针 `compat-backup-path.txt`。

复验：显式 `LOADOUT_LIVE_TESTS=1`，运行 `python scripts/verify_opencode_coding.py`、`python scripts/verify_opencode_controls.py`；前者只有独立断言通过才退出 0。`--production` 创建并撤销临时精确模型 client。生产恢复后仍须重新执行完整 gate，不能仅凭 /health（本地进程健康）判定免费模型恢复。

## 官方直接调用对照：403 来源（2026-10-18）

本轮按用户要求只调查，不更新、重启或调用正式桥接。使用同一个官方 2.0.32 binary（SHA 见 receipt），同一个 `opencode/deepseek-v4.1-flash-free`，独立 XDG/home/工作区与内存 DB，不导入现有凭证。官方 CLI 和 serve 对照均未加载 Loadout plugin，也没有经过 Loadout 网关或 Python bridge。

| UTC 时间 | 路径 | 结果 |
|---|---|---|
| 10:24:30 | 官方 run --standalone，无插件 | 403，Detect Abuse Traffic |
| 10:24:33 | 官方 serve，直接 session API，无插件 | 403，同一消息；事件确认精确模型 |
| 10:24:41 | 隔离候选 bridge | 502 official_model_failed |
| 10:24:42 | 官方 run 再次对照，无插件 | 403，Detect Abuse Traffic |
| 10:25:52 | 官方 run 原生读取合成 probe.txt，无插件 | 推理阶段即 403，尚未调用 read |
| 10:26:01 | 隔离 bridge，额外观察原生事件 | bridge 502；原生事件确为 403，同一 Detect Abuse Traffic |

因此当前拒绝来自上游免费网关风控，移除桥接仍复现。这里的“Abuse”是服务端错误分类，不代表已经证实用户滥用。官方公开 [inference-proxy.ts](https://github.com/anomalyco/opencode/blob/60a5a6ca7ce176b04cd2321ffd1819b1ea8e6006/packages/console/app/src/lib/inference-proxy.ts) 将认证与免费资格交给独立 inference 服务，本次未获得它的具体规则/日志。[官方模型页](https://opencode.ai/v2/docs/console/models/) 仍列出该免费模型，不能据此解释成已下架。

历史事件时间线：10:09:23–10:09:36 UTC 编码任务成功；10:12:52–10:13:06 UTC 最后一份成功任务仍完成 5 次调用；10:14:01 UTC 生产任务已失败。间隔约 56 秒，但两份 receipt 不是完整网关日志，不能据此定位准确触发时刻。后来 rollback 028 也失败，今天的官方直接对照进一步表明当前故障不局限于兼容修复。

归因边界：可以确认“当前上游风险拒绝”；不能确认是否之前密集测试/大量新 session/桥接请求形状触发，也不能断言按 IP、账号、设备或全局限制。没有 429、余额耗尽或 Retry-After 证据，不宣称冷却时间。当前状态也无法排除原来的桥接流量曾导致共享限制。没有换 IP、伪装身份、修改工具 schema 或尝试绕过。

证据：[完整对照](../research/opencode-403-control-20261018.json)、[事件时间线](../research/opencode-403-timeline-20261018.json)。诊断脚本 `scripts/probe_opencode_403.py` 默认至多 6 个顺序用例，必须显式 opt-in；当前已停止继续探测。隔离子进程和临时目录已清理，正式服务未动。未来若需确认触发因素，应在官方恢复后先做低频原生基线，再做最小桥接对照；准确命中规则需官方服务端支持确认。

### opencode2api 参考审查（2026-10-18）
固定 revision `83d623d541c47a17cd832ee4f53bd7c95f3a18fa`，许可证与具体文件见 reference-map。结论：有协议实现参考价值，但没有证明可解除当前 `Detect Abuse Traffic`。
- 身份层生成官方格式 session，声明 CLI User-Agent；匿名入口优先 legacy Zen，随后 inference；失败可尝试下个入口/代理。没有官方 serve，不能视为官方客户端对照。
- inference 请求补入官方内置工具；同名客户端工具也被替换为捕获的官方定义，可能改变调用参数契约。legacy 路径重命名 read 类工具并恢复下游名字。不能把支持 tools 宣传等同任意编码 Agent 无损兼容。
- 参数转换和 SSE parser/emitter 存在，但本轮未执行该网关，未证明本机 DeepSeek max_tokens、tool_choice 或连续工具回合可用。上次真实官方 CLI/serve 的 403 证据仍成立于其记录时间，不据此声称当前实时状态。
- Issue 120 仍有免费层 403 反馈；109 提到 IP 池无效。它们是社区报告，不是恢复 SLA，也不能推断本机 IP 被封。
- ADR-017 明确不模拟身份/工具定义，本轮仅静态检查；没有新的上游调用、安装、生产配置或进程变更。现有 029 保持 BLOCKED_EXTERNAL。待官方原生调用恢复，再重跑参数与真实编码验收。

### opencode2api 隔离实测（用户追加授权，2026-10-18）
10:35:22–10:35:28 UTC（悉尼 21:35），按官方 CLI → opencode2api → 官方 CLI 的顺序，固定 `deepseek-v4.1-flash-free` 和同一 HELLO prompt，三次均为 403 / `Forbidden, OpenCode Free Model Gateway Detect Abuse Traffic`。opencode2api 已成功监听随机 loopback 端口、完成模型发现并列出精确模型，其 Chat 请求在 0.525 秒返回上游错误；不是启动失败或模型名不存在。

第三方固定 commit 83d623d，Go 本地编译；zip 逐文件对照确认只修改 runtime.go 一处，关闭全模型自动 availability 探测，其推理请求路径不改。匿名 public、direct 网络、无上游账号 key、无代理池、无模型轮换，max_attempts=1，45 秒总请求预算。该参数不关闭项目原有 legacy/inference 入口遍历；本次只捕获最终 HTTP 响应，未记录每个入口的独立状态，不能声称两个入口分别已确认同样 403。

基础调用失败，按用户条件不执行 SSE、tool_choice、max_tokens 或多轮工具扩展。此次证明该版本在本机当前网络和时间窗口也不能恢复调用，不证明全球不可用、永久封禁或 IP 是唯一原因。

隔离进程 PID 208508 已停止，临时状态目录（含本地随机 key）已删除；生产官方 PID 211364、Loadout PID 128088 保持原实例，四个部署脚本与回滚备份 SHA 相同。源码/编译产物仅保留在临时研究目录，未安装。

证据：[实测回执](../research/opencode2api-live-20261018.json)，脚本 `scripts/probe_opencode2api.py`（需事先准备固定源码构建及显式 LOADOUT_LIVE_TESTS=1）。没有新增生产集成代码；029 保持 BLOCKED_EXTERNAL。

### DESN131 隔离实测（2026-10-18）
固定 `b48d93ef91a6eae3c71f04b99a49294e7af9d28a` 原始 mjs，直接下载到临时目录运行，不改第三方代码、不引入 Loadout 依赖。随机 loopback 端口，无真实上游凭证、模型回退或网络切换，默认 legacy Zen 入口。10:49:00–10:49:07 UTC 官方 CLI → DESN Chat → 官方 CLI，同一 DeepSeek 和 HELLO prompt，均为 403 `Forbidden, OpenCode Free Model Gateway Detect Abuse Traffic`。DESN 请求用时 0.285 秒，错误为 FreeTierError；本地 health 和远端模型发现成功且包含精确模型。

基础调用失败，按用户条件跳过流式、tool_choice、max_tokens 和连续工具测试，不能把这些能力记为通过或实测不支持。隔离进程及目录已清理；正式官方/Loadout 仍为原 PID，部署四脚本匹配回滚备份。本结果仅说明本机本时间窗口仍拒绝，不能推断永久不可用或具体阻断维度。证据：[回执](../research/desn-opencode-live-20261018.json)，脚本 `scripts/probe_desn_opencode.py`（显式 live opt-in）。

### 网络出口诊断（2026-10-18 10:53 UTC）
Windows Internet Settings ProxyEnable=0、无 PAC，WinHTTP direct；进程/用户/机器 proxy 环境变量未配置，监听端口及常见代理进程未发现现成代理。IPv4 默认路由只有 WLAN，无 IPv6 默认路由；目标域名 trace 显示 loc=AU、warp=off（不排除路由器/运营商层转发）。官方隔离 DeepSeek 基线仍 403 Detect Abuse Traffic。没有第二个已验证不同出口，故出口因果对照未完成，IP 限制仍 UNKNOWN。已请求用户提供现成代理地址或第二网络；没有修改系统代理/路由或正式服务。证据：[网络回执](../research/opencode-network-control-20261018.json)。

### 手机 5G 热点对照（2026-10-18 10:55 UTC）
用户自行从宽带 Wi-Fi 切换手机 5G 热点后，同一官方 binary、同一 DeepSeek、同一 HELLO prompt、相同隔离配置连续 2/2 成功（3.440 秒、4.527 秒），无工具/错误。之前宽带基线 10:53:34 为 403 Detect Abuse Traffic。热点 trace 仍 loc=AU/warp=off；未保存前后公网 IP，不能断言具体 IP 被封；没有切回宽带完成 A-B-A，时间相关恢复尚未完全排除。证据强烈支持网络路径相关因素，优先于纯桥接格式问题。此结果只验官方文本可用性，不代表桥接/工具/生产验收。正式服务未修改。回执 opencode-hotspot-control-20261018.json。

### 热点下代理与真实编码复验（2026-10-18）
用户授权隔离两条路线，成功后真实编码；禁止生产更新。DESN原始b48d93e源码未改，Loadout使用仓库候选029（不是正式回滚028）。

| 验证 | DESN131 | 隔离 Loadout 候选桥接 |
|---|---|---|
| 实时 SSE | 39文本增量，首文本3.768s/总4.493s | 18文本增量，首文本2.598s/总4.627s |
| max_tokens=16 | JSON/SSE均length | JSON/SSE均length |
| tool_choice | none、required、指定lookup_code通过 | 同左 |
| 连续工具 | 三轮调用，最终精确汇总三份随机结果 | 同左，使用已支持的消息顺序 |
| 真实编码 Agent | 5工具轮/6请求，14.117s | 4工具轮/5请求，13.765s |
| 编码产物 | 原测试不变、独立测试和隐藏检查通过 | 同左，且每次精确模型route header通过 |

编码任务由真实官方OpenCode客户端在临时工程执行，读calculator.py/test、观察失败、编辑total、运行测试成功，非只描述操作；每次编码请求带max_completion_tokens=4096。本轮小预算以finish_reason=length验证终止语义，未独立计数上游token；零可见文本可由推理预算耗尽造成。

兼容限制：Loadout首个工具序列包含连续两条user，现有校验返回400 invalid_message_order。没有默默修改产品行为；保留初始失败回执，改成单user起始后继续工具验收。后续回执标记6项先前检查复用，本轮不是所有项目零失败。默认19测试通过。生产四脚本SHA匹配回滚备份，正式三个进程未重启；隔离session=0、MCP=[]、db=0。无生产升级，不证明原宽带恢复、任意客户端兼容或持续免费可用。

回执：[DESN热点](../research/desn-opencode-hotspot-20261018.json)、[Loadout热点](../research/opencode-loadout-hotspot-20261018.json)、[初始消息顺序失败](../research/opencode-loadout-hotspot-initial-failure-20261018.json)。验证入口 scripts/verify_hotspot_capabilities.py 与 probe_desn_opencode.py 的显式热点开关。

### 并发与连续 user 候选修复（ExecPlan 030，未部署）
候选代码允许连续 user 按原顺序导入；两个独立常驻官方 worker、最多8等待，Provider admission从1改10。24项Python、1项Node及2项Provider测试通过，fmt/clippy和独立release构建通过。真实双工具/双编码验收仍未完成，不能宣称正常编码Agent并发已可用。

2026-10-18 11:17 UTC，当前手机热点上官方无插件CLI同模型单次HELLO再次403 Detect Abuse Traffic；隔离桥接原生事件亦同样403。不是仅Loadout报错，具体上游判定原因未知。停止扩展实测，不部署030；两个测试worker session=0/MCP=[]，进程已清理。正式029 daemon PID176208及桥接/官方进程未重启，五个部署脚本hash与部署029一致。

回执：[官方对照](../research/opencode-concurrency-official-control-20261018.json)、[隔离候选](../research/opencode-concurrency-isolated-20261018.json)。待官方单次恢复后重跑全部并发编码gate，再按授权更新正式环境。

### 官方低频复测（2026-10-18 11:20–11:25 UTC）
不改变热点/配置/版本，官方无插件同一DeepSeek三次单次HELLO：403、间隔168秒后403、再间隔135秒后成功。未做并发刺激，正式服务未变；恢复后停止追加请求。支持暂时性限制/波动，不能确认调用频率或并发触发，也未证明持续稳定。公网IP变化及其它设备流量未观测，不能将同SSID等同固定出口。回执：[低频对照](../research/opencode-low-frequency-20261018.json)。030并发编码验收及部署仍未完成。

### 低频稳定性gate未通过（2026-10-18 11:51–11:56 UTC）
官方同模型单次结果为403→成功→403，间隔193秒和120秒；全程没有双并发或编码Agent调用。未达到连续3次成功的有限窗口门槛，停止升级，不部署正式。403在低频串行下即可复现，无法证明频率或并发是起因。回执：[逐级恢复](../research/opencode-staged-recovery-20261018.json)。

### 只读差异审计修正（2026-10-18）
无新增模型调用、无正式变更。官方v2.0.32源码显示新根会话可能异步生成标题；无plugin CLI探针未覆盖title，因此此前“一次CLI启动”不能作为“一个上游HTTP请求”的证据。桥接和编码client profile已有本地title hook。403默认归Authentication且不重试，响应头x-should-retry可覆盖；历史头和临时详细日志未保留，不能证明实际attempt数。后台5分钟模型目录刷新和桌面10分钟软件更新有日志/源码支持，但不是推理调用。当前两次IPv4/IPv6网络trace各自出口不变；历史成功/失败出口未记录，IP变化仍UNKNOWN。详见ExecPlan030只读审计及[回执](../research/opencode-readonly-diagnosis-20261018.json)。

### v0.4 连接保护与当前限制（ExecPlan 031）

403 根因仍未确定：官方无插件 CLI 在低频下也失败；历史成功与失败缺少同期出口 IP 和完整 HTTP attempts，不能断言 IP 封禁或并发触发。此前工具与流式成功仅代表当时窗口，不能承诺持续可用。

正式发布使用单 worker、账号并发上限 1；桥接全局两次请求开始至少间隔 120 秒。上游 403 后冷却 30 分钟，截止时间落盘并在重启后保留，损坏状态按冷却处理。不自动重试、轮询推理或探测恢复；官方 title 本地返回。桥接拒绝返回 429 和 Retry-After，已开始 SSE 后的上游失败以流内错误终止。网关可能将桥接错误映射为自己的路由错误，客户端不能假设所有入口均透传 Retry-After。

连续 user 消息导入保序通过离线测试；双 worker 不启用，030 的真实并发编码验收仍未完成。已有的 SSE、tool_choice、max_tokens、工具历史能力保留，但每个工具回合都受 120 秒间隔限制，客户端需要处理限流。64 KiB/64 条消息、单次执行 45 秒等边界仍在，不宣称任意编码 Agent 无损兼容或长期稳定。

主 Release 更新 Loadout 程序。外部桥接脚本需另取同 Release 的 `Loadout-0.4.0-windows-x64-opencode-bridge.zip`，解压后运行 `powershell -NoProfile -ExecutionPolicy Bypass -File .\Update-OpenCodeBridge.ps1`。只适用于已安装的官方 2.0.32 桥接；脚本核验进程归属，备份、替换并重启，失败回滚，不做推理请求。不包含官方 binary、Python 或凭据，未完整部署任何第三方网关。内置软件更新不会自动覆盖外部桥接目录。
