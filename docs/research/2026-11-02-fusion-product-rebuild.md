# 067 Fusion 产品化重做：记录

对应 [ExecPlan 067](../exec-plans/completed/067-fusion-product-rebuild.md)。本文只记录这次核实过的事实和取舍，不重复计划内容。

## 产品形态

- 入口：仓库根目录 `Loadout.lnk` → `dist/fusion/app/Loadout.exe --instance development/fusion/isolated/loadout-fusion-isolated/main`。
- 窗口启动后先显示启动页，后台就绪后进入控制台。没有登录页。
- 实例 `main` 由隔离实例 059 经加密 backup/upgrade 复制而来，059 原样保留。地址 `http://127.0.0.1:8318`，`network: direct`。
- 正式安装、8317、正式数据和凭据没有被读取或改动。

## 免密是怎么做的

管理 API 仍然要会话，因为 `/api/*` 暴露在 loopback 上，无鉴权会让其他本机用户可管理。窗口启动时，`node.exe tools/desktop.mjs prepare-session` 用系统凭据库里的机器密钥换一个会话，经私有管道交给壳，壳用 `window.set_cookie` 以 HttpOnly 写入 WebView 后再跳转。cookie 不进文件、命令行和页面脚本。

`/v1` 仍然要客户端密钥。浏览器直接打开地址没有会话，界面会提示从应用打开。

## 前端结构

用户对 066 的否定是“只保留了内核和设置”。这次直接复用原版页面（`apps/desktop/src` 的 fork），把 `invoke` 换成 `POST /api/loadout/rpc`，方法名和原 IPC 一致，所以账号拖动排序、模型分组、号池、总 API、一键导出没有被重写一遍。

用结构做引导的地方：

| 页面 | 结构 | 引导什么 |
|---|---|---|
| 导航 | 账号、模型、路由、客户端穿在一条线上，选中项并入页面 | 配置的顺序就是一个请求经过的路径 |
| 总览 | 左上游、右下游的实时流向图 + 接入地址 + 最近请求；有问题才出现“需要留意” | 请求从哪来、到哪去；图两端空着时，上面的按钮就是下一步 |
| 账号 | 添加时先选服务商方块，订阅、平台、中转、本机分组 | 不用先知道 Base URL |
| 模型 | 左分组、右目录，把目录里的行拖到分组；比价、价格表、免费模型同页切换 | 分组就是一个给客户端用的模型名 |
| 路由 | 客户端 → Loadout → 候选账号的路由图；健康、重试与冷却同页切换 | 这个名字现在实际走谁、失败走谁 |
| 客户端 | 密钥表 + 一个策略对话框，模型、账号、日周月上限都在里面 | 限制属于客户端，不另开页面 |

解释性文字都在使用说明里，每页标题旁的问号直达对应章节。

Magpie 的设计语言来自路由图和丝带流向，全部为原创实现，没有复制其代码。OmniRoute 的做法只用在免费模型的目录网格。

## 实时流向

`stats.live` 返回两部分：内存里进行中的请求（`getPendingById`），和 `call_logs` 里 rowid 大于游标的新记录。第一次调用只拿游标，打开页面不会回放旧流量。只有元数据，不含提示词和回复，回归里有检查。

进行中的条目本来不带客户端名和字节数，所以加了两处补丁：`chatCore` 把 `apiKeyName` 写进 pending 元数据；`streamingTail` 在发给客户端的流上计数，写入 `streamedBytes`。

前端有请求时每 400 ms 取一次，空闲时每秒一次，窗口不可见时不取。光点的密度按两次取数之间的字节速度换算；短到两次取数之间就结束的请求，事后按新记录补一串。失败的光点停在中间。没有光点也没有进行中的请求时，动画循环完全停止。关掉“界面动效”或系统关闭动画效果时只亮节点。

## 语义上踩到的坑

- **指定账号是偏好，不是硬限制**（ADR-003 §8）。`provider/account/model` 指名的账号不在客户端密钥范围内时，由范围内的账号提供同一模型，不算越权。我一度把它改成拒绝，被 `sync-does-not-expand-key-account-scope` 拦下后改回。
- **子号池是硬范围**：指名子号池之外的账号直接 400，否则“只在部分账号中路由”这句话不成立。
- **每分钟上限**：原生 `PATCH /api/keys/:id` 的 schema 不带 `maxRequestsPerMinute`，字段被静默丢弃，界面看起来保存成功、实际没存。改为经 `updateApiKeyPermissions` 写入，并测试了确实返回 429。迁移来的 `legacy-client-2` 的 8 次/分钟存在 `rate_limits` 规则列表里，执行时规则列表优先于那一列，所以读写都要先看规则列表，否则这条限制在界面上既看不到也改不了。
- **流式分块数不能单独当判断依据**：短回复 121 字符是 3 个分块、11 ms 内到齐，上游就是一次写完的。判断有没有被缓冲要看长回复的时间分布。
- **`NextResponse.rewrite` 会重新进入 proxy**，总 API 的 `/e/<name>/v1/*` 因此被判成 `DISTRIBUTION_ROUTE_DISABLED`。改用 `next.config` rewrites，鉴权按 `/v1` 判断。
- **不要中断 Next 构建**：构建期间上游 257 个文件被临时 stub，中断后不会恢复。这次恢复时按 `git show HEAD:path` 逐个核对后写回。
- **`fusionSafety` 钉住的设置改了也不生效**：`requestRetry`、`maxRetryIntervalSec` 恒为 0，`PATCH /api/resilience` 仍返回 200。所以直接请求永远不等待，能开关的只有分组的 `comboCooldownWait`，且 `maxWaitMs` 的写入上限是 30000。
- **原生预算只有一个生效窗口**：`getActiveBudgetLimit` 按 `resetInterval` 选日、周或月。三个数都填，只有一个起作用。密钥自身的 `dailyUsageLimitUsd`、`weeklyUsageLimitUsd` 是另一套，按 UTC-3 的日界线和滚动 7 天计算。
- **请求记录有三条删除路径，保留期不统一**：启动时的 compliance 和定期 rotation 都是环境变量优先（默认 7 天），每 6 小时的 `cleanupCallLogs` 只读 `retention.callLogs`（默认 30 天）。只设 `CALL_LOG_RETENTION_DAYS` 时，第三条路径照样删 30 天前的记录，而且没有任何报错。要保留更久，这三处都要核对，并等到定时清理真的跑过（启动后 30 秒）再下结论。

## 出站

空闲时不应连接任何上游。`isolation-guard` 在 direct 模式下仍拒绝 registry.npmjs.org、api.github.com 和 ipify 系列，并把首次出现的主机写入实例的 `egress.log`。这几个主机正是第一次运行时日志里发现的 OmniRoute 自带请求。加入拒绝名单后，最终包启动并空闲 45 秒，`egress.log` 没有新增主机；有请求时只出现 `www.workbuddy.ai`。

## 已知缺口

- ChatGPT 网页会话未移植，正式版仍可用。
- 用量读取只覆盖 OmniRoute 有 fetcher 的 Provider，其余按 Loadout 自己的请求记录估算，界面上有标注。
- 虚拟模型的档位由测速结果生成固定顺序，不是原版的内存快照。
- WorkBuddy 上游的渠道校验可能再变，relay 只在 `CHANNEL_REJECTED` 时冷却。
- 商汤曾有风控 429，本次验收的真实请求只走 WorkBuddy 和 DeepSeek，没有验证商汤的真实链路。

## 验证

结果记录在 `development/fusion/evidence/067-product/`，数字见 ExecPlan 067 的 Progress。
