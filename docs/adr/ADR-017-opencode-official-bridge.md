# ADR-017: OpenCode 官方常驻服务桥接

- **Status**: Accepted for user-requested integration, 2026-10-18
- **Amends**: ADR-009（仅本人本机未修改官方 OpenCode 的免费模型）

## Context
用户明确要求验证官方常驻服务后接入，不部署 9router。官方 v1.19.3 免费模型被拒绝；官方 v2.0.32 会话保留原生工具、权限 ask 时 big-pickle 返回预期文本。全部 deny 会移除工具并导致上游 FreeTierError。

## Decision
- ExecPlan030：允许两个独立常驻官方worker提供并发，每worker独立workspace/配置/内存DB与工具目录；桥接最多8等待，等待最长45秒，取消不影响其它请求。单worker仍独占请求，禁止仅删除原共享状态锁。连续user按原消息顺序保留导入，实测通过后部署。
- 新 Provider `opencode-bridge`，slug `opencode-free`，trust `CLI_BRIDGE`，逐账号 opt-in，排除默认虚拟路由。
- 只调用未修改官方 `serve` 的本地 API 与官方 plugin hook。不复制或模拟其上游身份、工具定义、session 算法；不安装 9router。
- 独立 XDG/home/空工作区；官方数据库使用 `OPENCODE_DB=:memory:`，不保存对话正文到数据库文件。不读取用户已有凭证/工程/插件。
- 每个 API 请求独立官方 session；权限全部 ask，桥接永远不批准工具；已声明外部工具在权限边界交给客户端，其它工具请求中断并报错；完成/失败均删除 session。不是操作系统 sandbox。
- 暴露实测 big-pickle 与 deepseek-v4.1-flash-free。单并发、请求上限 64 KiB、最多 64 条消息、总等待上限 45 秒，无跨模型回退。
- **Agent 文本兼容桥接**支持完整 user/assistant 历史（交替、末条 user），可有首条 system。用官方 experimental import 导入已完成的历史消息，保留官方创建的 session ID；校验导入完整性，仅返回本次新增 assistant。system 为官方 Agent 的 system update，不承诺原始 system role 优先级。支持下述已验证 DeepSeek function tools；拒绝 vision、采样参数、stateful 请求。输出长度参数见 ExecPlan 029 扩展。官方 Agent 自身系统上下文不可移除。
- 多轮状态由客户端每次重传完整 messages 恢复；服务不落盘保存对话，进程重启丢失原生会话。此兼容范围不等于服务端持久会话或完整 OpenAI Chat 语义。
- DeepSeek 通过官方 `/api/event` 增量转为实时 Chat SSE；订阅完成后才发 prompt，按 session/message 过滤。首个有效内容前不提交响应，流丢失/错误不得伪造 stop/DONE。big-pickle 同路径须回归验证。不得把官方 HTTP 200 当成功。
- 外部 function tools 通过官方 runtime MCP 注册（内存覆盖层，codemode=false），只用于让官方模型接收真实 schema；所有执行权限仍 ask，桥接不批准，不执行 MCP tools/call。模型结束工具生成后，把原生调用映射为 Chat tool_calls，由客户端执行。
- 后续请求必须包含完整 assistant tool_calls 与匹配的 role:tool 文本结果；转换成官方 completed tool history，通过空 synthetic input 唤醒原生 Agent 继续生成。这是 Agent 兼容层，不是无损纯模型 API。禁止缺失/重复/未知 tool_call_id、未完成参数、工具名或模型偷换。
- 单请求动态 MCP 目录只在锁内有效，结束后移除。工具注册与会话清理失败则关闭推理能力直到重启。工具选择见下述扩展；不声称支持 strict schema 保证或任意采样参数。请求边界仍 64 KiB / 64 条消息。
- 两个端口仅监听 127.0.0.1，分别用随机 Basic / Bearer secret 保护，部署 secret 用 Windows DPAPI，Loadout key 进 Credential Manager。不在日志或错误中回显正文。
- 使用现有平台 spawn_detached 启动常驻服务；不随单次请求启动 CLI。桥接/官方服务失败时报错，重启由显式启动脚本处理；本次不增加系统自启。

### ExecPlan 029 扩展（热点隔离及正式验收通过）

2026-10-18 用户授权正式部署，五文件SHA及plugin载入核对通过，正式Loadout8317端口完成参数/实时SSE/连续工具与真实编码验收（15成功/0失败），big-pickle JSON/SSE基础回归通过。以下旧403/回滚记录为历史，网络相关风险与消息顺序限制仍保留；不承诺原宽带可用。
原创本地官方 plugin 通过 session HTTP hook 传递 max_tokens/max_completion_tokens（二选一，整数 1–32768）与 auto/none/required/指定 function tool_choice；不修改身份、模型、原生工具 schema。参数由带官方 Basic 认证的 loopback 内存控制接口按 session 提供，hook 回执确认后才转发有效增量或成功结果。每 session 只允许一个上游 primary HTTP attempt，拒绝官方自动续写以维持请求输出上限。length 允许推理预算耗尽而没有可见文本。

保留原生工具，追加明确的外部执行上下文；禁止执行未声明内置工具。required/指定函数还校验返回调用，违规报错，不伪造调用。实验中筛掉内置工具会触发 FreeTierError，该方案已撤回。store=false 可接受，store=true 拒绝；assistant 的 reasoning_content 只允许空值占位，不默默丢弃非空推理历史。

真实 OpenCode 2.0.32 编码客户端通过隔离 Loadout 完成读文件、失败测试、修改、成功测试的四轮工具调用。客户端采用显式 profile 本地生成标题，避免标题请求与主任务争用单并发；这不等同任意客户端零配置兼容。部署须经过隔离验收 gate。

2026-10-18 生产复验收到 FreeTierError，后续新建隔离实例与回滚后的 028 基线也失败。已恢复生产 028 脚本并核对 hash；029 代码仅保留为候选。拒绝原因由上游报告为免费层客户端资格，不据此猜测配额、封禁、版本变更或修复回归。恢复可用性前不得宣称生产接入完成。

## Alternatives

2026-10-18 同类隔离研究授权扩展至用户指定的 DESN131/opencode-zen-proxy；仍仅临时原生第三方对照，无产品移植/部署，失败停止能力扩展。

2026-10-18 用户显式要求隔离运行 opencode2api 与官方客户端对照：允许临时第三方副本的原生请求路径做有界研究实验（含其自身身份/工具形状），不移植或部署。单网络出口、精确模型、无账号/代理轮换，基础调用失败停止扩展；只关闭第三方全模型后台探测，推理实现保持原样。此研究例外不改变 Loadout 产品采用官方常驻服务的决策。

9router 完整部署与身份伪装直连不采用。官方无会话 generate 接口实测仍被拒绝。官方 v1 不可用。

## Consequences
增加两个本地常驻进程与约 200 MB 官方二进制。免费可用性随上游变化；不代表官方允许通用代理，也不保证长期免费或模型内部身份。DeepSeek function 工具往返已验证；完整 Chat 语义与真实本地工具执行仍不在能力范围内。没有 Router/Gateway/数据库 schema 变化。

面板通过 `ProviderKind.requires_transport_opt_in`（IPC 向后兼容 bool，默认 false）读取描述符中的 trust 判定，按能力显示开关，不能只根据 OAuth/browser 认证类型决定是否提供启用按钮。无 Provider slug 分支。
