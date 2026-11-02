# OmniRoute 第一阶段隔离验证

日期：2026-10-30（Australia/Sydney；原始结果使用 UTC）。对应 ExecPlan 039、ADR-019 Proposed。

**结论：主要后台、凭证持久化和账号行为已证明可通过小补丁实现；“融合后长期少维护”尚未得到证明。正式环境保留 Loadout，不进入 UI 或生产迁移。OmniRoute 保留为候选，后续应先验证完整运行包与升级成本。**

本轮没有再造协议转换器或第二套路由器。实验新增代码负责编译约束和保护凭证，账号筛选、健康处理、重试和发送仍由 OmniRoute 执行。

## 已运行的证据

| 检查 | 结果 | 证据边界 |
|---|---|---|
| 官方 v3.8.52 发布包独立启动 | `/healthz` 200；headless `/dashboard` 404；只监听随机 loopback 端口；结束后进程停止 | 使用官方 npm 包的 `dist/server.js`，没有加载私人补丁。这是原生后台启动证据，不是融合包验收 |
| 适配后的真实 HTTP 路由链 | 最终 18/18 个实验检查通过 | 原版 `POST` route、`handleChat`、账号选择器、executor，通过薄 HTTP 测试宿主调用；不是自行模拟 picker，也不是完整 Next 打包运行 |
| Windows Credential Manager | 真实写入/读取、清空缓存后恢复、重启后推理成功；最后删除 7 条测试凭证，剩余 0 | 只访问随机 `loadout-test:omniroute-phase1-*` service；不可用测试是在适配边界注入故障，没有禁用系统凭证服务 |
| 上游 focused 回归 | 62/62，通过 5 个测试文件 | 实验开关关闭时检查上游原行为；不代表完整上游测试、类型检查或发布构建通过 |
| 持久化/日志哨兵 | 最终扫描未命中假 Provider secret、client key、正文哨兵 | 覆盖本轮生成的数据、SQLite/WAL、备份和日志；不是所有 Provider、导出或故障路径的全面审计 |
| 重放及版本接缝 | v3.8.52 补丁重放检查通过；向 v3.8.51 反向试套有 4 个文件冲突或不存在 | 只做临时 Git index 上的 patch 检查；没有声称完成 v3.8.51→v3.8.52 的编译、升级或运行回归 |

可审查结果：最终实验、原始基线、原生后台、62项回归、补丁与版本记录。

固定源码：OmniRoute `ad6f33f66bda34eba98645d3bfdabbde294bc843` / v3.8.52。实验分支 `codex/loadout-phase1-20261030`，位于 `development/upstreams/omniroute/.claude/worktrees/loadout-phase1/`；原始参考 checkout 保持干净。Loadout 正式源码没有因本实验修改；既有未提交改动保留。

## 哪些原生支持，哪些需要适配

| 功能 | 原生实测 | 本轮处理 |
|---|---|---|
| 优先账号、有序换号 | a 优先；a/b 429 后能到 c；模型始终相同 | 复用原执行链。观察到 a、b 各重试 3 次，合计 7 次上游尝试；尚未验收跨协议变体的账号预算 |
| 逐请求轮转 | 配置 `round-robin`、批次 1、禁用 affinity 后得到 a,b,c,a | 配置即可；并发分配、持久粘性与恢复行为未全面覆盖 |
| 子集只有 c | 所有实际请求命中 c | 用 client key 的 `allowedConnections` 表达范围，不复刻原 URL/CLI |
| 明确空子集 | 原生 `[]` 表示不限制，实测仍发送到 b | 编译为不匹配任何账号的 sentinel；适配后 403、零上游请求。这个编译必须覆盖未来控制面的所有写入入口 |
| `provider/account/model` | 原生强制账号语义不会换号 | 编译地址，增加可选 `preferredConnectionId`，在原有授权/健康筛选之后优先选账号；b 失败后同模型到 a |
| 禁止 account failover | 需要区别“首选”与“固定” | 把首选账号同时收窄为唯一可用范围；越过 key 子集的 strict pin 返回 403、零上游请求 |
| Provider secret | 默认无加密配置时写入明文 | 复用上游已声明的 keytar 7.9.0，先串行写 Windows 凭证库，DB 字段只写 `loadout-ref:v1:*`；请求侧读取已预热的内存缓存 |
| client key | 原生同时保存原文/可逆值和 hash | 保留原 hash 鉴权，持久化值改为不可还原的 hash marker；实验禁用 reveal 的可逆语义 |
| token 与嵌套 secret | 字符串和字符串数组可通过集中映射处理 | 实测 access/refresh token、console key、extra key 数组；未支持的结构化 cookie 对象直接拒绝，不能默默以明文保存 |
| SSE 提前 keepalive | 约 1 秒先给客户端字节，随后还会重试并换号 | 实验入口关闭提前 keepalive，保留原执行器；同一反例修正后，所有重试均在首字节之前完成 |

SSE 反例见 保留的失败结果：客户端收到首字节时只有 1 次上游尝试，结束时已有 4 次（a,a,a,b）。修正后为首字节前 4 次、之后无新增。它只验证这条反例，不能替代正常流式工具调用、半途断流、取消、stateful Responses 的矩阵。

## 适配面与维护判断

冻结补丁 修改 **7 个现有运行时文件，24 行新增、6 行删除；新增 2 个模块，共 131 行**。合计 155 行新增、6 行删除，不包括测试宿主、复现脚本和文档。

| 接缝 | 文件 | 私人责任 |
|---|---|---|
| 凭证写入/读取 | `db/providers.ts`、`db/encryption.ts`、`db/apiKeys/secretAtRest.ts` | 异步 OS 写入必须在同步 SQLite transaction 外完成；漏掉的写入必须拒绝，不得回退明文 |
| 账号约束 | Chat route、`sse/handlers/chat.ts`、`sse/services/auth.ts`、`credentialSelectionOptions.ts` | 约束随 request admission 重建传递；首选账号不能越过现有候选集；固定账号不能放开范围 |
| 新增模块 | `loadoutPhase1/secretStore.ts`、`loadoutPhase1/policy.ts` | OS store/cache、地址和范围编译；没有独立健康状态或第二个 failover loop |

**代码量小，但不是稳定插件 API。** 相邻旧标签上，Chat route、Chat handler 两处 patch 冲突，`secretAtRest.ts` 和 `credentialSelectionOptions.ts` 两个目标文件不存在。这说明接缝存在版本布局变化；反向试套失败不等于未来每次升级都会失败，也不能算实际升级演练。

本轮还发现两个容易低估的成本：

1. **请求热路径仍有数据库依赖。** 最终探针使用 AsyncLocalStorage 区分请求、Undici dispatch 事件标记实际发送，关闭 `OMNIROUTE_DISABLE_CONVERSATION_TRACKING=1` 后，热请求发送前聚合查询为 0，但仍执行约 **197 条普通 SQL**。这是“发送前窗口”，不是纯 router 时延；不能因此宣布满足 Loadout 的 `RouteSnapshot` 只读内存规则。早期探针误把发送后的 gamification SQL 算入窗口，已经纠正，早期数量不作为结论。
2. **原生后台与适配源码尚未合成一个发布包。** 源码 Next dev 尝试经过依赖补全和安全缓存处理后停在 instrumentation 编译，未完成端到端启动；官方预编译包随后启动成功。薄宿主完成了适配链路验证，但不能替代带补丁的 production build、管理鉴权及安全启动闭环。

## 安全、性能与未覆盖项

实验使用 Node **24.19.0**，全部假账号，仅 loopback mock 推理。子进程 home、APPDATA、LOCALAPPDATA、DATA_DIR、临时目录与环境变量隔离；未读取正式 Credential Manager 条目，未改正式端口、注册表、自启、客户端配置或运行中的 Loadout。

JavaScript fetch/TCP guard 拦截了 4 个后台外联目标：npm 更新查询、GitHub API、两个公网 IP 查询服务。它们没有成功发出推理流量；运行期出站限制和安装期合法下载是分开的。这个 guard 是测试防护，不能冒充操作系统沙箱或已交付的生产网络策略。

20 次串行 mock HTTP 的 p50：原始基线约 **349.7 ms**，最终适配约 **349.9 ms**；p95 分别约 351.8 / 353.3 ms。两个测量前的故障场景不同，样本很小且包含上游节流/冷却行为，只能说明未发现数量级恶化，不能证明适配零开销或满足正式性能 SLO。

以下仍未覆盖或未解决：

- 全部 `/v1/*`、Responses 有状态固定路由、工具 SSE、取消、跨协议失败预算、并发轮转；本轮只执行 Chat Completions 的主要账号场景。
- 所有 OAuth 刷新、cookie 格式、导入/导出、管理 API、备份恢复、直接 DB writer 的安全闭环。未注册字段、结构化凭证和绕过标准 CRUD 的路径不能假定自动安全。
- OS store 写入成功而 DB 写入失败时的孤立凭证回收；正式删除/轮换后的缓存失效与撤销；测试 service 的 finally 清理不等于产品 GC。
- Node 字符串不能继承 Loadout Rust secret 类型的可靠清零保证，仍须评审 ADR-004；本轮仅证明持久化底线和所测错误路径。
- 账号集合/主子集的持久管理入口、完整进程崩溃恢复、自启、Windows 发布包升级回滚。
- 完整依赖 license/security 审计、上游全量测试和带补丁的生产构建。62 项 focused tests 不替代这些门槛。

因此，本轮建议是：**保留当前正式 Loadout；继续保留这份 OmniRoute 小补丁作为候选验证成果。不要把“18 个实验检查通过”升级为“可以迁移”。下一项投入应是把这两个模块接入一个真实独立发布包，并完成一次实际跨版本前向重放和运行回归；在此前不开始 UI 融合。** 如果为消除热路径 DB/安全旁路必须大改公共执行链，重新选底座，不以现有投入为理由扩展私人 fork。

复现入口和清理边界见 实验 README。正式环境没有切换；本次进程均已停止、测试凭证归零；源码参考和实验依赖按用户要求保留。

