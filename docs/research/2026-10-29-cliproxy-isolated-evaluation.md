# CLIProxyAPI 隔离替换验证（2026-10-29）

## 决策

**本轮结论：保留 Loadout 当前协议和接入主链路，不启动整体迁移到 CLIProxyAPI。**

CPA 可以作为受控的执行组件接在 Loadout 后面，已用实际 Gateway 串联证明部分可行；但当前没有证明能在保留安全与账号规则的同时，删除足够多的 Loadout 代码来抵消新增的 Go 运行时、适配边界和升级维护成本。因此，不建议现在将它设为默认接入引擎，也不建议深度 fork。

这会收紧此前“考虑保留小核心、复用成熟引擎”的建议：**方向值得验证，不应据此直接批准迁移。实验结果支持继续参考和择项复用，尚不支持整体替换。**

## 版本、隔离与证据

- Loadout：工作区 HEAD `740c7d8`，版本 `0.4.0`；保留任务开始前已有未提交修改。生产 crate、依赖清单、配置和安装目录未修改。
- CPA：[固定 commit](https://github.com/router-for-me/CLIProxyAPI/tree/90c447d3a8727d115aa7552ec93ce023736ed492)，module v8，Go 1.26.10。根 LICENSE 为 MIT；sdk/internal 检查范围内未找到嵌套许可证。原始实现只在临时 checkout，notice 已记录。
- 全部上游为随机端口 loopback mock；凭证为内存随机标记或既有测试假 key。不读取真实账号、Credential Manager、正式数据库，不启动/停止正式 daemon，不调用真实 Provider、不消耗额度。
- Go 执行路径禁用代理并拒绝非 loopback HTTP；测试进程的 HOME/APPDATA 等指向本次临时目录。它是实验路径的出站约束，不是操作系统级沙箱。
- 实验源代码与重跑说明、结果清单、Go 原始 JSONL、实际 Gateway 串联日志。

## 实测结果

| 验证层 | 结果 | 能支持的判断 |
|---|---|---|
| 原有 Loadout Gateway 基线 | `listen` 2 + `proxy` 31，全通过 | 当前契约在本机 mock 下成立；不是 CPA 集成结果 |
| CPA 组件检查 | 55 个叶级检查：46 pass、9 fail | 存在明确的直接替换缺口；包含诊断断言，不计算“兼容率” |
| 外部 Go module | public translator 可编译运行；直接 import internal executor 被编译器拒绝 | 公开转换 SDK 与内置执行器的复用边界不同 |
| 实际 Loadout → CPA → mock | 14 pass、0 fail、17 ignored | Chat Transport 适配能保留所选路由/流契约；**Loadout 协议转换仍在** |
| 安全与清理 | 两次运行目录扫描均0文件；合成标记未出现在 Info 日志；bridge exit=0 | 本实验未观察到落盘泄漏，实验子进程正常关闭 |
| 静态检查 | `go vet` 通过 | 实验代码基础检查通过，不等于生产安全验收 |

### 协议转换：有可用能力，不能作为无差别转换器

三种客户端格式（Chat、Responses、Messages）通过 CPA OpenAI-compatible executor 接到 Chat mock，流式/非流式6项、流式工具输出3项均通过。原生格式未知字段保留3项通过。

直接公共转换矩阵缺少 `openai → openai-response` 和 `claude → openai-response` 两条请求边；对应工具历史往返也失败。SDK 在没有注册转换器时会返回近似原始 body，而不是自动拒绝，所以调用方必须检查转换边并验证格式。[Registry 源码](https://github.com/router-for-me/CLIProxyAPI/blob/90c447d3a8727d115aa7552ec93ce023736ed492/sdk/translator/registry.go)

**这不是“CPA 不支持 Responses”。** 补测三种格式转 `codex` 均能得到 Responses 形状的 input。但该路径服务于 Codex：实测强制 `stream=true`、`store=false`，删除 `max_output_tokens` 和 `temperature`。不能直接用它替代任意厂商的原生 Responses 透传。[Codex 请求正规化](https://github.com/router-for-me/CLIProxyAPI/blob/90c447d3a8727d115aa7552ec93ce023736ed492/internal/translator/codex/openai/responses/codex_openai-responses_request.go)

有状态 Responses 转 Chat，使用带 Err 的新版 `TranslateRequestEnvelope` 仍得到无 previous_response_id 的非空请求且没有拒绝。Loadout 必须继续在入口按 ADR-006 判定原生能力并固定路由。该失败表明底层 translator 不替我们执行此策略，不证明 CPA 完整 HTTP 服务的所有状态路径都错误。

### 凭证：可内存注入，但安全边界不能直接交接

单次执行器允许直接提供内存 Auth，不需要把 key 写配置文件。本次未观察到随机 secret/正文标记落盘或进入 Info 日志。

两项直接替换安全断言失败：

1. mock 上游在400错误正文中回显合成 key/正文标记，执行器的 `err.Error()` 保留这些内容。
2. `fmt.Sprintf("%+v", Auth)` 能显示 `Attributes` 中的合成 key，未提供 Loadout Secret 类型的默认脱敏语义。

这属于**接口向调用者暴露敏感值**，不是本次发现了正式日志泄漏。若复用，Loadout 的 Credential Manager、secret_ref、脱敏错误、日志白名单仍须保留，适配器也必须强制执行，不能只配置“关闭 debug”。[Auth 类型](https://github.com/router-for-me/CLIProxyAPI/blob/90c447d3a8727d115aa7552ec93ce023736ed492/sdk/cliproxy/auth/types.go)

持久化测试还确认：Register 使用 `WithSkipPersist` 不触发 Save，后续普通 Update 会触发 Save。这个标记只作用于当前操作。无持久化要求应由自定义内存 Store 统一保证；本次用的是计数 Store，未写真实文件。

### 账号规则：保留 Loadout 决策可以做到

CPA manager 注册两个假账号，设置零重试、单凭证尝试并 pin 到 B：成功请求仅访问 B；B 返回429也不访问 A；pin 不存在的账号则拒绝且零上游调用，模型名保持指定值。直接 executor 的429测试也确认只有一次指定账号调用。

实际串联使用每请求单账号 executor，**不启动 CPA 第二套账号池**。复制既有 Gateway 测试到临时 Cargo 项目，仅修改 mock 地址并标记未覆盖测试，14项原断言通过，包括：

- 429后同模型切账号、下一次跳过冷却账号、同 key 的跨协议冷却；
- exact账号额度耗尽后的同厂商切换、显式禁止 failover；
- 专用端点的多账号切换和按请求轮转；
- SenseNova 套餐无权限的原有分类与健康状态；
- 有状态请求在缺少原生 Responses 上游时拒绝；
- Chat/Responses/Messages 客户端经 Chat 上游返回、usage与未知字段；
- 首内容提交后断流，保留 partial、输出错误终止并记录 upstream_interrupted。

它证明 Loadout 的选路权可以保留，**不证明移除 Loadout Router/Provider 错误分类也会保留这些行为**。多协议账号尝试计数、原生 Messages/Responses、状态链持续路由等另外17项未通过本桥接运行；其中已有基线通过，不能算迁移后通过。

### 流生命周期：执行器不是完整 Gateway

直接 executor 在空 Chat 流或输出后无 finish_reason/DONE 的正常 EOF 时，没有返回明确错误，两项断言失败。进一步检查发现，CPA 的公开 HTTP handler 自己检查 finish_reason；这不是完整产品完全没有保护，而是直接复用底层执行器时仍需外围处理。[HTTP handler](https://github.com/router-for-me/CLIProxyAPI/blob/90c447d3a8727d115aa7552ec93ce023736ed492/sdk/api/handlers/openai/openai_handlers.go)

串联适配器必须重新封装 SSE、补正常终止标记，并区分“上游错误事件”和“已输出后的连接中断”。最终将后者传递为传输中断，由 Loadout 生成客户端终止事件和遥测分类，原测试才通过。最初的8项失败来自实验桥接的帧封装错误；第二轮剩3项涉及未实现的 Messages 桥接和中断映射。日志保留，**不计为 CPA 产品缺陷，也不隐去适配成本**。取消测试确认 context 取消能到达 mock 上游。

## 性能观察

本机 loopback、预热20次、采样100次、Windows QPC，进程内 CPA 执行器、Claude↔Chat 转换：

| 指标（毫秒） | 直接 mock P50 / P95 | CPA adapter P50 / P95 |
|---|---:|---:|
| 非流式完成 | 0.087 / 0.108 | 0.170 / 0.218 |
| 首内容 TTFT | 0.082 / 0.095 | 0.170 / 0.211 |

这里只能说明小型 fixture 的进程内转换开销没有出现数量级问题。没有测 Rust↔Go IPC 的分位数、并发背压、大上下文、进程重启/崩溃、TLS或真实厂商；不能拿这些数据批准 Harness 的生产性能预算。最初 `time.Now()` 采样在本机呈0/1ms量化，已改QPC，未使用其结果得出性能结论。

## 为什么当前保留更合适

| 方案 | 当前判断 | 依据 |
|---|---|---|
| 用 stock CPA 接管本地网关 | 不采用 | 完整 Service 的凭证/路由/生命周期要重新配置与验证；本轮没有证明与 Loadout 契约等价 |
| Loadout 管账号，CPA 负责全部协议与接入 | 暂不迁移 | 原生 Responses和状态策略、安全包装、错误分类、流状态仍需保留；独立执行器没有可直接跨module导入的公共工厂 |
| 为已明确需要的新增接入建立受控 CPA adapter | 可保留为候选 | 单账号执行与14项串联可行；需证明新增覆盖带来的收益，不能仅为已有Chat能力新增一个进程 |
| 保留主链路，参考或择项复用上游转换能力 | 本轮选择 | 无迁移风险；已建立可重跑的契约验证，后续可按具体功能计算替换收益 |

`internal` 并不代表只能大规模 fork：本实验只在固定版本 checkout 添加原创入口，没有修改上游实现。代价是维护一个受版本影响的构建覆盖层。使用公开 Service builder 是另一条路，但会带入更多生命周期、配置和管理组件；本轮没有运行它，不能断言一定做不到。CPA 的 MIT 许可证允许评估所用方式，许可证不是这次保留结论的原因。

最关键的经济性判断是：**本次可工作的路径主要增加了一层，尚未证明可以删除哪一整块现有实现。** 没有独立测量维护工时，不能给出节省百分比或虚构盈亏天数。既有投入是沉没成本；保留的依据是未来仍需承担相同策略责任，同时新增适配与发布成本，而本轮未验证出新的生产接入收益。

## 重新考虑迁移的门槛

只有出现具体的新 Provider/协议需求，且预计自行维护明显更贵，才值得重开窄范围试点。届时至少需要：

1. 明确可删除的 Loadout模块和上游长期维护边界；优先稳定公开接口，避免绑定大面积 internal 实现。
2. 隔离凭证通道、内存 Store、错误与日志脱敏，覆盖异常/刷新/关闭路径；不只验证正常请求。
3. 原生 Responses及状态链、所有账号/模型/failover语义通过实际集成，不能以SDK组件通过替代。
4. IPC取消、背压、崩溃重启、安装升级回滚、并发性能达到现有要求。

本轮没有改正式环境、没有创建迁移分支或推送。保留临时源码与编译缓存供复查；测试运行目录已删除，桥接子进程已退出。此次改动仅为实验、证据、参考记录与执行计划。
