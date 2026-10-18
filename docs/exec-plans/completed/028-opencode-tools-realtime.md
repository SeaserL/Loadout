# ExecPlan 028: DeepSeek 工具往返与实时流验证

- Status: Completed, 2026-10-18

## Goal
验证官方 OpenCode 免费 DeepSeek 的外部工具调用与实时增量，成功的能力接入现有 Loadout，不部署其它网关。
## Context
用户要求超出多轮文本，指定 AxonHub 作为参考。
## Existing behavior
ADR-017：官方 2.0.32 常驻进程、每请求内存 session、文本历史 import、缓冲 SSE、工具请求拒绝。相关代码与测试已读取；保留其它未提交工作。
## Constraints
官方二进制不修改；不伪装上游身份；不执行客户端提供的任意命令；不偷偷换模型；不保存真实正文。
## Architecture impact
优先仅改 bridge 与 Provider capability。工具必须有真实 schema、call ID、arguments、result 往返，不能把提示词 JSON 冒充原生工具。
## Files expected to change
验证脚本与 receipts；scripts/opencode_bridge.py、测试、Provider（仅验证通过的能力）、ADR-017、Provider/reference 文档。
## Implementation stages
1. 官方 API/source 和 AxonHub targeted source 研究。
2. 隔离合成输入验证实时事件及工具往返，不执行文件/网络工具。
3. 先修订 ADR，再实现已验证能力与失败/取消路径。
4. mock、真实隔离 Loadout、正式链路验证与部署。
## Tests
LOADOUT_LIVE_TESTS=1。验证首增量早于结束、多个增量、exact model；工具原生调用、参数与外部结果返回、权限隔离、断流/取消/清理。
## Performance validation
记录 TTFT/总耗时与事件数量；Router/Gateway 改动则跑 benchmark。
## Security considerations
用隔离会话与合成工具；默认官方工具仍不批准。认证不输出，工具结果与正文不落盘。动态工具若需插件/MCP，先确认作用域和生命周期。
## Migration considerations
无预期 schema 变更；本机服务改动备份后部署。
## Known risks
免费通道限制、事件流丢失/溢出、工具由官方 Agent 自行执行而非外部 handoff、客户端断开后泄漏会话。
## Definition of done
每项能力有实际证据；可行的实现完成接入；不可行项明确具体阻塞，不宣传完整 Agent 兼容。
## Progress
- 已完成入口文档、ADR、当前实现和测试阅读。官方 event API 是 volatile SSE；prompt 不接受 tools 字段，继续检查扩展接口。
- 官方隔离实测：19 个 text delta，首段 2.264 秒、结束 4.236 秒。runtime MCP codemode=false 注册后真实 call ID / 参数产生，权限停在 ask，执行次数 0。completed tool history + 空 synthetic resume 后准确返回外部 token。
- AxonHub 固定 commit 已读 targeted stream 源码、根 LICENSE/NOTICE 与 llm/LICENSE；llm 为 LGPL-3.0，仅参考概念。已更新 reference-map 与 ADR-017，开始实现。
- 已实现有界事件流、严格消息/tool result ID 校验、官方内存 MCP 目录、外部工具交接、空 synthetic continuation、取消和清理。DeepSeek catalog 增加 TOOLS，big-pickle 保持文本能力。
- 隔离 bridge 实测 JSON/SSE 工具往返通过；文本 22 个增量、首段 2.758 秒 / 总 5.194 秒。客户端首段后断开，官方 session 被取消清理。剩余 session/MCP/db 均为 0。
- Python 15 tests、Provider 2 tests、workspace clippy、release workspace build 通过。开始隔离 Loadout 验证，增加双工具反序结果回传反例。
## Decisions
- 官方文档/源码为主要依据，AxonHub 为本任务唯一社区实现参考；不复制代码，先研究协议状态处理。

- Loadout 隔离实时 SSE 和 JSON/SSE 工具往返通过；双工具调用反序结果回传通过，断开清理通过，session/MCP/db=0。
- 验证过程中出现 response.model 与 Loadout route name 混用的断言错误，已按上游 ID 与 x-loadout-route 分别校验。另一次工具续轮精确字符串断言失败，首次未采集差异详情，原因 UNKNOWN；增加失败 receipt 后全链路复测通过，不把初次失败删除或当作已定位缺陷。
- 正式脚本与二进制已备份后部署，model sync 更新 2 个模型；正在生产最终验证。备份指针 `%LOCALAPPDATA%/LoadoutOpenCodeBridge/agent-backup-path.txt`。

- 生产实时文本 20 delta，TTFT 2.787 秒，总 4.917 秒；JSON/SSE 单工具往返与双工具反序结果回传通过。实际响应 exact upstream ID 和 x-loadout-route 均校验。
- big-pickle 生产多轮 JSON/SSE 回归通过。最终 session/MCP/db 文件均为 0。临时测试服务及临时 client/凭证已清理。
- 未修改 Router/Gateway/schema，无对应 benchmark/migration；未推送，未添加自启，未做 GUI 视觉验收。完成范围为 Chat function tools 与实时文本 SSE；参数片段实时逐 token、strict/forced tools/其它采样参数、完整 Responses/任意 coding client 兼容未宣称。
