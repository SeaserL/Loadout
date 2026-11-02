# 059 真实绑定、精确映射与隔离恢复

本轮指定范围已完成：15个真实API key迁入隔离OS vault，WorkBuddy按原绑定账号只读接入，8条配置路由下发并通过真实状态的加密恢复核对。没有修改058候选源码或重打包；唯一候选仍为`fusion-v3.8.52-isolated-058.1-39f5ab1fafa9`。

当前隔离实例**已停止、出站仍为loopback-only**。这是凭据和配置迁移验收，不是上游可用性或日常推理验收。未发起任何真实上游推理，未改客户端、正式数据库、正式凭据或安装；无UI、提交上传。

## 用户决定与待决配置

| 项目 | 用户决定及落地结果 | 以后若需要启用 |
|---|---|---|
| `loadout/default` | 保持禁用；不下发别名、不代选模型，从迁移后的受限客户端白名单中移除它。 | 明确账号、模型及有序成员；再按既有map应用，不自动找“能用的模型”。 |
| Codex订阅转发 | 旧库`transport_opt_in_at`为空，用户选择继续停用；未读取或复制其access/refresh token，未发起登录。 | 独立受控登录、核对同一账号后由用户确认订阅转发。 |
| `loadout/gpt` | 用户选择整池暂不启用；完整3成员及顺序保留在迁移记录，未向原生后台下发缩水池。 | 上述订阅成员满足条件后，再整体启用原池。 |
| `loadout/fast` / `loadout/realtime` | 仍未映射；未被本次客户端策略引用，不代填。 | 只有实际需要时再明确目标，不为本轮验收新增动态选路。 |

本轮没有需要用户继续决定才能完成上述已选范围的配置。旧只读Codex/Claude Code来源，以及OpenCode Free桥接/ChatGPT网页会话仍在完整来源清单中，没有被本轮转成新的推理连接或静默删除。`fullMigrationReady=false`明确保留，不宣称21个来源全部迁完。

## 实际绑定和路由

| 绑定 | 数量 | 处理 |
|---|---:|---|
| 商汤 | 12 | 旧API key只读复制到隔离vault；12账号顺序及全部5个模型保留。每账号20秒间隔、并发1、3RPM，沿用既有原生有界等待配置。 |
| CommandCode / DeepSeek / OpenCode Go | 各1 | 复制旧API key并核对存储；OpenCode Go继续Responses接入，其他使用原Chat profile。没有探测401或调用模型。 |
| WorkBuddy | 1 | 账号digest与旧桥接绑定一致；已验证官方加密会话的access token可读。仅生成隔离桥接key，不复制官方refresh token，不使用正式11128端口。 |

旧库这4类API Provider的`base_url`字段均为空，原应用使用Provider默认值。本轮从既有Rust descriptor解析默认值，记录源码位置和SHA；没有另选服务端点或维护第二套运行时地址表。初次编排在处理空地址时停止，尚未写Provider凭据；修正外围地址解析后复用同一空隔离实例完成，失败回执保留，没有扩大运行时补丁。

已下发的8条路由：

| 外部别名或原生Combo | 实际模型 | 账号范围 |
|---|---|---|
| `loadout/workbuddy-flash` | `deepseek-v4.1-flash` | 原WorkBuddy账号1个 |
| `deepseek/main/deepseek-flash` | `deepseek-flash` | 原DeepSeek `main`账号1个 |
| `workbuddy/main/deepseek-v4.1-flash` | `deepseek-v4.1-flash` | 原WorkBuddy `main`账号1个 |
| `endpoint-sensenova-1` | `deepseek-flash` | 原商汤12账号 |
| `endpoint-sensenova-2` | `deepseek-v4-flash` | 同上 |
| `endpoint-sensenova-3` | `glm-5.2` | 同上 |
| `endpoint-sensenova-4` | `kimi-k3` | 同上 |
| `endpoint-sensenova-5` | `sensenova-6.8-flash-lite` | 同上 |

商汤入口沿用之前已验的原生Combo迁移方案：未来接入新后台使用`/v1`和上述Combo名，转发模型仍是原模型；本轮没有修改正式客户端的旧`/e/sensenova/v1`配置。这里不增加兼容URL或别名功能。

旧受限客户端策略迁为`legacy-client-2`，仅允许两个原精确别名和对应两个连接，保留8RPM；仅按用户决定移除默认别名。各路由专用key也固定对应成员。旧无约束客户端未自动重发为全权限key。CommandCode/OpenCode Go凭据虽已绑定，但未通过缩水GPT池开放使用。

## 恢复与安全证据

7组执行检查全部通过，直接使用未改动的058包内工具：

- 源元数据、精确账号alias/模型和候选身份核对；15个API key逐一与目标vault值在内存比较。
- WorkBuddy官方会话解密可读、账号与旧绑定一致；没有真实聊天请求。
- 原生连接、12账号节流、8个Combo、3条精确映射及客户端范围/RPM读回一致。
- 停止原实例后，用既有AES-GCM+scrypt备份恢复到新隔离目录；Provider凭据、启动/客户端key、原生配置及完整迁移记录相等，恢复实例可以启动。
- 使用恢复后的受限key请求`loadout/default`和`loadout/gpt`均返回403；未授权模型返回400。没有允许的真实模型请求。
- 源元数据投影、15个源API key、WorkBuddy认证/绑定文件前后相同；原实例/恢复实例文件与回执扫描未出现真实秘密明文。
- 恢复测试实例已停止并清空凭据；当前隔离实例保留16条Provider秘密和10条启动/客户端秘密，均属于独立测试命名空间，状态为停止。它们是有意保留的迁移状态，不是清理遗漏。

证据：执行结果、绑定表、映射与停用清单、最终审计、清理回执。这些文件不含秘密值。

## 保留状态与下一步

当前迁移状态索引指向停止的隔离实例：`development/fusion/isolated/loadout-fusion-isolated/059-current`，本地文件约2.62MB，已加入忽略规则。运行该状态需要临时解包匹配的058候选，用既有`fusion.cmd`操作；不要用另一个发行身份打开它。

本轮临时解包、加密测试备份和恢复目录已删除（约1.19GB逻辑大小），仅一个候选ZIP。临时随机备份口令已丢弃；本轮恢复验证不是给用户交付一份长期灾备副本。058首次失败目录的自动审批清理阻塞仍由原计划跟踪，本轮未重试删除它。

外部事项保持独立：商汤风控429、CommandCode401、WorkBuddy自然凭证轮换仍为DEFERRED；存储/恢复成功不证明这些问题解除。下一步若继续，只需在明确出站白名单和调用上限下验收实际使用链路及失败恢复；不先新增功能、UI或客户端调试。正式环境切换尚未授权。
