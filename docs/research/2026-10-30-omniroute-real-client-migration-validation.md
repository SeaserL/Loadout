# OmniRoute真实客户端与迁移验证（043，2026-10-30）

**结论：继续采用OmniRoute原生SQLite和最小适配路线有价值，但当前不具备正式迁移条件。** 这次没有发现必须重建Loadout路由器或数据库的理由；新增运行时适配只有两个元数据字段。阻止验收的是实际上游/官方会话可用性、桥接能力和部分旧路由行为尚未迁移。不能把这些失败解释成“需要退回全量自研”，也不能因为离线用例通过就宣布替代成功。

三类入口按同等重要性验收；没有将失败的商汤、CommandCode或WorkBuddy降为可选项。

## 实际结果

| 链路/能力 | 结果 | 证据及边界 |
|---|---|---|
| Codex 0.165.0-alpha.1.2 → OmniRoute | PASS，合成上游 | 官方CLI，独立CODEX_HOME，Responses，经原生转换完成；没有使用现有Codex会话/订阅登录 |
| Claude Code 2.1.309 → OmniRoute | PASS，合成上游 | 文本及Read工具两轮完成，工具实际读出fixture内容 |
| OpenCode 2.0.32 → OmniRoute | PASS，合成上游 | 文本及read工具两轮完成；修正的是测试profile的v2参数/Location相对读权限，没有改网关协议 |
| Codex → OmniRoute → CommandCode | BLOCKED，真实HTTP 401 | 精确转发`deepseek/deepseek-v4.1-flash`，1次真实请求；凭证/服务原因未进一步确定，停止其他客户端调用 |
| Codex → OmniRoute → 商汤 | BLOCKED，真实HTTP 429 | 精确转发`deepseek-v4-flash`，选定1号账号；原生executor额外重试两次，共3次。未试探其他11账号、未换模型绕过拒绝 |
| Claude Code → OmniRoute → WorkBuddy | BLOCKED，503 | 官方IPC locator不存在；独立桥接已启动，但无法获得官方会话。未启动或重启正式WorkBuddy |
| WorkBuddy工具请求 | 明确400拒绝 | 现有桥接只有文本能力；不能承诺完整编码Agent可用 |
| OpenCode → OmniRoute → OpenCode免费桥接 | PASS，真实上游 | 独立官方worker，`deepseek-v4.1-flash-free`，收到合成成功标记；没有使用生产bridge key/会话数据库 |
| 加密备份恢复后的免费链路 | PASS，真实上游 | 新实例、新vault service、原client key和模型，再次完成真实调用 |

真实免费链路共两次成功且遵守120秒间隔，仅证明当时可用。既有历史403并未被删除；两次成功不证明持续稳定、免费池SLA或所有平台可用。两个真实API Provider未成功，因此它们恢复后的真实推理仍是**未验收**。

### 协议验收与真实可用性分开

最终独立客户端测试5/5通过：三客户端文本，Claude/OpenCode工具往返。没有声称Codex工具执行或真实付费上游工具能力通过。真实外部拒绝后用mock继续验证客户端接线，不能将mock结果计作真实Provider PASS。

OpenCode免费桥接仍实施单并发、每轮间隔120秒、403后冷却30分钟。多轮工具调用每轮都受这一限制；客户端是否能正确等待仍未完成真实编码工作流验收。WorkBuddy不支持tools是另一条独立能力缺口。

## 固定底座与最小改动

固定上游`v3.8.52 / ad6f33f66bda34eba98645d3bfdabbde294bc843`。没有自动追默认分支，也没有改正式Loadout代码。

042已有430行自有适配、14个上游接缝，本轮自有模块只新增`prefix`、`nodeName`两个已知元数据字段，合计432行；上游hook数量不变。原生自定义Provider CRUD会自动填充这两个字段，旧白名单因此返回500；修正后完整后台包重建，CRUD、带斜杠模型、恢复路由通过。首轮失败报告保留。

- 新包：`development/experiments/omniroute-phase4/runs/package-1793349143406`。
- 固定基线、新增差异、适配模块快照。
- Node、server入口及源码补丁有hash；**完整依赖树hash未计算**，不是已签名的公开发布包。
- 新增两行补丁在v51实验源码上`git apply --check --ignore-space-change`通过；默认严格应用因CRLF上下文失败，保留这一区别。这是静态重放检查，不是又一次v51→v52实际升级。
- 042已实跑的跨版本migration/回滚证据继续有效于其当时包；旧完整补丁仍存在8个冲突日志，不能因本轮两行易重放就宣称以后无冲突。

没有修改选路热路径，不重构SQLite，不新增第二路由器；本轮未重跑性能benchmark，沿用042该路径的测量。此次重建只改变凭证写入时的元数据白名单。

### 新发现：重试有两层

商汤429后，即使配置`requestRetry=0`和Codex重试为0，原生`open-sse/executors/base.ts`仍按`RETRY_CONFIG={maxAttempts:2,delayMs:2000}`重试同一URL。初次真实测试观察到3次请求，已如实计入。

本轮没有为此再改运行时。测试出口现在在首次401/403/404/429后锁住，新增5/5反例证明之后的尝试不会再次到达上游。未来若产品要求“立即换号/不得同账号重试”，需要选择原生`skipUpstreamRetry`/combo语义并复验，不能只改全局requestRetry。这是正式迁移前的行为对齐项。

## 迁移、备份、恢复与回退

新增隔离工具及使用说明：

- 只读迁移预览；仅按指定ref读取API key，13账号进入独立OS vault；外部OAuth、网页登录不复制。
- 原生Provider node/connection/key API写入，保持来源账号顺序；client key重新生成，分Provider限制连接范围。实验账号使用`account-N`别名。
- 未知client policy/账户集合拒绝自动迁移；Pool和专用URL差异必须显式`--accept-partial`，不能静默忽略。
- 停实例、检查活跃owned PID、SQLite WAL checkpoint；数据库、凭证及运行配置使用scrypt+AES-256-GCM加密。
- 恢复前验证口令/完整性，在全新目录和随机vault service恢复；原目录保留，回退通过恢复旧副本完成。错误口令、篡改、既有目标均拒绝。
- 当前rollback工具限定固定基线，不做跨schema逆向迁移或生产切换；通用未来版本恢复不在本次通过范围。

最终工具测试14/14通过。真实13个源API key及独立WorkBuddy测试key完成加密备份/新service恢复，计14项provider凭证；免费桥接另有实际恢复后推理成功。没有把“凭证恢复成功”说成“13个账号均能推理”。

**剩余迁移差异：** 当前`gpt`池包含codex-sub、CommandCode和opencode-go，不在本轮API-key选定范围内完整覆盖；`workbuddy-flash`依赖尚不可用的官方会话；商汤旧`/e/sensenova/v1`及30秒等待语义没有完整移植。接口形式可以变，但这些行为必须明确映射并验收。当前工具是可复验的partial迁移，不能拿去切生产。

测试备份采用一次性随机内存口令，清理后不作为长期用户备份；正式备份需要用户保管口令。OS vault是允许的受保护持久化，Node/Python内存不承诺Rust式清零。

## 隔离与清理

正式配置文件hash、源账号/路由元数据及13个源credential值前后相同。对原/恢复实例12个文件及后台内存日志搜索真实key，明文命中0。正式daemon仍为原安装路径、PID10532、启动于2026-10-30 01:20:43本地时间，早于本轮实验。

最终16个实验实例的OS credential剩余0，owned进程剩余0；一个旧worker PID被无关IDA进程复用，按exe路径识别后没有终止它。清理回执。未做UI、正式配置切换、自启修改、commit或push；脚手架按用户要求保留。

## 证据索引

可保留的脱敏回执及SHA256见验收索引。最终客户端复验另使用独立Git根，确认请求未附加父仓库development-harness指令；早期真实失败探针未保存正文，不对其所有隐式上下文作追认。

| 内容 | 本地目录（相对仓库） |
|---|---|
| 首轮自定义Provider失败 | `development/experiments/omniroute-phase4/runs/tools-1793348765549/result.json` |
| 工具最终14/14 | `development/experiments/omniroute-phase4/runs/tools-1793350504994/result.json` |
| 真实API Provider及WorkBuddy、凭证恢复 | `development/experiments/omniroute-phase4/runs/live-1793349346800/result.json` |
| 三客户端最终5/5 | `development/experiments/omniroute-phase4/runs/clients-1793350451061/result.json` |
| 免费桥接真实成功 | `development/experiments/omniroute-phase4/runs/bridge-live-1793349711836/result.json` |
| 免费桥接恢复后真实成功 | `development/experiments/omniroute-phase4/runs/bridge-live-1793349887685/result.json` |
| 出口拒绝停止5/5 | `development/experiments/omniroute-phase4/runs/relay-1793350061478/result.json` |

`runs/`包含本机验收资料，不提交。旧client诊断中的字符串匹配可能误命中随机ID（例如成功记录中出现“403”），不能把它当HTTP错误；HTTP结论以上游relay状态和成功回答为准，后续代码已抑制成功用例的该类诊断。

官方参考：[Codex配置](https://learn.chatgpt.com/docs/config-file/config-reference)、[Codex非交互执行](https://learn.chatgpt.com/docs/non-interactive-mode)、[OpenCode v2权限](https://opencode.ai/v2/docs/permissions/)。其余协议和CLI选项同时核对本机对应版本源码/help，没有复制新的第三方协议代码。

## 下一步判断

保留OmniRoute为候选唯一运行时，暂缓UI和正式迁移。下一轮先在用户正常登录/服务恢复后复验CommandCode、商汤、WorkBuddy，再对齐原生重试、两个池和商汤端点等待规则，最后验证真正多轮coding会话。若这些需求必须通过新增大范围协议fork或削弱免费桥接保护才能满足，再重新比较底座；现有证据尚不支持走回全量自研。
