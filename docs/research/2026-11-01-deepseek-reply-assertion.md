# 063 — DeepSeek回复断言诊断

**本轮未复现062的差异：两次真实请求均通过原严格判据。062原回复未保存，仍无法追溯判定那一次是格式差异还是实际字段错误，不能用本次成功改写旧结论。** 没有放宽验收门槛，也没有修改网关。

## 判据分离

保留原判据：回复trim后必须完整等于`AMBER-062-DEEPSEEK|LOCAL-062-OK|37`，无额外工具调用且模型匹配。新增的纯测试诊断器不参与放宽pass，只输出字段匹配布尔值、表示形式、长度和分类：

| 观察 | 诊断 | 原严格验收 |
|---|---|---|
| 原trim后整串相等 | EXACT | 通过 |
| 只存在分隔符边缘ASCII空白或整段单层代码格式，3字段逐字同序正确 | FORMAT_ONLY | 仍失败，报告格式不符 |
| 在支持的表示处理后，字段字符串与预期不一致 | CONTENT_MISMATCH | 失败，不用包含关键词、字段重排、大小写转换或数值转换补齐 |
| 字段数量不对、没有文本等 | UNRECOGNIZED_STRUCTURE / MISSING_TEXT | 失败，不猜测 |

CONTENT_MISMATCH表示限定解析下的字面字段不符，不自动推广为模型事实错误；未识别的表示方式不能靠模糊匹配认定语义正确。原文/推理内容仍不进入证据文件。

22项离线测试通过，覆盖纯格式变体、错误marker/receipt/value、顺序、大小写、缺项、多项、数字转换、额外说明、残缺/嵌套包装、字段内部空白和Unicode近似字符；格式诊断不会把strictPass从false升成true。

## 一组真实工具往返

2026-11-01悉尼时间01:39，沿用原DeepSeek账号和`deepseek-flash`。初始提示、工具schema、marker/record_id和固定结果保持062一致，不调整提示词去追求通过；本轮接收的实际assistant消息完整回传，保留其reasoning_content。工具只返回内存固定值。

| 请求 | 结果 |
|---|---|
| 工具请求 | 200，1.891秒；工具名和3个参数逐值正确，只有1个工具调用 |
| 工具结果回传 | 200，1.558秒；EXACT、strictPass=true，marker/receipt/value全部正确，3字段同序、34字符、plain表示，无额外字段边缘空白或工具调用 |

真实请求总数2，原生usage_history/call_logs各2条，均为原DeepSeek账号和精确模型。未重试、未换号，也未为了复现失败继续请求。商汤、WorkBuddy、CommandCode、GPT池均0次调用。此次工具往返通过，不代替尚未完成的DeepSeek长会话和重启续聊验收。

## 安全与收尾

仅api.deepseek.com允许出站；自动探测计数1417→1417，新增0。16个账号healthCheckInterval=0保持，默认别名/订阅转发/GPT池仍禁用。原生配置读回与测试前相等，vault16项及bootstrap10项逐值核对不变；正文日志和detailRows新增0，实例文件未发现这些凭据原文。

实例已停止且恢复loopback-only，包进程0。唯一058.1候选SHA256保持`84bb5edd4fc1148c966f73a148d1a10db32b1de02031be4bc70ce2d03959f579`；继续使用060展开目录，无候选副本、产品功能、UI、客户端调试、正式环境变更或提交上传。

## 后续判断

现在可以明确区分今后观察到的格式不符与字段不符，并且保留严格验收状态。本次DeepSeek工具往返没有暴露需要改网关的证据；062那次只保留UNKNOWN，不归因为账号失效或协议故障。无需为获得某种结论追加重试。

测试入口：`scripts/fusion/bounded-live-acceptance.mjs 063`，仅显式LOADOUT_LIVE_TESTS=1且无本轮回执时可执行。纯诊断器：`scripts/fusion/reply-diagnosis.mjs`；离线反例：`tests/fusion/reply-diagnosis-test.mjs`。本轮证据：`development/fusion/acceptance/063/{result,usage-audit,restore-config}.json`。语法检查和22项离线测试通过，无Router/Gateway变更，不跑额外benchmark。
