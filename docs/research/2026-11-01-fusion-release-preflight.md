# 058 发布收口与实际迁移预检

状态：发布入口、最终包240/240项验收与实际只读迁移预检已完成。首次失败临时目录的清理被自动审批阻塞，单独保留；不据此扩大开发范围。

## 本轮边界

冻结057网关功能。OmniRoute仍固定v3.8.52；不新增Provider、路由、UI或客户端调试，不改正式安装/数据/凭据，不提交上传。外部账号事项不排查。

构建、封包、回归、升级、只读预检及清理统一到`scripts/fusion-release.ps1`；操作方法见[发布说明](../architecture/fusion-release-operations.md)。现有回归移至`tests/fusion/`，复用既有规则和断言；只新增预检边界检查，不另建网关框架。

WorkBuddy选定模块与原notice移到`fusion/vendor/`，三文件SHA均与原实验模块一致。既有依赖目录移动到`development/fusion/tools/`，未重装或复制整套node_modules。保留一个OmniRoute开发工作副本。

旧057包manifest实际记录Node24.22.1，与旧文档24.19.0不一致。本轮从已核验旧ZIP复用实际二进制并固定版本，补匹配官方LICENSE及URL/hash。首次构建因D盘缓存ACL被SWC拒绝；257/257临时源码已恢复，失败日志留存。后续使用独立受限ACL的临时SWC缓存，不放宽安全检查。

## 实际旧库核对

通过既有`snapshot()`进行只读事务查询，仅输出脱敏账号引用与配置，不读取账号秘密字段或正式vault。初次投影为21个启用账号、2个旧池、1个启用端点、0 account sets、2条客户端策略。

| 项目 | 当前旧配置 | 迁移处理 |
|---|---|---|
| GPT池 | 手工顺序：Codex订阅/gpt-6-astra → CommandCode/gpt-6-luna → OpenCode Go/gpt-6-luna | 保留3成员及原有跨模型显式池，不改为自动模型选择；订阅沿用受控登录接入。 |
| WorkBuddy池 | 单成员，deepseek-v4.1-flash | 沿用桌面登录绑定与窄连接器，不另做客户端适配。 |
| 商汤端点 | 主账号范围12个账号；5个模型；旧wait_secs=30、defaultModel=null；源表无rotate列，按false读取 | 逐模型生成原生Combo，成员范围不缩小；接受已授权的原生有界等待语义，不保留严格30秒总期限。 |
| 受限客户端 | loadout/default、deepseek/main/deepseek-flash、workbuddy/main/deepseek-v4.1-flash；8 RPM | 三个引用均需明确映射并保留限速；不以测试模型补默认值。 |
| 通用别名 | loadout/default、loadout/fast、loadout/realtime | 列为显式决策，不重造旧动态分档算法，不自动纳入订阅。 |

21个账号中，17个已有现成接入方式：商汤12、Codex订阅/CommandCode/DeepSeek/OpenCode Go/WorkBuddy各1。Claude Code与Codex各1仍是只读订阅来源。OpenCode Free桥接和ChatGPT网页会话各1不在当前已验传输profile中；保留来源、明确暂缓，不能静默丢弃后宣称全量迁移完成。

“已有接入方式”不代表真实凭据已绑定。此轮不读正式凭据、不登录、不迁入，预检始终报告`activationReady=false`；后续使用已有工具补绑定和别名，再验证实际配置即可，无需继续堆后台功能。

## 验收与限制

唯一当前候选：`fusion-v3.8.52-isolated-058.1-39f5ab1fafa9`，396,021,189 bytes（约396 MB）。ZIP SHA-256：`84bb5edd4fc1148c966f73a148d1a10db32b1de02031be4bc70ce2d03959f579`。路径和证据索引见当前候选回执。

- 15组、240/240通过：既有232项 + 8项只读预检边界。新包CLI预检也通过合成源库验证，未修改源文件或输出敏感列。
- 057→058使用真实完整旧包建立合成状态、加密备份并恢复到新包；凭据、原生预算/目录/价格/Radar/路由状态保留，旧包可重新开启回滚，未知版本对拒绝。4/4通过。仍是同一OmniRoute底座，不代表未来上游升级已经验证。
- 21个测试实例审计：残留凭据0、活跃进程0、包内适配源码与工作源码差异0。核心完整HTTP mock基准20次，p50 63.50ms、p95 79.98ms；不是上游延迟或纯Router指标。
- 最终包实际预检两次元数据投影一致；未提供新后台真实账号绑定，`activationReady=false`。明确缺项：`CLIENT_ALIAS_MAPPING_REQUIRED`、`REQUIRED_ACCOUNT_BINDINGS_INCOMPLETE`、`DEFAULT_OR_CLIENT_ALIAS_MAPPING_REQUIRED`。
- 上游运行时补丁与057一致，10个自有接入源码文件也一致。Next仅向tsconfig自动补了固定构建目录的两条include路径；没有新增运行时补丁或第二业务层。

单独`-Action preflight`也已成功运行并清理临时解包，结果一致，见预检入口回执。

详细证据：集中回归、实际预检、升级、审计。

现有Rust/桌面许可证策略检查通过（348项、0策略违规，7项缺包内许可证文件，范围不是整个OmniRoute依赖闭包）。上游全量typecheck和其他Provider/界面测试仍不在本轮证明范围。

商汤风控429、CommandCode401、WorkBuddy自然凭证轮换继续DEFERRED；本轮未真实调用或排查。不扩展客户端调试。

## 存储与下一步

当前仅一个候选ZIP，旧057 ZIP、成功运行的展开包/测试数据、Webpack构建缓存和SWC临时缓存均已清理。首次失败目录 `development/fusion/runs/20261031T130254339-d0118775` 仍占1,182,367,412逻辑字节（约1.10 GiB，主要是旧包临时解包）；自动审批拒绝删除，仅返回`blocked by policy`，未绕过。它不是主动保留的回滚基线。见残留记录。

默认/精确别名按第一轮实际使用范围逐项决定；未使用的fast/realtime可以明确暂缓，不要求为了迁移核对而先开发更多动态路由功能。

下一步顺序：明确默认/精确别名 → 用已有API-key、受控登录、桌面绑定工具完成有范围的隔离迁入 → 对这份配置做有界日常使用与加密恢复验收。真正更换上游版本时再做完整升级演练，不连续追版本，不把新UI或更多原生接口当成前置任务。
