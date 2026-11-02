# ExecPlan 046: 429节流修复与WorkBuddy渠道诊断

## Goal
优先解决045的429后同账号节流失效；先查上游现成修复，再验证原生配置，必要时评估最小补丁，不重写路由。继续定位WorkBuddy11128。
## Context
用户授权隔离修复/评估，CommandCode由用户处理。045已证明原生20秒minTime在429 eviction后约2.1秒即可重试。
## Existing behavior
唯一运行时为固定v3.8.52，核心17项通过，节流反例FAIL。原生Bottleneck与providerDefaultRateLimit是现有 abstraction。
## Constraints
不做UI、不动正式服务/数据/配置/凭据。商汤仅mock，不试探真实风控；CommandCode零调用。WorkBuddy先核对官方源码；按用户追加请求尝试指定项目的只读内存凭证和连接兼容能力，固定账号/模型/域名及调用次数，不刷新、不落盘副本、不改写提示词。授权细分见ADR-019的046小节。原冻结包保留。
## Architecture impact
优先原生配置或上游窄修复；需要patch时只改隔离rate-limit组件，不增加账号选择器/协议层。
## Files expected to change
phase7脚本/证据/必要候选patch；046报告、ADR019、reference-map和必要notice。
## Implementation stages
1. 检查latest release/main及相关PR、WorkBuddy源码。
2. 复用045反例，验证原生滑动窗口是否足够；必要时构建窄补丁候选。
3. 重试/范围/并发/取消/预算/恢复及普通请求性能回归。
4. 出具可采用范围、维护增量与剩余阻塞，清理。
## Tests
429短hint后至少20秒、长hint不缩短、无hint、三协议、同模型换号不越界、并发排队/取消/预算、备份恢复；Workbench诊断仅状态类别与错误码。
## Performance validation
原生配置对照及mock正常请求时间；如patch运行时，追加相关上游单测和实际包回归。
## Security considerations
随机隔离OS vault、loopback mock、只记录脱敏数据。真实诊断固定模型/次数、LOADOUT_LIVE_TESTS=1，失败停止。
## Migration considerations
配置成本与patch成本分别记录；不将latest main研究等同升级验收；scope/key/model契约保持。
## Known risks
原生quota窗口记录admission而非dispatch，不能单凭该配置宣称实际间隔；本轮候选改在执行前记录并由完整包测量。进程重启不保留历史，上游5000-key上限仍在；11128重载多种错误，不仅缺header，禁止凭issue猜测本机原因。
## Definition of done
故障有可复现修复或明确的最小补丁评估，失败保留，无生产变更，维护结论明确。
## Progress
- [x] Boot sequence与045回执复核。
- [x] 上游main仍有429 eviction；相关11413未合并。开始配置替代验证。
- [x] 429回归与维护增量评估：完整候选23/23，三协议约20秒，25秒hint保留，授权换号与排除账号边界通过。
- [x] 429失败单测复现；dispatch floor候选相关56项通过，开始完整后台构建。
- [x] 用户指定WorkBuddy参考已固定25680ca；发现旧IPC无法传会话头，隔离试验仅复用只读解密/连接头。
- [x] WorkBuddy连接模块7项mock通过；真实冻结后台→隔离桥接→新连接200/LOADOUT_OK，1.95秒，账号和凭证文件未改。
- [x] 完整候选后台构建成功；新增限流接缝1个文件+25/-1，静态试套当前main通过。
- [x] WorkBuddy诊断与收尾：累计38实验实例凭据/owned进程均0，报告与参考许可登记完成；仍未正式部署。
## Decisions
- 当前不自动升级底座；先利用原生provider quota window，验证不足再评估patch。
- 原生provider quota override只允许整数rpm（60秒window），不能表达1次/20秒实际dispatch间隔；rpm3可突发、rpm1改变吞吐和等待语义，因此复用原生SlidingWindowLimiter增加独立dispatch history。
- WorkBuddy不装完整DSH插件，复用MIT参考的只读内存解密和请求头函数；保留既有bridge的账号绑定、固定模型及文本限制。工具/长会话/凭证轮换仍未验收。
- 只读审计发现正式clientPolicies比044快照多一项，本轮未写该库，来源未归因；保留差异、不覆盖正式配置，其余metadata及冻结安全adapter检查另记录。

## Outcome

建议继续最小适配融合。429候选+25/-1一个限流文件；WorkBuddy指定项目的窄连接实际可用，不安装完整DSH插件。真实商汤风控、WorkBuddy工具/长会话/凭证轮换和正式迁移仍未验收，不据此扩大patch或切换生产。

详见[046报告](../../research/2026-10-30-omniroute-pacing-workbuddy-connector-validation.md)与实验README。
