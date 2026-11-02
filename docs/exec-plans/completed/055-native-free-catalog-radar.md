# 055 原生免费目录与 Radar 同步

## Goal
接通原生免费目录、Radar缓存/更新状态、手动及后台同步；明确日期与过期含义，只留一个隔离候选。
## Context
承接054固定OmniRoute v3.8.52，用户优先少维护；无UI、正式变更、提交上传或真实Provider调用。
## Existing behavior
原生free-models为发布时静态目录；Radar签名缓存带generatedAt/fetchedAt。目录后台按fetchedAt+24h、每小时检查；页面6h规则不是后台规则。候选屏蔽Radar入口并关闭通用后台服务。
## Constraints
复用原生名单、校验、缓存、merge及scheduler，不建第二列表/同步服务。仅管理员会话；社区feed范围，不接supporter凭证。隔离loopback签名feed验证，不放宽已有出站限制。
## Architecture impact
扩展既有proxy、外围配置/启动和版本对；调用原生scheduler。settings仅optIn，拒绝supporterKey写入；不开同步全付费feed、本地模型覆盖或自动建账号。
## Files expected to change
OmniRoute src/proxy.ts、src/app/api/radar/settings/route.ts、既有loadoutPhase1/runtime.ts；fusion runtime/supervisor/backup/README；phase15实验及来源/状态文档。
## Implementation stages
1. 核对原生源码与许可证、日期/缓存/版本语义。
2. 最小入口及原生scheduler接线，社区目录配置。
3. 原生focused测试、最终包签名feed隔离专项、原有回归和054→055升级。
4. 清理旧包/构建缓存/测试数据，保留证据与唯一候选。
## Tests
默认opt-out无下载；生成/获取日期区分；签名/schema/版本/大小/网络失败保持旧缓存；重启和opt-out后台行为；无账号/池/推理权限变化；管理员/CSRF/方法边界；备份升级保留缓存；实际CLI命令。
## Performance validation
同包已有mock HTTP benchmark，不新增推理热路径工作。
## Security considerations
用原生Ed25519验签；测试私钥只在内存。配置仅feed地址和公开验签key，经已有出站限制。拒绝Radar supporterKey以免扩大凭证生命周期适配。
## Migration considerations
054→055同底座版本对；原生Radar表已存在，无新schema。跨未来上游版本另验。
## Known risks
原生stale返回指版本未更新，不等于缓存按时间过期；旧目录可继续读取，generatedAt不保证实时。相同版本不会更新fetchedAt，可能每小时再检查。原生scheduler同时尝试referrals，付费feed无key不下载。宿主服务可用性及免费平台有效性不由mock证明。
## Definition of done
同包目录/状态/同步及隔离边界有证据，改动保持小范围；文档明确实际语义，只留当前候选，测试凭证/进程清零。
## Progress
- [x] 已核对原生目录、日期、版本floor和定时同步；当前根MIT，所用目录无额外许可。
- [x] 接通既有proxy、原生optIn/sync及独立Radar scheduler启动；supporterKey输入明确拒绝。
- [x] 原生7组focused测试118/118通过，使用隔离数据目录与出站guard。
- [x] 最终包Radar专项23/23；首轮测试自身的请求归类、阻塞mock及原生Origin断言问题已修正，保留失败历史，同一候选未改产品代码。
- [x] 同一最终包185项通过（Radar23、只读20、订阅18、升级4、集中回归120）；19个测试实例凭据/进程清零，包与工具源码一致。
- [x] 删除17处临时材料（10.17 GB文件数据）；D:\Loadout去重约6.71 GB，仅保留055 ZIP 396.0 MB、展开包0。报告、来源图、README、功能清单与ADR结果已更新；正式环境/UI/Git提交上传均未触碰。
## Decisions
社区目录先接通；supporterKey拒绝，保持现有凭证保护。启用Radar功能入口不等于opt-in；不启用全部后台服务。
