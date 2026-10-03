# ExecPlan 015: Demo — Transparency (decision chain, cache stats, richer /v1/models)

- **Status**: Completed
- **Created**: 2026-10-04
- **Completed**: 2026-10-04
- **Phase**: 功能调研落地（[feature landscape §5](../../research/2026-10-04-feature-landscape.md) "透明度"与"模型"）

---

## 1. Goal

快速做出可演示的透明度功能，带测试：

1. 每条请求日志记录**决策链**（跳过原因、失败的尝试、最终结果），只存元数据；
2. 统计里给出**缓存命中率**和**预计节省金额**；
3. `GET /v1/models` 带上上下文长度、档位、健康状态、价格和类型，Pool / 虚拟模型排在前面。

## 2. Decisions

- **决策链格式**：`a/b/m: skipped (quota) → c/d/m: 429 rate_limit → e/f/m: ok`。
  - 只含 target 名、固定的短原因、`FailureKind::code()` 和 HTTP 状态码；**不含**上游错误正文、请求或响应内容（ADR-008 §1）。
  - 选路排除的 target（`Candidates::skipped`）排在最前；熔断类原因细化为 `quota` / `rate limited` / `auth failed` / `cooldown`（读原子状态）。
  - 同一账号模型的不同协议变体产生相同步骤时只记一次。
  - 上限 500 字节：超出时中间步骤记为 `… N more`，始终保留最终结果；按字符边界截断。
  - 在选路之后构建，只用内存数据，不违反热路径规则（ADR-003）。成功的单次请求只多一次 `exact_name()` 格式化。
- **存储**：migration `0009_request_decision` 给 `request_events` 加 `decision TEXT`（可空，旧行为 NULL）。目录编号沿用协调好的 0009；版本号由在 `MIGRATIONS` 中的位置决定，与其他分支合并时需按合并顺序排列。
- **缓存统计**：`request_events` 早已有 `cache_read_tokens` / `cache_write_tokens`。
  - 命中率 = `cache_read / input`（规范 usage 语义里 input 包含缓存 token），封顶 100%。
  - 节省 = `cache_read × (输入价 − 缓存读价) − cache_write × (缓存写价 − 输入价)`，按 `route_target` 关联 `account_models` 的**当前**价格；没有缓存价格的模型计为 0。
  - IPC `StatsEntry` 追加 `cache_read_tokens`、`cache_write_tokens`、`cache_hit_rate`、`cache_savings_usd`，均 `#[serde(default)]`；`RequestSummary` 追加 `decision`。
- **`/v1/models`**：保持 OpenAI 形状，每项可选追加：
  - `context_window`（虚拟模型取当前链首）；
  - `loadout: {kind: virtual|pool|exact, tier, state, input_price, output_price}`，虚拟模型 / Pool 另有 `head`（第一个可用 target）和 `targets`（链长度）；
  - 顺序：内置虚拟模型 → Pool → exact，各组按名称排序。
  - 数据全部来自内存快照：`RouteTable` 新增 `with_pool`（标记 Pool）和 `with_tiers`（构建时的档位），由 `route_build` 写入。Anthropic 形状不变。
- **UI**：
  - CLI：`loadout logs --verbose`（`-v`）在表格后列出每行的决策链；`loadout stats` 增加 CACHE HIT / CACHE SAVED 两列和汇总行。
  - 桌面：日志行可展开显示决策链（按钮 tooltip 也显示）；统计页"Token 构成"卡增加缓存读 / 写、命中率和预计节省。
  - 新 i18n key 放在两份字典末尾的 `// Demo: transparency` 区块。

## 3. Validation

- 单元测试：migration 0009 上下行（列存在 / 删除、数据保留）；`request_stats` 缓存 token 与节省（带价格和无价格的模型）；`decision` 写入与读回；决策链截断、去重、空链。
- Gateway 集成测试：429 failover 的决策链；下一次请求显示 `skipped (rate limited)` 且不含上游错误文本；`/v1/models` 的排序、`kind`、`state`、`head`、价格。
- e2e：`proxy_e2e` 校验日志里的 `decision`；`latency_e2e` 校验 Pool / 虚拟模型的 `kind` 和排序；`stress_e2e` 校验缓存字段存在。
- `cargo fmt`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`npm --prefix apps/desktop run build` 通过。

### Benchmarks（Harness §20）

`cargo bench -p loadout-benchmarks --bench stream_proxy --bench gateway_skeleton`，本机，均值。参考值取自同一天 main 上的上一次运行（`target/criterion`）。本机噪声较大：`passthrough_200_events` 重跑一次得到 179 µs。

| Benchmark | 本次 | main 参考 | 目标 |
|---|---|---|---|
| `gateway_skeleton/get_models_keepalive` | 48.8 µs | 59.3 µs | P95 < 1 ms |
| `stream_proxy/passthrough_200_events` | 287 µs / 179 µs（重跑） | 253 µs | — |
| `stream_proxy/convert_chat_to_messages_200_events` | 566 µs | 616 µs | — |
| `ttft/direct_upstream` | 90.9 µs | 103 µs | — |
| `ttft/via_gateway_passthrough` | 523 µs | 698 µs | 额外 TTFT P95 < 10 ms |
| `ttft/via_gateway_converted_messages` | 558 µs | 601 µs | 额外 TTFT P95 < 10 ms |

结论：没有可测出的回退（差异在噪声范围内）。

## 4. Limitations

- 节省金额使用模型的**当前**价格，而不是请求发生时的价格；改价后历史节省会随之变化。
- 选路排除的 target 一律列在尝试之前，而不是按链中的原始位置穿插。
- 决策链只记录本次请求；流式请求提交后的中断体现在最终状态（`error` / `client_closed`），不另记步骤。
- 档位来自路由表构建时的快照；档位变化后要等下一次重建才更新到 `/v1/models`。
- 桌面端没有新增组件测试（与现有页面一致，只做类型检查和构建）。
