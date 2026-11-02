# ExecPlan 057 — 原生配额、预算与模型同步

## Goal
接通 OmniRoute 原生账号配额只读查询、预算管理与限额执行，以及按账号的模型发现与同步。优先少维护，不另造预算或模型目录。

## Context
继承 056 候选包和固定 v3.8.52 底座。用户明确授权这两项；仍只隔离验证，不做 UI、不动正式环境、不提交上传，结束只留一个候选 ZIP。

## Existing behavior
已有只读模型目录、价格同步、用量统计和账号路由规则。相关原生管理路由被发行入口限制，业务模块已随后台构建。

## Constraints
保留 OS vault 凭证、账号范围、精确模型规则。禁止真实 Provider 调用和真实客户端配置写入。接受原生 SQLite 与预算窗口语义，不承诺严格预付费余额扣减。

## Architecture impact
仅扩展管理入口与 CLI 查询路径白名单；优先零新增业务模块。配额观察、Provider 缓存与本地预算分别使用原生接口，文档明确来源与未知值。模型同步手动按账号触发，不打开后台任务总开关。

## Files expected to change
上游 src/proxy.ts；fusion/runtime.mjs、backup.mjs、README.md；development/experiments/omniroute-phase17；ADR-019、能力表、来源表与验收记录。

## Implementation stages
1. 核对原生语义与副作用；开放最小入口。
2. 隔离回环模拟 Provider，验证预算执行与模型同步后的账号边界。
3. 封装单一候选，最终包集中回归、056→057 升级与回滚。
4. 审计凭证与进程、清理临时包及缓存、更新文档。

## Tests
复用原生预算、模型导入和配额测试；最终包测试认证、只读边界、真实 HTTP 限额拦截、未知价格、目录刷新失败、按账号目录、精确模型、持久化与备份升级。所有账号与响应均为合成数据。

## Performance validation
不修改推理热路径；沿用最终包回环请求 benchmark，区分完整 HTTP 延时与 Router 开销。

## Security considerations
仅管理会话可访问新路由；拒绝推理 key、跨域写入；无真实凭证或请求正文落盘。只读配额不主动刷新上游。模型发现的出网受现有隔离 guard 约束。

## Migration considerations
无自建 schema；沿用原生 migration。仅登记已验证的 056→057 适配升级对，回滚使用原备份和原包。

## Known risks
原生 quota 包含合成限流压力，不能当真实余额；预算可能事后统计，未知价格及并发在途行为需要如实说明；模型同步可能触发原生 CLI 配置写入或陈旧条目清理，检查默认关闭及隔离保护。

## Definition of done
两项功能最终包可用、范围与精确模型约束通过、升级与回滚通过、凭证清理完成、只留一个 ZIP，局限有明确文档。

## Progress
- [x] 阅读相关约束、前序验收与原生入口，确认原生 abstraction。
- [x] 最小入口：仅修改 proxy 白名单、CLI 查询路径及升级对。原生预算/同步测试 34/34；另 3 项旧 fixture 缺少强制 vault 初始化，保留失败证据，本轮通用接入在包级真实启动验证；原生特定分支不计通过。
- [x] 最终包新功能专项 22/22（管理边界、真实 mock 预算拦截、模型同步、范围与精确模型、持久化及加密恢复）。
- [x] 同一最终包232项通过，含集中回归120、只读20、Radar23、价格25、订阅18、新功能22与升级4。
- [x] 056→057升级/回滚通过；审计27实例零凭据/进程/源码差异；清理27目标约11.78GB文件，唯一ZIP、零展开包，仓库约6.71GB。来源/状态/操作说明与报告已更新。

## Decisions
- 不开放全量 /api/quota 控制面；先复用 usage/quota、provider-limits、usage/budget 和 keys/:id/usage-limits。
- 模型更新只开放 providers/:id/models 与 sync-models，保留原生每账号缓存，不启用全局自动同步或客户端配置同步。

- 验证中修正测试调用：key 使用 PATCH 而非 PUT；命名账号默认是优先账号，严格固定需 allow_account_failover=false；Combo 超预算不保证使用单模型路径的 budget_exceeded 字符串。均未改变产品逻辑。

## Outcome

两项均沿用原生能力接通，无新增业务模块或schema，无Router改写。保留真实Provider未验、缓存非余额、预算非严格预付费、原生特定fixture未通过及新上游版本未验的限制。候选fusion-v3.8.52-isolated-057.1-df64cd42e4fa。无UI、正式环境改动、提交或上传。
