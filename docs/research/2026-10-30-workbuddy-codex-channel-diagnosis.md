# Codex → WorkBuddy 渠道拒绝：原因与最小适配成本

日期：2026-10-30。ExecPlan 048。隔离诊断完成，未安装修复、未修改正式环境。

## 结论

**继续 OmniRoute 窄适配路线值得做。此次阻塞已有可复用的上游处理，而且实测只需要作用于 Codex 系统消息的第一行。** 不需要重写路由、协议转换或凭证管理，也不需要将原生替换规则应用到整个请求。

当前客户端首条系统消息开头包含 `Codex CLI` 和 `OpenAI` 的身份说明。这一行单独放进成功的 Claude 请求结构中，就能触发 WorkBuddy 的 HTTP 400、application code **11128**，错误文本精确匹配 `Illegal API invocation from an unapproved channel`。

反向对照：保留实际 Codex 请求的四条消息、13 个工具、tool_choice、并行工具设置及模型，仅在首条 system 的第一行应用固定 OmniRoute 已有替换规则，请求返回 **200、完整 SSE 和 FUSION_OK**。除该行外的指令和消息没有删减。这足以支持“这份请求的渠道拒绝由系统开头身份说明触发，局部处理可以解除”的结论。

这没有证明 WorkBuddy 内部使用了哪一种检测算法，也没有逐字证明是 `Codex CLI` 还是 `OpenAI` 单独触发。没有必要为本轮成本判断继续穷举词表。

## 实际请求与因果对照

三个已安装客户端各自运行在临时 HOME/workspace，经同一个完整候选后台转换后发到本地 mock。正文只留内存，磁盘保留角色、结构、长度、哈希和预期 marker；仅额外保留用于定位的短客户端内置身份行。正式客户端配置未更改。

| 文本任务请求 | 转换后的消息 | 工具数 | 原始 stream | 体积 |
|---|---|---:|---|---:|
| Claude Code | system + user，字符串 | 0 | true | 437 B |
| OpenCode | system + user，字符串 | 1 | true | 2,945 B |
| Codex | system + system + user + user，text 数组 | 13 | false | 40,891 B |

三者原始任务都是只回答 FUSION_OK。工具数反映该受限测试配置，不代表各客户端日常默认配置。Codex 第一条 system 的 text 数组序列化长度 17,758 B，各轮捕获哈希一致；开头行 186 B（包含行尾 CR），哈希 `4af978e2aec13fc89c058814ff9cf3cb02d9dc8dc72789d5aeb479eb55d7795f`。

Live 重放固定当前绑定账号、`deepseek-v4.1-flash`、本机 WorkBuddy 5.9.4 / CLI 2.165.0 的连接头。沿用既有 SSE 语义，所有请求统一 stream=true、最多 512 输出 token；会话 UUID 每次正常生成。单并发、单请求 60 秒、无自动重试。

| # | 唯一声明的对照变化 | 结果 |
|---:|---|---|
| 1 | Claude 控制请求 | 200，预期答案 |
| 2 | OpenCode 控制请求 | 200，预期答案 |
| 3 | 完整 Codex 基线 | 400 / 11128，渠道拒绝 |
| 4 | Codex 其他字段及 13 个工具 + Claude 消息 | 200，预期答案 |
| 5 | Claude 其他字段 + Codex 全部消息 | 400 / 11128 |
| 6 | 完整 Codex 的 text 数组转字符串，保留文本 | 400 / 11128 |
| 7 | Claude 字符串转 text 数组 | 200，预期答案 |
| 8 | Claude 结构 + Codex 第一条 system | 400 / 11128 |
| 9 | Claude 结构 + Codex 第二条 system | 200，预期答案 |
| 10 | Claude system + Codex 两条 user | 200，预期答案 |
| 11 | Claude 结构 + Codex 第一条 system 的第一行 | 400 / 11128 |
| 12 | 完整 Codex 原始请求再对照 | 400 / 11128 |
| 13 | 完整 Codex，仅第一条 system 的第一行应用已有规则 | **200，预期答案** |
| 14 | 完整 Codex，仅 system content 应用已有规则 | 200，预期答案 |
| 15 | 完整 Codex，全 body 字符串应用已有规则 | 200，预期答案 |

前 12 次用于定位；发现原生已有处理后，明确追加最多 3 次，比较必要作用范围。没有扩大适配词表，没有模型或账号切换。所有 200 都检测到 `[DONE]`，返回模型匹配、答案包含预期 marker。

由此排除了这份请求的 developer role、text 数组表示、工具数量/定义或工具相关顶层字段是此次拒绝的必要原因。不是“看到 11128 就推断账号有问题”。前轮 STREAM_REQUIRED 是另一个已经处理的流式要求。

## 指定项目和上游已有处理

| 来源与当前核对范围 | 找到的处理 | 对本次请求的意义 |
|---|---|---|
| AidenOrz/dsh-workbuddy-connect 0.7.15，`25680ca7f7ff5d7b84bb35f45de766519fccbc9d` | 强制 SSE、developer→system、tool_choice；DSH 身份/产品名/句子规则及外部覆盖表 | 实际导入原函数，对已做 SSE 归一化的 Codex 请求 **changedKeys=[]**；没有现成 Codex 匹配规则 |
| 历史原仓库 corrinehu/dsh-workbuddy-connect | 当前 API 返回 404 | 不将历史文档中的原仓库地址视作当前可追踪上游 |
| GitHub 当前标记的 fork parent：jojoman2024/dsh-workbuddy-connect，`cc97452665eb87e7798581bdf9c80a52e54bae5b` | 角色、SSE、tool_choice、reasoning 处理 | 核对的 prepareChatBody 路径没有 Codex 身份处理；两个可访问 fork 的 issues 列表为空，不能据此证明全网无修复 |
| OmniRoute 固定 v3.8.52，`ad6f33f66bda34eba98645d3bfdabbde294bc843` | **原生 workbuddy registry 的 bodyStringReplacements 已含 Codex CLI / OpenAI / Codex 等规则** | 本次直接复用其函数和规则完成 #13–15，未创造新指纹规则 |
| OmniRoute 当前 main `55ccad495c4a73eb64deaf094829fe5b90ded80c` | WorkBuddy registry 中上述规则仍存在且一致 | 此问题无需先追到新版底座才能找到处理 |

为什么当前实验没生效：实验用的是自定义 openai-compatible Provider 连接只读凭证 relay；`DefaultExecutor` 的 `applyRegistryBodyRepairs(this.provider, ...)` 按 **registry provider ID** 取规则。自定义节点即使用户可见名称叫 WorkBuddy，也不等于内建 `workbuddy`。这保留了窄连接边界，但同时没有继承内建的消息替换。

原生 helper `replaceBodyStrings` 会递归处理**全部字符串值**。离线合成反例确认，它会把用户文字中的 OpenAI、`C:/Codex/project` 路径和工具 arguments 字符串一起改写。所以不能为了零补丁就直接开启整套全 body 替换；需要约束作用范围。

另有 [PR #18212](https://github.com/cheaper-inference/OmniRoute/pull/18212)，已合并到 release/v3.8.53，解决 forceStream Provider 仍发出 stream:false 等问题。它属于流式要求修复，不是本轮身份句拒绝修复；本轮没有更新底座。

主要源码依据：

- [指定项目的准备函数](https://github.com/AidenOrz/dsh-workbuddy-connect/blob/25680ca7f7ff5d7b84bb35f45de766519fccbc9d/src/upstream.ts)与[内置规则表](https://github.com/AidenOrz/dsh-workbuddy-connect/blob/25680ca7f7ff5d7b84bb35f45de766519fccbc9d/src/channel-fingerprints.ts)。
- [OmniRoute 固定版本 WorkBuddy registry](https://github.com/cheaper-inference/OmniRoute/blob/ad6f33f66bda34eba98645d3bfdabbde294bc843/open-sse/config/providers/registry/workbuddy/index.ts)与[原生字符串处理 helper](https://github.com/cheaper-inference/OmniRoute/blob/ad6f33f66bda34eba98645d3bfdabbde294bc843/open-sse/utils/strictChatHistory.ts)。
- [原生兼容改动 PR #17046](https://github.com/cheaper-inference/OmniRoute/pull/17046)已于 2026-10-18 合并至 release/v3.8.52；不是待合并的修复建议。

## 最小适配方案与成本

建议下一步只在 **WorkBuddy 连接器内**增加显式、可关闭的兼容函数：识别已验证的客户端内置 system 开头，仅在该行复用已验证的上游规则，其他 body 内容保持逐字不变。匹配不到不做猜测性改写，仍返回清楚的上游拒绝。

| 方案 | 代码/升级面 | 判断 |
|---|---|---|
| 指定 DSH 项目直接整包接入 | 带入 DSH host、刷新存储和新的规则维护；当前规则还不命中 Codex | 不合适 |
| 直接使用内建 WorkBuddy 全 body 替换 | 规则已存在，但会触及用户正文、路径和工具数据；还需另验内建 OAuth 存储边界 | 不直接采用 |
| **连接器中的首行局部兼容** | 预计几十行边界处理、一个 Provider 内调用点、零 Router/SQLite/凭证格式改动；上游后端新增 hook 可为零 | **建议继续验证** |
| 重写路由/协议或回到全量自研 | 不能解决上游内容渠道检查，维护量明显增大 | 没有证据支持 |

“几十行”是工程估算，不是已经完成的发布补丁行数。主要维护工作是客户端升级时检查开头句、上游规则变化及真实回归，而不是再维护一套协议转换。下一阶段的有限门槛应为：首行范围外逐字不变、非目标 system/用户/工具结果不改、多模态不丢、真实 Codex 文本与只读工具闭环、多轮重放。匹配失效就保留明确拒绝，不自动扩张为全文替换词典。

## 证据范围与清理

本轮的成功是 **真实客户端生成请求的上游重放成功**。并未把转换装进 relay，也没有重新宣称真实 Codex 工具/长会话闭环已通过。047 的 Claude/OpenCode 工具结果与本轮文本对照是不同验收项。

- WorkBuddy 推理共 15 次；商汤、CommandCode 为 0。
- 新增后台 hook、候选包改动为 0；完整候选 21,732 文件校验通过，树哈希仍为 `0aeff8ee6652210a8e3f9cf1be9c75d3bac8d991736782c2df6c0c243df885db`。
- 官方 auth/binding、检查范围内正式客户端配置的前后哈希一致；没有 refresh、正式登录写入、生产部署或 UI 改动。
- 本轮 617 个实验/运行文件扫描当前 WorkBuddy access token 的 UTF-8/UTF-16 明文，命中 0。此结论限定于扫描对象和当前 token，不是全机通用秘密审计。
- 47 个历史隔离实例清理核对：剩余测试凭据 0，仍存活的实验后端 PID 0。

可复核回执：12 次定位、3 次已有规则作用范围对照、上游核对、安全及包审计、清理。复现入口见 phase9 README。
