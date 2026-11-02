# ExecPlan 048：Codex 到 WorkBuddy 的渠道拒绝定位

## Goal
对比三个真实客户端经固定 OmniRoute 后端形成的请求，定位 Codex 渠道拒绝，核对指定连接器及其上游现成处理，评估最小适配成本。
## Context
047 中 Claude Code/OpenCode 文本和只读工具通过；Codex 强制上游 SSE 后仍被渠道拒绝。用户要求先明确原因与成本，不为通过测试扩大补丁。
## Existing behavior
复用 phase4 Instance/client、phase7 固定候选包和 WorkBuddy header、phase8 只读凭证 reader。原生转换已将 developer 转成 system；现有实验 relay 不改提示词。
## Constraints
只在隔离目录运行；不改正式环境、UI、账号、模型、客户端配置或登录。默认只 mock，live 显式 opt-in；最多 12 次固定账号/模型请求，一并发、每次 60 秒、最多 512 输出 token。401/403/429 或未知错误关闭出口；仅对已识别渠道 400 做预先限定的差分对照，无自动重试。CommandCode/商汤零调用。
## Architecture impact
仅增加实验诊断脚本。不修改候选包、路由和 relay，不加入提示词替换规则。内容分组替换只用于合成请求的因果对照，不视作可交付适配。
## Files expected to change
development/experiments/omniroute-phase9/，本计划、调研报告、reference-map 的结论入口。
## Implementation stages
1. 隔离 mock 捕获三种真实客户端的转换后请求，仅内存保留正文。
2. 核对指定项目/原上游当前源码、历史与公开问题。
3. 有限 live 对照，区分字段、工具、消息组与渠道内容检查。
4. 脱敏证据、清理验证、最小成本和继续路线结论。
## Tests
真实客户端对 mock 的单请求；固定模型/预算/鉴权停止逻辑；对照请求只改变声明的维度。记录状态、application code、消息长度/角色/字段类型与哈希，不保存正文/凭据。
## Performance validation
记录隔离调用数和耗时。没有 Router/Gateway 代码变更，不重复后台性能 benchmark；不把缓冲返回时间冒充 TTFT。
## Security considerations
凭据只读并仅内存解密 access token；不 refresh。不记录请求/响应全文；只保存分类、计数与哈希。客户端独立 HOME；检查正式 auth/binding 与客户端配置未改变，清理隔离 vault 和进程。
## Migration considerations
不替换冻结锁或候选包，不迁移正式数据。
## Known risks
上游渠道检查不透明；有限差分能定位触发范围，不能证明其内部实现或长期稳定。删除指令能通过不意味着删除可接受。
## Definition of done
形成可复核的请求差异、当前上游处理与明确成本结论；没有扩大运行时补丁；清理完成。
## Progress
- [x] Boot sequence、047 回执和现有实验抽象复核。
- [x] 三客户端 mock 捕获及结构对照；指定连接器 body preparation 对实际 Codex 请求无改动。
- [x] 12 次 live 将触发范围定位到首条 system 的第一行；数组/工具/第二条 system/用户上下文均有成功对照。当前 OmniRoute 原生 WorkBuddy 已带 Codex/OpenAI 替换规则，需核对其全请求作用范围。
- [x] 追加 3 次验证既有规则：首行、system、全 body 均 200；最小首行范围已足够，不引入 relay 补丁。
- [x] 617 文件 token 明文扫描 0 命中，21,732 文件候选校验通过；47 个历史实例剩余凭据/后端 PID 为 0。报告、来源表和 notice 已更新。
## Decisions
- 不把已被原生处理的 developer role 当成新补丁理由。
- 不将诊断性内容删减或上游身份替换当作可接受生产修复。
- 12 次定位完成后，发现固定底座已有 Codex 规则且作用于整个 body。为回答“最小适配成本”，追加独立最多 3 次对照（总上限 15）：仅首行、仅 system、原生全 body 替换，其他限额不变。只验证已有规则的必要作用范围，不写入实际 relay，不扩大规则词表，不继续二分穷举。

## Outcome

建议继续 OmniRoute 窄适配：预计几十行 Provider 内首行兼容即可承接当前拒绝，零 Router/SQLite/凭证变更，不直接启用全 body 替换。此轮只完成原因、已有处理及作用范围诊断，真实 Codex 完整工具闭环仍是后续验收项。详见 [048 报告](../../research/2026-10-30-workbuddy-codex-channel-diagnosis.md)。
