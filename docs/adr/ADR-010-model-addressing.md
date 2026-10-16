# ADR-010: Model Addressing

- **Status**: Accepted
- **Date**: 2026-09-29
- **Amended by**: [ADR-015](ADR-015-dedicated-endpoints.md)（专用端点 `/e/<name>/v1` 内可用裸模型名）

## Context

产品文档 §12 使用 `commandcode/kimi-k3` 形式的 exact model 名，但没有说明同一 Provider 有多个账号时如何区分。对齐阶段决定参考第三方 agent 平台导入 API 的做法：按"平台-账号-模型"分类，账号信息在录入时填写。

## Decision

### 1. 三种形式

| 形式 | 示例 | 含义 |
|---|---|---|
| `loadout/<name>` | `loadout/realtime` | Virtual Model 或用户 Pool，Router 有选择权（ADR-003） |
| `<provider>/<account>/<model>` | `opencode-go/main/kimi-k3` | 精确到账号 |
| `<provider>/<model>` | `opencode-go/kimi-k3` | 该 Provider 下任一账号上的该模型 |

### 2. 命名规则

- **provider slug**：由 Provider 模块定义，小写 kebab-case（如 `opencode-go`、`commandcode`、`openrouter`）。
  - Generic OpenAI-compatible Provider：用户添加时填写自定义 slug（如 `my-vllm`），不得与内置 slug 冲突。
- **account alias**：导入 / 录入账号时由用户填写，规则 `[a-z0-9][a-z0-9-]{0,31}`，在同一 Provider 内唯一。默认建议值 `main`。
- **model**：使用上游原始 model id，可以包含 `/`（如 OpenRouter 的 `anthropic/claude-x`）。
- `loadout` 为保留的 provider slug。

### 3. 解析算法

```text
seg[0] == "loadout"          → Virtual Model / Pool
seg[0] 是已知 provider slug:
    seg[1] 是该 provider 的 account alias → exact account 形式，model = seg[2..]
    否则                                   → provider 形式，model = seg[1..]
否则                          → MODEL_NOT_FOUND
```

**歧义防护**：录入账号或 catalog 刷新时，若 account alias 与该 Provider 某个模型 id 的第一段相同（例如 OpenRouter 账号起名 `anthropic`），拒绝该 alias 或提示用户改名。

解析在请求路径中只做内存字典查找（名称表随 RouteSnapshot 一起编译）。

### 4. `/v1/models`

- 返回：内置 Virtual Models、用户 Pools、每个可用 RouteTarget 的 `provider/account/model`。
- 默认不列出 `provider/model` 简写（避免列表重复），但简写始终可用于请求。
- 按 client key 的 `allowed models` 策略过滤（ADR-011）。

### 5. 跨 Provider 的"同一模型"

- `models.canonical_name` 用于识别不同 Provider 上的同一模型（如 OpenCode Go 与 Command Code 上的 `kimi-k3`）。
- v1 映射来源：Catalog driver 提供的默认映射 + 用户手动确认/修改。
- 仅在用户显式开启跨 Provider failover 时使用（ADR-003 §8）。

## Alternatives

| 方案 | 否决原因 |
|---|---|
| 只有 `provider/model` | 无法指定账号 |
| 账号用数字 ID | 不可读，客户端配置难以维护 |
| 用特殊分隔符（如 `provider@account/model`） | 与常见 agent 平台的 `provider/model` 习惯不一致；歧义通过录入校验解决 |

## Consequences

- 客户端配置可读性好，例如 `opencode-go/work/kimi-k3`。
- 重命名账号 alias 会使已有客户端配置失效；UI 在改名时必须提示。
