# WorkBuddy 首行兼容适配与实际 Codex 验证

ExecPlan 049；2026-10-30。正式环境未修改，未做 UI。

## 适配结果

已实现并接入隔离 relay 的 **27 个非空行纯函数**：`development/experiments/omniroute-phase10/workbuddy-first-line.mjs`。只识别第一条 system 消息、首个 text 部件或字符串内容的完整已知首行，输出固定为048验证过的 OmniRoute 原生规则结果。

- 精确匹配整行；仅兼容原有 LF/CRLF，不忽略前导空格、不做模糊匹配。
- 只改首行。后续行、后续 system、其他 text/image 部件、用户消息、工具 schema、工具参数及结果不扫描、不替换。
- 不修改输入对象。relay 中通过 `firstLineCompatibility: true` 显式开启，默认关闭；不添加新的 runtime dependency。
- 未知首行原样通过，保留真实上游拒绝；不会自动回退成全文替换。
- 修改点位于协议转换之后、WorkBuddy 发出请求之前。OmniRoute 后台、路由、SQLite、凭证管理和冻结包均未改动。

这里“其他内容不变”指本次兼容函数的输入输出字段值与文本。原 relay 已有的 SSE 及输出 token 上限处理仍保留，未将它们伪称为全请求 wire bytes 不变。

## 范围验证与成本

22 项离线回执全部通过，包括：原生替换结果等价、字符串/text 数组、LF/CRLF、无换行、重复身份句、后续 system、用户正文及路径、工具 schema/arguments/result、多模态部件、未知词句不匹配、默认关闭、完整 loopback relay。恢复唯一首行后对整个 body 做 deep equality，覆盖范围外数据；输入被 deep-freeze，确认没有原地修改。

另有原 phase8 凭证/relay 的12项回归全部通过。新增工具验收负例明确拒绝把 `Goal tools require a persistent thread.` 或单独最终 marker 当成工具成功。

小输入与150KB文本各运行10000次纯函数 benchmark，最终回执 p99 均约1.7微秒。这只是纯函数成本，不含 JSON 编解码、网络、模型时间；未据此声称端到端延迟下降。没有 Router/Gateway 后台代码变化，不新增上游 hook。

## 实际 Codex 文本和长会话

客户端实测为本机 `codex-cli 0.165.0-alpha.1.2`；使用独立 HOME/workspace、read-only sandbox、临时 client key，以及固定完整 OmniRoute 候选包。真实账号仅由 relay 只读解密到内存，客户端从未获得 Provider token。

固定 WorkBuddy 当前绑定账号和 `deepseek-v4.1-flash`，单并发、单请求65秒、最多2048输出token。本轮调用总预算16；没有换账号、换模型、增加补丁来消除失败。

| 验收项 | 实际结果 | 证据 |
|---|---|---|
| Codex 文本 | PASS | 真实客户端退出0，FUSION_OK；1次上游200 |
| 长会话第1轮 | PASS | 95,380 B、4条消息，取回合成marker |
| 同一会话第2轮 | PASS | 150,033 B、6条消息，明确按同一session ID恢复 |
| 同一会话第3轮 | PASS | 204,686 B、8条消息，仍取回最早marker |
| 文件读取工具 | **BLOCKED：本机执行策略** | 上游正确返回exec_command，Codex拒绝执行命令 |
| 持久会话只读 get_goal | PASS | 真实调用get_goal({})，Codex返回有效goal=null结构，再正常完成回答 |

长会话是实际 Codex `exec`→`exec resume <同一ID>`，不是手工构造 history 的 HTTP 重放。上下文和消息数确实增长，没有删除旧消息；所有上游响应均保留同模型并完整结束。该结果不覆盖自动压缩、数小时运行或并发会话。

回执：初轮真实验收。

## 文件工具阻塞的独立定位

初轮工具任务和一次定点重跑都返回：WorkBuddy 200，工具名 `exec_command`，合法 JSON 参数，命令精确为 `Get-Content -LiteralPath ./acceptance.txt`。客户端将失败结果回传，上游正常完成后续回答，但文件未读到，不能判为工具成功。

随后使用相同完整后台和真实 Codex，仅将上游替换成本地 mock，返回完全相同的只读工具调用。产生的工具失败结果与真实链路哈希一致：`879b646e6ec67cc46c1d7847ad8e8f1877ce5ffa746f5942377e29fef0cb7f1f`，长度316 B。错误含 `exec_command failed`、`CreateProcess`、`blocked by policy`；被拒绝的是本机 bundled PowerShell 的这条命令。

这把该失败定位到隔离 Codex 的本地执行策略，不能归因于 WorkBuddy 11128、工具参数被首行适配改写或协议完全不支持工具。本轮未关闭 sandbox、未放宽 approval、未改正式客户端策略，也未换命令包装规避拒绝。

回执：真实工具定点诊断、零 Provider 调用的本地复现。本地错误正文中的机器路径已去掉，仅保留分类和用于同源比较的哈希。

补充 `get_goal` 验证用于区分无进程只读工具和命令执行；它不代替文件读取验收。第一次使用 ephemeral 会话得到“需要持久会话”，虽然模型输出了预期 marker，也被纠正为 FAIL，保留在 ephemeral负例。最终持久会话结果另见 工具回执。

持久会话补测通过：工具名get_goal、参数{}，工具返回`{"goal":null,"remainingTokens":null,"completionBudgetReport":null}`，模型收到结果后完成回答。以工具真实返回内容验收，不以最终marker或CLI通用事件标签推断成功。未创建或修改任何目标。

## 安全、清理与继续条件

只有首行适配模块及实验 relay 入口是本轮运行时改动，其他新增内容为测试、诊断和文档。开关关闭即回到原行为，无 schema migration 或正式数据变更。凭证仍不进入 SQLite、日志或fixture；工具诊断没有写入真实请求/响应全文。

最终候选包、凭证扫描和清理结果见 审计与清理。复现命令及预算见 phase10 README。

- WorkBuddy真实推理共12次，低于16次预算；商汤/CommandCode为0。另有本地mock请求，不计作真实Provider成功。
- 1224个本轮实验/运行文件扫描当前WorkBuddy access token的UTF-8/UTF-16明文，命中0；这限定于当前token和列明扫描范围。
- 21,732个候选文件校验通过，原429补丁不变，新增后台hook为0。
- 各次运行的官方auth/binding及检查范围内正式客户端配置前后哈希一致。
- 核对52个历史隔离实例：剩余测试凭据0、仍存活的实验后端PID为0；所有本轮CLI命令均已退出。
- 新增脚本语法与Prettier检查通过；没有Rust、正式GUI或数据库变更，因此未重复无关的Rust全套检查。

**建议保留这个最小适配，继续 OmniRoute 窄融合；不因本地命令执行策略阻塞扩大首行规则或重写路由。** 正式迁移仍不批准；文件工具日常可用性需在正常允许该只读命令的隔离客户端策略下另验，当前保留 BLOCKED，不能把文本和长会话成功等同完整日常工具验收通过。
