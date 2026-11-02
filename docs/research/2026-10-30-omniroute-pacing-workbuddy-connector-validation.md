# 046：429节流与WorkBuddy指定项目连接验证

日期：2026-10-30，Australia/Sydney。范围：隔离后台、mock商汤、有限WorkBuddy合成请求；没有UI开发、正式切换或CommandCode调用。

## 结论

**继续OmniRoute最小适配路线。商汤429后的节流失效已有完整包验证通过的窄补丁；用户指定的dsh-workbuddy-connect可以提供WorkBuddy所需的连接能力，本机隔离文本链路已恢复。** 不需要重写路由，也不需要把整个DSH插件及其凭证存储搬进来。

这不是正式环境修复完成的声明：冻结包、正式IPC桥接和安装均保留。本轮候选独立保存，正式迁移仍未授权。真实商汤风控是否解除、WorkBuddy工具/长会话/凭证轮换仍未验证。

## 固定来源与现成修复核查

- OmniRoute固定v3.8.52：`ad6f33f66bda34eba98645d3bfdabbde294bc843`。
- 检查上游main：`55ccad495c4a73eb64deaf094829fe5b90ded80c`。其中`open-sse/services/rateLimitManager.ts`与固定版字节一致，仍在429后evict Bottleneck。[PR11413](https://github.com/cheaper-inference/OmniRoute/pull/11413)关闭、未合并，不能当作已经发布的修复；它也不是完整的操作者minTime历史保留方案。
- 用户指定[AidenOrz/dsh-workbuddy-connect](https://github.com/AidenOrz/dsh-workbuddy-connect/tree/25680ca7f7ff5d7b84bb35f45de766519fccbc9d)：固定`25680ca7f7ff5d7b84bb35f45de766519fccbc9d`，package版本0.7.15，MIT / Copyright (c) 2026 Corrine Hu。参考checkout保持干净；具体文件许可、notice与来源锁已记录。
- 证据：`development/experiments/omniroute-phase7/evidence/upstream-research.json`、`audit.json`、`development/upstreams.lock.json`。

## 商汤：一个限流接缝，不新增路由

原失败反例来自045：每账号minTime=20000、maxConcurrent=1、rpm=3，收到429/Retry-After=2后同账号约2127ms就再次发出。普通成功请求之间的20秒不能证明失败重试也受限。

先检查原生配置替代：`providerQuotaOverrides`的rpm映射到固定60000ms窗口，不能表达1次/20000ms的实际dispatch间隔。rpm=3允许突发；rpm=1改变为每分钟一次，不能作为等价修复。

候选只改`open-sse/services/rateLimitManager.ts`，**+25/-1**：复用现有`SlidingWindowLimiter`，在实际调用executor前记录dispatch floor，使其不随429的Bottleneck替换而丢失。账号选择、模型选择、原生429 eviction、长Retry-After、SQLite和凭证生命周期保持原有实现。等待沿用当前queue预算和取消信号，不新增Provider专用路由分支。

| 检查 | 本轮结果 |
|---|---:|
| 未修复源代码的新回归用例 | 5项中3失败，反例保留 |
| 修复后相关上游单测 | 56/56通过 |
| 完整候选后台构建 | 退出码0 |
| 完整包隔离检查，含恢复与清理 | 23/23通过 |
| 普通连续请求间隔 | 20010ms |
| Chat 429短hint后同账号间隔 | 20002ms |
| Responses 429短hint后间隔 | 20012ms |
| Messages 429短hint后间隔 | 20013ms |
| 上游25秒hint | 实际25126ms，未缩短为20秒 |
| 另一授权账号立即接手 | 133ms完成；同模型 |
| 存在健康但被排除的账号 | 只重试获授权账号，间隔20006ms |

同时保留原生最多2次wait、累计sleep预算、60秒hint超预算不等待、401不重试、无正hint不强制循环、取消后不再发出、计划篡改拒绝和恢复后配置/范围检查。没有把累计sleep预算说成总请求30秒deadline。

普通请求12样本：候选median77.26ms，冻结包median78.04ms；候选p95=90.60ms，冻结包p95=377.82ms。冻结包过滤运行包含冷启动，候选来自已热身完整suite，**p95不能作同条件性能提升结论**。这轮未发现median明显新增成本，但不是生产负载benchmark。

候选完整目录含21732个文件，初始manifest摘要`0aeff8ee6652210a8e3f9cf1be9c75d3bac8d991736782c2df6c0c243df885db`；测试前后已列文件hash验证通过。包为`development/experiments/omniroute-phase4/runs/package-phase7-1793356148280`，仅由phase7候选锁显式选用。原冻结锁没有被替换。

## WorkBuddy：指定项目确实有用，但只取连接部分

本机安装源码表明旧`wb.request/http.fetch`只允许调用方传content-type和accept，内部没有注入X-Conversation三项。这条IPC入口无法直接采用参考项目的会话头修复。

指定项目的[渠道检查说明](https://github.com/AidenOrz/dsh-workbuddy-connect/blob/25680ca7f7ff5d7b84bb35f45de766519fccbc9d/docs/channel-check-fix.md)及`src/upstream.ts`区分多种11128：会话标识缺失、首条system要求、渠道提示词检查。其完整插件还会写自己的refresh凭证副本、自动刷新，并改写部分提示词及路径。本轮没有启用这些行为。

采用的隔离连接为：

```text
合成Chat请求 → 冻结OmniRoute后台 → 独立loopback bridge
             → 指定项目header/UA函数 + 内存access token
             → https://www.workbuddy.ai/v2/chat/completions
```

- 复用参考源码的header、UA、auth parser、内存key helper；不安装DSH。构建时临时导出chatHeaders，参考checkout不改。
- 只读取现有绑定账号的加密桌面凭证，用本机官方Electron helper取得解密所需信息，**只解密access token**。refresh token不解密、不调用刷新接口、不写副本。
- 读取本机WorkBuddy **5.9.4** / CLI **2.165.0**，三个会话标识每次生成；只访问固定官方域名，禁止重定向。
- 保留现有bridge的账号绑定、固定deepseek-v4.1-flash、文本限制、模型一致性和完整SSE检查。输入原文保持，没有指纹改写。
- 7/7离线检查通过，覆盖会话头、账号头、无refresh token、消息不变、模型限制、错误脱敏、无隐式重试、内存解密及live opt-in。

真实验证：**HTTP200、预期LOADOUT_OK、1950ms**。来自完整冻结后台经测试bridge的调用，不是单独curl成功冒充集成成功。测试前后桌面auth及正式bridge binding文件hash一致，测试vault清空。

本轮旧IPC对照未完成，不能将此前11128唯一归因于缺会话头。新路径同时改变传输、版本头和会话头，证明的是该连接组合在本机有效。首次本地准备曾因诊断脚本仅允许workbuddy.ai、漏掉实际www.workbuddy.ai而停止，零外部调用；修正为两个明确官方域名后成功，未刷新或替换账号凭证。

证据：`evidence/workbuddy-static.json`、`workbuddy-tests.json`、`workbuddy-live.json`；完整live结果在`development/experiments/omniroute-phase4/runs/workbuddy-connector-1793355968892/result.json`。失败准备结果同样保留在`workbuddy-connector-1793355949991`。

## 维护增量与实际限制

1. **OmniRoute新增1个运行时接缝**，原安全adapter内容未变；原14个上游接缝变为15个（构建tsconfig另计）。新patch静态试套当前main通过；没有新的release可做实际跨版本运行，本轮不能冒充一次新的升级验收。
2. WorkBuddy隔离adapter格式化后181行，构建选择模块78行，复用上游实现。`chatHeaders`不是公开稳定导出，必须保留固定SHA和构建/契约测试；它对本机Electron私有binding的依赖也可能随桌面升级变化。不能称作零维护。
3. dispatch floor为单进程内存状态，重启不恢复历史；沿用上游5000-key上限，12账号profile内通过不等于无限规模或多实例承诺。当前候选不要扩展为分布式限流。
4. WorkBuddy只验证合成文本。工具请求、实际长会话、客户端system提示词、桌面token轮换、账号切换与进程取消传播未完成验收。当前脚本固定60秒响应超时、1MiB响应上限和一次新连接调用；既有bridge仍拒绝工具/有状态请求。
5. 凭证没有新增普通磁盘副本；Node字符串不提供Rust式安全清零保证，沿用此前已明确的候选差异。
6. 商汤本轮零真实请求，修复本地节流并不保证上游风控永不触发。CommandCode零调用，仍由用户处理。

**建议：保留OmniRoute主体，将本轮限流修复及WorkBuddy连接作为小范围候选继续验收；不回退全量自研，也不整包搬入DSH插件。** 正式切换前仍须验证真实商汤保守调用、WorkBuddy实际客户端所需能力、凭证轮换和完整迁移范围。

## 隔离与收尾

全部实验实例核查：38个累计实例，剩余实验凭据0，owned进程0。没有安装插件、改桌面端、启动正式实例或推送提交。

只读审计发现正式clientPolicies比044快照多一项；本轮未写正式库，变化来源未归因，未覆盖回旧值。第一次046审计之后的路由metadata仍一致。不能用旧历史快照宣称整台机器完全未变。

生产依赖许可检查：348个包、0策略违规、7个无bundled license文件；本轮仅实验源码复用notice，不据此宣称新组合已完成发布许可审计。

可重放步骤与边界见phase7 README。核心回执在该目录evidence；原045失败反例继续保留。
