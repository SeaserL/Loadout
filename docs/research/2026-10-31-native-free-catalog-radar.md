# 055 原生免费目录与 Radar 隔离验证

固定OmniRoute v3.8.52 / `ad6f33f66bda34eba98645d3bfdabbde294bc843`，承接054候选。仅后台融合，无UI、正式安装/数据/凭据变更、提交或上传。

## 复用范围

| 能力 | 原生实现与入口 | 接线范围 |
|---|---|---|
| 静态免费目录 | `open-sse/config/freeModelCatalog{,.data}.ts`，`GET /api/free-models` | 直接读发布基线，不复制维护另一份名单；没有独立更新时间。 |
| Radar目录 | `src/lib/radar/index.ts` / `applyFeed.ts`，`GET /api/radar/catalog` | 原生静态基线与签名缓存合并。无缓存/损坏时回退基线；不自动创建Provider、账号或combo。 |
| 更新状态 | `GET /api/radar/status`，`src/lib/db/radar.ts` | 原生cache版本、tier、generatedAt/fetchedAt、optIn；缺失日期不补造。 |
| 下载开关 | `GET/POST /api/radar/settings` | 原生optIn；本轮只接受该字段，拒绝supporterKey及其他输入，避免扩大凭证生命周期。 |
| 手动同步 | `POST /api/radar/sync` | 原生sync/signature/schema/版本检查，仅空body或`{}`；没有自有同步器。 |
| 后台同步 | `src/lib/radar/scheduler.ts` | 在既有隔离启动模块调用原生init；保留通用后台服务关闭，仅恢复Radar的opt-in调度。 |

所有管理入口要求管理员会话，普通推理key不能读取或改写全局Radar状态。既有Host/Origin与原生鉴权管线、出站限制保留；不开放sync-all、付费offers/intel、本地模型覆盖和引导建账号入口。原生浏览器写保护依据Origin/Fetch Metadata，同源或无Origin的已认证请求并非一律强制CSRF token；融合入口额外拒绝外部Origin，测试验证跨站写入拒绝，不宣称逐次token强制校验。

## 时间与过期语义

- `generatedAt`是上游数据生成时间；`fetchedAt`是本机接受这个版本的时间。最近下载不代表数据最近生成。原生社区目录在固定版本文档中描述为可能延迟约30天；本轮不验证托管服务的当前供应、时效或平台额度。
- 原生后台每小时检查一次，catalog到期点为`fetchedAt + 24h`。缺少缓存立即到期；重启且已opt-in时也检查。referrals使用独立1小时窗口。原生页面的6小时规则没有接入无UI后台。
- `stale`同步结果指同/旧版本被版本floor拒绝，不是一个“缓存按时间失效”标记。同版本不刷新fetchedAt，因此达到24小时后可能每小时再检查。
- 旧缓存不会因时间到期自动删除或禁止读取。`available:true`表示有缓存，不能作为“最新”“免费仍有效”或“账号可用”的证明。
- status只返回缓存状态；未另建持久化lastAttempt/lastError。手动sync返回当次结果。offers/intel本身未保存generatedAt时，原生status不会伪造该字段。
- opt-out保留缓存并阻止后续tick/手动同步，原生实现不主动撤销已发出的下载。同步下载为公开目录数据，不携带Provider配置、客户端key、对话正文或推理流量。

## 同步与安全边界

使用原生30秒网络timeout、10MiB响应上限、Ed25519签名、schema和版本检查。签名/schema/响应大小/网络错误保留最后有效缓存。没有关闭验证或伪造成功状态。

实例可通过`radar.feedUrl/publicKey`映射原生环境变量，供自托管与隔离测试；publicKey仅接受公开Ed25519 SPKI-DER，拒绝私钥与URL凭据。仍受独立的network/hosts规则约束。未导入任意环境变量，也未给测试放开外部网络。

本轮只接社区feed。原生scheduler附带referrals检查；无supporter key时不下载付费feed。将来需要付费Radar key时，应单独评估其安全生命周期，不通过本轮设置接口暗中落盘。

## 验收记录

原生7个focused文件118/118通过，见原生测试。这证明原生纯逻辑/模块用例，最终包运行结果另列，不混合为生产可用性。

最终包签名feed专项23/23通过，见专项证据。覆盖默认opt-out、签名/schema/版本/大小/网络失败保留缓存、日期区分、手动与重启调度、管理边界、真实CLI及加密恢复。所有下载测试使用loopback模拟feed及内存生成的签名私钥；没有真实Provider请求或托管Radar下载。升级的旧cache使用明确标记的离线公开元数据fixture，不能当成签名下载验收。

首轮失败保留在radar-history：测试把合成Provider创建时的模型探测误计为Radar下载；同步CLI使用spawnSync阻塞了同进程mock server；鉴权断言误设为所有请求强制CSRF token，造成后续状态断言连带失败。修正测试的来源分类、异步进程等待及原生语义断言后，同一发行包通过，无产品代码扩围或重新封包。

同一最终ZIP解包后的验收结果如下，共185项；原生118项另列，不混算：

| 验证 | 结果 | 证据 |
|---|---|---|
| Radar目录、同步与恢复 | 23/23 | Radar专项 |
| 只读模型目录、统计、诊断 | 20/20 | 只读回归 |
| 订阅登录、生命周期、别名和账号池 | 18/18 | 订阅回归 |
| 054→055实际包升级、旧包回滚、未知版本拒绝 | 4/4 | 升级证据 |
| 三协议、凭据、节流、账号范围、迁移和窄连接器集中回归 | 120/120 | 集中回归 |

升级保留原生Radar表的日期、账号范围和用量；未知发行对继续拒绝。20次完整本地HTTP mock benchmark：p50 62.85ms、p95 75.90ms；前轮054为76.74/94.26ms。这是本机样本，未见额外延迟，不证明上游耗时或生产性能改善。

最终候选`fusion-v3.8.52-isolated-055.1-4d0b0a9c19c8`，ZIP为396,008,558 bytes；SHA256 `54e504ab096ee6e8aa7e6f7328a3b7d7d92b7a490303dda08525f26b5b8d32a6`。见发行回执。审计19个本轮实例：测试凭据0、存活进程0、随包工具与源码不一致0，见审计。

## 维护范围

上游已跟踪运行时文件累计20个，+160/-22；054为19个、+145/-21。本轮新增的上游文件改动点仅Radar settings的严格输入限制，另扩展既有proxy白名单。自有启动模块调用原生scheduler，外围工具增加公开feed配置映射与054→055精确升级对；不新增名单、SQL schema、Router或同步实现。文件/行数不包含自有模块、外围工具及测试，不能等同整个fork维护成本。

后台构建成功；继续沿用既有backend-only构建及跳过全量类型检查的限制，不能宣称全量上游类型检查通过。许可证检查348包、0违规，7个既有缺少bundled license提示，无新增依赖。Rust/桌面端没有改动或重编。

## 来源与限制

Primary仅为上述固定OmniRoute源码；相关路径继承根MIT，随包保留上游版权和许可证，更新THIRD_PARTY_NOTICES。没有新增依赖或另选名单服务。

本次为固定底座的054→055适配升级，不证明未来OmniRoute升级兼容；商汤、CommandCode和WorkBuddy外部账号事项继续延期。操作见[fusion说明](../../fusion/README.md)。

## 结论

适合继续直接复用。目录、日期、过期检查、签名缓存与后台调度均由OmniRoute承担，新增产品代码集中在管理入口、社区凭证边界和启动接线。它是带来源时间的发现目录，不能保证每个免费平台实时可用；本轮未验证真实托管feed。无需为这部分另造名单或更新服务，也不因此开启UI或其他上游功能。

## 清理与保留

验收后删除17处临时材料，包括054旧ZIP、两版测试展开包、055组装目录、测试实例、focused数据和本轮构建缓存。文件大小累计10.17 GB，其中D盘7.80 GB、C盘临时目录2.36 GB；不将其等同于NTFS空闲空间变化。见清理回执。

`D:\Loadout`按文件ID去重约6.71 GB；唯一候选为上述055 ZIP（396.0 MB），展开包目录0。源码、未提交修改、失败及通过证据均保留。正式桌面端、正式数据和凭据未改，无提交上传。见占用核对；历史receipt中的临时路径保留用于溯源，文件已清除。
