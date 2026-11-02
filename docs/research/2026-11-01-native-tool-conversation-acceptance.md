# 062 — 修正原生工具输入后的有限验收

**结论：WorkBuddy在本轮限定范围内完成工具往返、约21KB上下文和重启后续聊；DeepSeek工具调用通过，结果内容仍未确认。** 不扩大为全账号、全协议或长期稳定性结论。没有运行时补丁或候选副本。

## 本次只改验收输入

依据[DeepSeek官方Thinking Mode说明](https://api-docs.deepseek.com/guides/thinking_mode/)，工具对话需回传已有assistant消息及其reasoning_content。本轮取消强制指定函数，使用原生`tool_choice=auto`，不关闭思考、不改模型；原样回传实际收到的assistant对象，不伪造缺失字段。

初次请求是正常system/user消息，工具结果后直接续接；较长背景作为后续user消息加入，不再预造多轮assistant历史。工具仅按精确参数返回固定内存记录，不执行命令、文件或额外网络操作。真实响应、推理内容及工具参数不落证据文件。

## 实际结果

2026-11-01悉尼时间约01:26–01:31。预算8次，实际6次，均HTTP 200，5次通过内容检查；未重试、未换号。

| 链路 | 工具请求 | 工具结果回传 | 较长上下文 | 重启后续聊 |
|---|---|---|---|---|
| DeepSeek原账号 / deepseek-flash | 通过，工具名与参数准确，1.596秒 | 200，未满足内容断言，1.197秒 | 按失败停止规则未执行 | 未执行 |
| WorkBuddy原账号 / deepseek-v4.1-flash | 通过，工具名与参数准确，2.172秒 | 通过，1.592秒 | 通过，21,582 B，1.610秒 | 通过，21,850 B，2.765秒 |

WorkBuddy真实形成4轮请求，消息数为2→4→6→8；较长上下文及重启后的回复均准确返回原始marker、工具receipt及value。恢复的是客户端重新携带完整history的无状态对话，不是服务端持久会话。全部请求max_tokens=1024、超时100秒；网关活跃观察窗口约253秒。

DeepSeek结果使用严格字符串断言。未保存原始回复，因此不能区分空白/排版差异与实际语义错误，也不能据此认定账号或网关协议失败。第一次工具assistant消息未暴露reasoning_content字段，第二次响应暴露了该字段；测试端未生成虚假内容补齐。下一步仅需改进脱敏的字段级结果判定，不应因此重写适配或调试客户端。

## 调用边界和重启

- 商汤429本轮单列且0次调用，其域名不在本轮出站名单；CommandCode/OpenCodeGo及停用GPT池也未调用。
- 出站仅允许api.deepseek.com、www.workbuddy.ai。两条既有精确别名沿用受限客户端key和原账号，原生Combo临时限制单次尝试，结束后恢复；没有新建账号或临时商汤key。
- 16个账号healthCheckInterval=0保持不变，全局间隔保持60分钟。先在loopback-only下跨过35秒启动窗口；每次调用前后及观察期间核对检查计数。全过程connection-test累计数1417→1417，新增0。
- 同实例正常停止再启动，后台PID/run变化；实例ID、端口56603、账号/模型配置、key权限及vault值保持。只有已通过前三步的WorkBuddy执行重启后真实续聊。

## 原生记录与最终状态

原生usage_history和call_logs各新增6条实际连接记录，全部对应预定DeepSeek或WorkBuddy账号、精确模型；没有额外Combo汇总错误。上游usage合计输入10,851、输出296 tokens，仅为返回统计，不代表账单核对。

原生配置读回与测试前相等，provider vault16项、bootstrap10项逐值核对不变；测试实例文件未发现这些凭据原文，正文标记及request_detail_logs新增均0。实例最终停止，网络恢复loopback-only。

唯一候选仍为058.1，SHA256 `84bb5edd4fc1148c966f73a148d1a10db32b1de02031be4bc70ce2d03959f579`。直接复用060展开目录，没有构建、解包、复制新候选，没有重试旧清理。默认别名、订阅转发、GPT池继续禁用；正式环境、UI、客户端配置、提交上传均未涉及。

## 判断与剩余项

WorkBuddy当前固定账号/模型的标准Chat Completions工具与多轮使用已有真实证据，可以继续有界试用，不需要新增网关功能。DeepSeek需要厘清结果断言；商汤429、CommandCode历史问题和WorkBuddy自然凭据轮换继续单列。此次未测其他客户端/协议、极限上下文或长时间负载，不解除正式切换门禁。

执行入口：`scripts/fusion/bounded-live-acceptance.mjs 062`，要求LOADOUT_LIVE_TESTS=1，已有回执时拒绝重复执行以避免消耗额度。证据：`development/fusion/acceptance/062/result.json`、`usage-audit.json`、`deepseek-response-metadata.json`及`restore-config.json`。语法检查通过，运行时未变，不重跑全量产品测试或benchmark。
