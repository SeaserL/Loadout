# 054 原生只读目录、用量与路由诊断

## Goal
接通OmniRoute原生只读查询，在最终统一隔离包验证并只保留一个候选。
## Context
用户授权依053继续，优先直接复用；固定v3.8.52，无UI、正式环境变更、提交上传。
## Existing behavior
现有proxy按管理路径白名单放行；原生models/provider-models、usage history/latency/call-logs/route-explain/combo-trace未开放。已有noLog与requestLoggingEnabled=false保护正文，原生usage_history/call_logs仍可保存元数据。
## Constraints
只读入口仅GET/HEAD，管理员会话访问，普通推理key不能读取全局管理数据；不触发真实Provider，不开放目录同步或改写接口。保留现有账号范围。
## Architecture impact
仅扩展已有proxy接入点与操作说明，复用原生路由、SQL、缓存和数据结构；不新建指标库或诊断服务。解释重算与真实短期trace明确区分。
## Files expected to change
OmniRoute src/proxy.ts；fusion操作说明及限定版本升级；development/experiments/omniroute-phase14；来源清单、ADR-019与功能状态。
## Implementation stages
1. 核对原生接口、许可证与读取/存储边界。
2. 开放限定只读方法，准备合成统计/模型测试。
3. 构建统一包，专项及现有回归、适配版本升级和性能检查。
4. 清理临时产物、记录限制及唯一候选。
## Tests
无认证/推理key/伪造Origin拒绝、写方法禁用；目录及映射、真实mock调用后token与用量、模型延迟、调用索引/解释/trace；无body/凭据落盘；重启/恢复后元数据可读；原有120及订阅18项。
## Performance validation
原有最终包mock HTTP benchmark；查询只在管理路径，不增推理采集链路。
## Security considerations
复用noLog和现有凭证保护，检查读取结果与实例磁盘；不暴露详情/正文/导出接口。管理视图是全局管理员视图，非推理key子集视图。
## Migration considerations
测试053→054同底座适配升级，未知版本对拒绝；真实账号与未来OmniRoute升级仍未验证。
## Known risks
静态目录/可用标记不等于真实探测；成本为估算；route-explain有事后重算，combo trace有内存TTL；原生错误字段可能含上游文字；合成api_key字段回显已验证脱敏，不对任意自由文本给出完全脱敏承诺。
## Definition of done
最终同包查询可用、原有边界通过、只有当前候选、测试凭据/进程清理、来源及限制明确。
## Progress
- [x] 已查原生接口、存储及管理鉴权实现，MIT根许可证；所用路径无另行license声明。
- [x] 仅扩展现有proxy的7类只读路径，操作复用admin GET；未加新API/采集/SQL/schema。
- [x] 后台完整构建及首轮只读/订阅检查；发现Host测试传输问题与升级mock缺usage，保留失败回执。
- [x] 升级验收发现admin工具禁止query；只为指定只读GET放行，重封工具；原生后台不变。
- [x] 最终7f8bb48c76cd ZIP：只读20/20、订阅18/18、053→054升级回滚4/4、集中回归120/120；本地HTTP p95 94.26ms。
- [x] 27个隔离测试实例审计：凭据0、活跃测试进程0、工具源码不一致0。
- [x] 清理22项临时产物，共11.77GB；D盘8.23GB/C盘3.54GB。D:\Loadout约6.71GB，只有1个395.6MB当前候选，展开包0。
- [x] 更新README、来源表、ADR和功能清单；[最终报告](../../research/2026-10-31-native-readonly-observability.md)记录实际结果及原生限制。
## Decisions
使用原生history/latency汇总和call-logs索引，不引入复杂analytics端点或第二统计层；本轮不开放免费目录/Radar同步。
