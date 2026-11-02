# 056 原生价格目录与同步

## Goal
接通OmniRoute价格目录、来源与同步，并验证成本统计衔接，不维护第二价格表。
## Context
承接055固定v3.8.52；用户要求少维护、隔离验证、无UI/正式变更/提交上传，只保留一个候选。
## Existing behavior
原生pricing读取四层合并及sourceMap；pricingSync仅支持LiteLLM固定GitHub URL，lastSync为本机成功写入时间，无上游生成日期。通用后台服务在候选中关闭，管理价格入口未开放。
## Constraints
不改Router、原生计价器或schema；不开放价格手工编辑/删除。保持管理员与出站边界；所有下载与推理测试为合成loopback流量。
## Architecture impact
扩展既有proxy、admin查询参数；自有启动模块调用原生initPricingSync，通过实例布尔开关映射PRICING_SYNC_ENABLED，默认false。原生同步URL不改；测试进程在网络边界将唯一固定URL映射到loopback，产品包不含测试替换。
## Files expected to change
src/proxy.ts、自有loadoutPhase1/runtime.ts；fusion runtime/supervisor/backup/README；phase16实验及状态/来源文档。
## Implementation stages
1. 核对来源、时间、分层、缓存失效与成本统计。
2. 最小接线与原生focused测试。
3. 构建同一完整包，价格专项、既有回归、055→056升级及回滚。
4. 审计清理，只留当前候选与源码/精简证据。
## Tests
默认无下载、管理员/方法/Origin边界、来源优先级、dryRun不写入、同步成功/失败/空数据、日期持久化、正常CLI、重启及原生定时初始化；实际mock推理usage与同步价格估算、历史重算/已存cost优先；账号路由不变、凭证与正文不落盘、备份升级恢复。
## Performance validation
沿用同包本地HTTP mock benchmark；不添加推理前网络取价。
## Security considerations
价格是公开元数据；同步沿用30秒timeout、既有出站allowlist。原生无Radar式签名或生成日期，不虚称具备。测试网络映射仅在实验进程加载，包内容及正常启动另验。
## Migration considerations
无新schema；055→056为同底座精确适配版本对，未来底座另验。
## Known risks
原生HTTP成功且空有效数据可能覆盖已同步层；sourceMap为模型级而非字段级。lastSync不等于上游发布日期，nextSync是估算而非定时器证明。未知价格可能按0计，部分历史会以当前价重算，不能作为真实账单。
## Definition of done
最终同包有来源/时间/同步/成本衔接证据，边界如实记录，无第二价格库；只保留一个候选，测试进程/凭证清零。
## Progress
- [x] 已核对原生价格接口、同步与分层；相关路径继承OmniRoute根MIT。
- [x] 既有proxy及自有启动/工具最小接线完成；原生9个focused文件53/53通过。完整后台构建成功，临时UI占位恢复257/257。
- [x] 最终包价格专项25/25、055→056升级/旧包回滚4/4通过，包内原生同步/计价未改。
- [x] 同一候选210项全部通过；19个测试实例凭据/进程清零，包工具与源码一致。
- [x] 审计及文档完成。删除17处临时材料（10.17 GB文件数据），项目去重约6.71 GB，仅留056 ZIP 396.0 MB、展开包0；源码/未提交修改保留。无UI、正式环境变更、提交上传。
## Decisions
只开放原生只读目录及sync GET/POST；手动同步不依赖自动开关。自动同步通过停机配置显式启用，不恢复全部后台服务。models.dev层仅保留原生读取，不启用另一同步器。
