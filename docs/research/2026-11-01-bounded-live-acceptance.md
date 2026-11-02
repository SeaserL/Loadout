# 060 — 当前候选有限真实使用验收

结论：058.1候选在059当前隔离实例上的三条既有短文本链路均成功，未发现需要新增适配的阻塞。适合继续限定范围试用；本轮不等于全面日常稳定性验收，也不构成正式切换。

## 实际结果

执行时间：2026-10-31 14:01–14:02 UTC，澳洲悉尼为2026-11-01 01:01–01:02。

| 原有入口 | 实际账号范围 | 请求/返回模型 | 结果 | 端到端耗时 |
|---|---|---|---|---|
| 商汤旧端点对应原生Combo `endpoint-sensenova-2` | 原号池account-1，仅此一个 | `deepseek-v4-flash` | 200，文本为OK | 3.853秒 |
| `deepseek/main/deepseek-flash` | 原DeepSeek账号 | `deepseek-flash` | 200，文本为OK | 0.863秒 |
| `workbuddy/main/deepseek-v4.1-flash` | 原WorkBuddy绑定 | `deepseek-v4.1-flash` | 200，文本为OK | 1.373秒 |

全程串行，网关请求3次，原生usage_history和call_logs各新增3条成功记录；实际连接均与上表一致。上游返回的usage合计输入151、输出16 tokens（不是账单核对）。每次max_tokens=256，客户端超时100秒。没有追加工具调用、长会话、流式或客户端进程测试。

## 范围与节流

- 沿用059-current、后台56603/WorkBuddy隔离桥56602；默认别名、订阅转发、完整GPT池保持禁用，未创建替代模型映射。
- 商汤12个账号原配置保持20秒最小间隔、并发1、3 RPM。本轮商汤总共1次，只用原有account-1，没有试遍号池或清除冷却。
- 为限制真实尝试预算，三个既有Combo临时设maxGlobalAttempts=1/maxRetries=0/maxSetRetries=0，保留failoverBeforeRetry=true（已有执行器以此跳过内部URL重试）；临时关闭冷却后的自动重试。商汤临时key仅允许既有Combo及account-1，其他两条用原受限客户端key并严格固定原账号。结束后均恢复。
- 仅临时允许三个已配置目标域名出站；没有模型发现、登录、配额刷新等额外外部请求。没有修改运行时源码或候选包。

## 收尾核对

1. Combo、模型映射、key权限、账号启用/节流配置及resilience读回与测试前相等；临时key已删除。
2. provider vault的16项与bootstrap的10项在内存中逐值核对未变；测试实例文件未发现这些凭据原文。新增call_logs的has_request_body/has_response_body均为0，新增request_detail_logs为0。
3. 隔离实例已停止，配置恢复loopback-only，本轮包进程为0。没有写正式数据、已安装桌面端、正式凭据或客户端配置，没有提交上传。
4. dist/fusion仍仅一个058.1 ZIP，SHA256仍为`84bb5edd4fc1148c966f73a148d1a10db32b1de02031be4bc70ce2d03959f579`。
5. **清理未完成**：本轮展开目录`development/fusion/runs/060-live-acceptance`的删除被自动审批以`blocked by policy`拒绝，未提供具体理由，未绕过重试。目录文件累计1,182,280,032字节，约1.10 GiB；不是新增发布版本。058遗留的同类清理阻塞未触碰。060计划因此仍active，只跟踪清理。

## 外部问题与证据边界

- 商汤本次1次成功且无429；不能据此认定历史风控已消失，也未在真实上游主动制造429。429后的节流行为沿用既有mock回归证据。
- CommandCode历史401未重新验证：它属于用户已停用的GPT池，不绕过授权范围单独调用；OpenCodeGo同理不启用。
- WorkBuddy当前凭据真实请求成功；自然轮换、长时间运行仍未验证。本次没有扩展首行替换或调试Codex/Claude Code/OpenCode。
- 单次请求验证了所选账号、模型与短文本返回，不代表全部商汤账号、其他模型、协议、工具调用或长会话已验收。未运行真实failover压力测试。

## 下一步

冻结新增功能和补丁。先清理本轮临时展开目录；随后如继续试用，使用同一候选与同一授权范围做有总量上限的短期观察，补实际使用所需的会话与停止/重启恢复记录。外部账号报错届时单列；不开GPT池补齐表面覆盖率。长期维护判断仍需既定完整适配的上游跨版本演练，不用这3次成功代替。

复现入口：`scripts/fusion/bounded-live-acceptance.mjs`，要求`LOADOUT_LIVE_TESTS=1`且已展开当前包；存在本轮result.json时拒绝重复运行，防止重复消耗额度。脱敏证据：`development/fusion/acceptance/060/{result,usage-audit,restore-config,cleanup}.json`。不需要重建或另存候选包。
