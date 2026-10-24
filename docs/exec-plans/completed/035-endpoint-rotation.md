# ExecPlan 035: Dedicated Endpoint Rotation (Round-Robin Across Member Accounts)

## Goal
项目所有者要求：使用商汤总 API（专用端点 `/e/sensenova/v1`）时，**每个请求都从下一个账号开始**，不管当前账号是否被限流，把流量平摊到每个账号，降低单个账号因流量集中触发风控的风险。只用于商汤账号池，统一入口、Pool 和虚拟模型不变。

## Context
- ADR-015 §3：端点候选按账号顺序（粘性），只在出错时换账号；Alternatives 里已把"轮询分摊"列为端点的后续选项。
- ADR-003 §5：有序粘性链，"不做负载均衡"。本计划在 ADR-015 新增 §5，并在 ADR-003 §5 注明例外。
- ExecPlan 034 之后，端点内限流按 key 共享到各协议、每个账号只试一次。
- 2026-10-23 的导出：商汤请求几乎全部落在第一个账号（2663 次成功，其余账号只作为溢出），26% 的输入 token 为上游缓存命中。
- 本机核对（2026-10-24）：商汤网关连通正常，24 条路由测速成功，未见封 IP。

## Existing behavior
- `RouteTable::resolve_endpoint` 返回成员账号上的同一模型，按表顺序（账号顺序）；点名账号时该账号排第一。
- `select()` 对 Endpoint：每账号一个 target，尝试次数 = 成员数；会话粘性（`X-Loadout-Session-ID`）把上次的账号模型提到最前。
- `routes::endpoint_proxy` 循环调用 `proxy::handle_bytes`（等待恢复时多轮）。

## Constraints
- Capability-first（Rule 3）：开关属于端点，不在代码里判断 provider。
- 热路径（Rule 1）：轮次是内存计数器（`Mutex<HashMap>`，与 `AccountSlots` 同类），无网络 / 数据库访问。
- Schema 变更只走 migration 并带测试（Rule 8）。
- 不偷偷换模型（Rule 7）：轮换只换账号，模型不变。
- 修改 ADR-003 §5 的规则前先修订 ADR（Rule 10）。

## Architecture impact
- Storage：migration 0015，`endpoints.rotate`。
- Router：`EndpointIndex.rotate`、`with_endpoint(…, rotate)`、`endpoint_rotates()`；`resolve_endpoint(endpoint, model, turn)` 按轮次旋转成员账号顺序。
- Gateway：`DataPlane.endpoint_turns`（`EndpointTurns`），`endpoint_proxy` 每个客户端请求取一次轮次放入 `Scope.turn`；轮换时不使用会话粘性。
- Core / IPC：`EndpointInfo.rotate`、`EndpointSaveParams.rotate`（省略 = 保持原值，新端点为关闭）。
- CLI：`loadout endpoint rotate <name> on|off`；`list` 显示轮换状态。
- Desktop：总 API 对话框新增开关。

## Files expected to change
- `docs/adr/ADR-015-dedicated-endpoints.md`、`docs/adr/ADR-003-snapshot-routing.md`
- `crates/storage/migrations/0015_endpoint_rotation/{up,down}.sql`、`crates/storage/src/{migrations,endpoints}.rs`
- `crates/router/src/{table,select}.rs`
- `crates/gateway/src/{plane,routes}.rs`、`crates/gateway/src/proxy/mod.rs`、`crates/gateway/tests/proxy.rs`
- `crates/core/src/{endpoints,route_build,dispatcher,daemon}.rs`
- `crates/ipc/src/api.rs`
- `apps/loadout/src/bin/loadout/endpoint_cmd.rs`
- `apps/desktop/src/pages/Endpoints.tsx`、`apps/desktop/src/i18n.tsx`
- `docs/providers/sensenova.md`、`AGENTS.md`（命令速查）

## Implementation stages
1. ADR-015 §5 + ADR-003 §5 注明 + 本计划。
2. Storage：migration 0015 + repo + 测试。
3. Router：端点索引与轮换解析 + 单元测试。
4. Gateway：轮次计数器、`Scope.turn`、轮换时不用会话粘性 + 集成测试。
5. Core / IPC / CLI：保存与显示 `rotate`。
6. Desktop：开关与文案；`npm run build`。
7. fmt / clippy / workspace test / benchmark（基线 `pre035`）；文档。

## Tests
- Storage：`rotate` 保存 / 替换 / 读回；migration 往返（已有通用测试）。
- Router：轮次 0、1、2 依次以不同账号开头；点名账号不轮换；未开启的端点忽略轮次。
- Gateway 集成：开启轮换的端点，3 个账号连续 4 个请求依次由 a、b、c、a 服务；某账号被限流时由下一个接手，下一个请求继续轮到再下一个；未开启时一直由第一个账号服务；会话头不影响轮换。
- Core：`save_endpoint` 省略 `rotate` 时保持原值。

## Performance validation
`cargo bench -p loadout-benchmarks --bench route_selection --bench stream_proxy -- --baseline pre035`（基线在改代码前保存）。未开启轮换的路径只多一次布尔判断。

## Security considerations
不涉及凭证与请求内容。轮次只在内存中，不记录。

## Migration considerations
0015 `ALTER TABLE endpoints ADD COLUMN rotate INTEGER NOT NULL DEFAULT 0 CHECK (rotate IN (0, 1))`；down 删除该列。已有端点保持关闭，需要用户开启。

## Known risks
- 上游 prompt cache 命中率下降：同一会话的连续请求落在不同 key 上。导出中命中与否首字延迟差别很小，但如果缓存 token 的积分更便宜，积分消耗会变多。
- 轮换只平摊每个 key 的请求量，**不减少同一 IP 的总请求量**；如果上游按 IP 风控，这个开关帮不上忙。用一个 IP 轮流使用多个账号本身也可能被视为异常。
- 轮次计数器在网关重启后归零；多个客户端并发时按到达顺序轮换，不保证某个客户端自己的请求严格交替。

## Definition of done
开关可在 CLI 与桌面端开启 / 关闭并持久化；开启后请求按账号轮换；测试、fmt、clippy、benchmark 通过；ADR、provider 文档、命令速查已更新；计划移至 `completed/`。

## Progress
- [x] 2026-10-24 调研：ADR-015、端点存储 / IPC / CLI / 路由 / 网关 / 桌面代码；导出中的缓存命中率。
- [x] 基线：`--save-baseline pre035`（改代码前启动）。
- [x] Stage 1：ADR-015 §5、ADR-003 §5 例外说明。
- [x] Stage 2：migration 0015（`rotate`，CHECK 0/1，down 删除列）；`EndpointRow.rotate`；repo 测试覆盖保存 / 替换；migration 往返测试通过。
- [x] Stage 3：`EndpointIndex.rotate`、`with_endpoint(…, rotate)`、`endpoint_rotates()`；`resolve_endpoint(…, turn)` 用 `start_at_account` 按账号旋转（点名账号时不旋转）。测试：轮次 0/1/2 依次 main、backup、main；未开启时忽略轮次；点名账号排第一。
- [x] Stage 4：`EndpointTurns`（`Mutex<HashMap<String, usize>>`，`DataPlane.endpoint_turns`）；`endpoint_proxy` 每个客户端请求取一次轮次放入 `Scope.turn`；轮换时不读会话粘性。集成测试：会话头存在时依次 a、b、c、a；b 被限流时 c 接手，下一轮仍是 c（b 被跳过），再下一轮 a。
- [x] Stage 5：IPC `EndpointInfo.rotate`（`serde(default)`）、`EndpointSaveParams.rotate`（省略 = 保持）；core 保存 / 显示 / 路由表构建；dispatcher 测试：新端点关闭、开启后省略参数仍保持、显式关闭。CLI `loadout endpoint rotate <name> on|off`（其余设置原样保留），`list` 显示轮换状态；`disable` 也改为保留其余设置。
- [x] Stage 6：总 API 对话框新增"每次请求轮换账号"开关与说明（中英文）；轮换开启时隐藏"第 n 位"序号。`npm run build` 通过。界面用临时预览页（vite dev server + 注入的模拟 `__TAURI_INTERNALS__`，账号为假名）检查：开关、说明、间距，点击保存时提交 `rotate: true`，保存后序号隐藏；临时文件与 `.claude/launch.json` 已删除。
- [x] Stage 7（2026-10-24）：fmt / clippy（`-D warnings`）通过；`cargo test --workspace`（PowerShell）46 个测试程序 416 passed、0 failed、1 ignored（原有）；`sensenova.md`、`AGENTS.md` 已更新。Benchmark 对比 `pre035`（基线测量时机器在编译存储测试，偏高）：
  - 第一轮：`stream_proxy` passthrough 166.0 µs（−12.4%）、convert 420.3 µs（−4.0%）；`route_selection` provider_model 518.2 ns（−7.2%）、web_gates 538.9 ns（−4.5%）、virtual 7.29 µs（+7.1%）、exact 249.6 ns（+5.9%）。
  - 本次未改动 exact / virtual 路径，重跑 `route_selection`：provider_model 527.2 ns（−5.9%）、virtual 7.26 µs（+1.9%）、exact 229.0 ns（−2.8%）、web_gates 546.4 ns（−3.0%）。与 034 收尾时同一代码的干净测量（519.6 ns / 7.33 µs / 226.3 ns / 530.0 ns）相比都在噪声范围内。
  - 端点路径没有专门的 benchmark；未开启轮换时只多一次布尔判断，开启时每个请求多一次 `Mutex<HashMap>` 查找与一次候选旋转（成员数级别）。
  - 未在真实商汤流量上验证（需要那台设备装上新版本）。

## Decisions
- **开关属于端点，而不是 Provider 能力**：专用端点本来就限定于一个 Provider，且目前只有商汤总 API 使用。用户在商汤总 API 上开启即可满足"只用于商汤"，代码里不出现 provider 判断。
- **默认关闭**：保持 ADR-015 现有行为，已有端点升级后不变。
- **轮换在 `resolve_endpoint`，按成员账号旋转**：轮换在端点解析时完成，只影响端点路径；点名账号时尊重点名。按账号而不是按 target 旋转，同一账号的 chat / messages 仍在一起。
- **每个客户端请求取一次轮次**：等待恢复的多轮重试不再前进，避免一个请求跳过多个账号。
- **`rotate` 省略时保持原值**：`loadout endpoint enable sensenova` 之类不带该参数的调用不会把轮换悄悄关掉。
