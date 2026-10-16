# ADR-015: Dedicated Endpoints（专用端点）

- **Status**: Accepted（2026-10-17，项目所有者要求）
- **Date**: 2026-10-17
- **Amends**：ADR-010 §1（新增第四种寻址：端点内的裸模型名）

## Context

项目所有者有三个商汤 SenseNova 账号，希望有一个"商汤总 API"：

- 与统一网关地址分开，不需要为此建子号池再切换；
- 客户端只填一个 Base URL 和模型名（如 `deepseek-v4-flash`），不关心账号；
- 时刻在线：单个账号出错（尤其是商汤按 key、按模型的 TPM/RPM 限流，返回 429）立刻换下一个；
- 开启时自动包含当前号池中的全部商汤账号。

现有的 `sensenova/<model>`（ADR-010 provider 形式）已经能跨账号 failover，但：模型名必须带前缀；`routing.max_attempts`（默认 3）按 RouteTarget 计数，同一账号的 Chat / Messages 两个 Target 会占掉两次机会；全部账号都在冷却时立即失败。

## Decision

### 1. 端点

- 端点是一个**命名的、限定于单个 Provider 的入口**：`http://127.0.0.1:<port>/e/<name>/v1/…`（OpenAI：`/chat/completions`、`/responses`、`/models`；Anthropic SDK 的 Base URL 为 `http://127.0.0.1:<port>/e/<name>`，路径 `/v1/messages`）。
- 名称规则同 provider slug；默认名等于 provider slug（`sensenova`）。
- 与统一入口共用同一个监听端口、同一套 client key 鉴权与 client 策略（ADR-011）；不新开端口，因此网关在线它就在线。
- 成员：`members = null` 表示"该 Provider 在**当前号池**中的全部账号"（随账号增删、子号池切换自动变化）；也可以显式列出账号。只有路由表中存在的账号（启用、在当前号池内）参与。

### 2. 寻址

端点内的模型名按以下顺序解析：

```text
<model>                       → 成员账号上的该上游模型
<provider>/<model>            → 同上（去掉前缀）
<provider>/<account>/<model>  → 同上（账号只决定首选顺序）
找不到且端点设了 default_model → default_model
否则                           → MODEL_NOT_FOUND（列出可用模型）
```

`GET /e/<name>/v1/models` 只列出端点内的裸模型名。解析仍只做内存查找（索引随 RouteSnapshot 编译，ADR-003）。

### 3. 选择与 failover

- 候选为成员账号上的同一模型，按账号顺序（粘性），每个账号只保留一个 Target（优先入口协议），尝试次数 = 成员账号数（不受 `routing.max_attempts` 限制）。这是"同模型换账号"，不换模型（ADR-003 §8）。
- **等待恢复**：全部候选都处于冷却 / 限流且最早的恢复时间在 `wait_secs`（默认 30 秒，0 关闭）之内时，请求在网关内等待到该时间后重新选择，而不是立即报错；全部尝试都因限流失败时同样处理。等待不发出任何网络请求，不违反 Harness §2.2。

### 4. 恢复监视

成员账号的 Target 进入限流 / 冷却 / 额度耗尽（非终止状态）后，冷却到期时由后台用极小请求探测一次（复用测速 probe），成功即恢复并记录恢复时间；失败则按退避继续。终止状态（凭证被拒绝两次）不探测，等待用户处理。

## Alternatives

- **独立端口**：需要再管理一个端口和监听生命周期，客户端配置也不会更简单。拒绝。
- **只用 `sensenova/<model>`**：不能去掉前缀、受 `max_attempts` 限制、全部冷却时立即失败。
- **轮询（round-robin）分摊**：能减少 429，但违背 ADR-003 的粘性顺序；留作端点的后续选项。

## Consequences

- 新增表 `endpoints`（migration 0012）、IPC `endpoint.*`、网关 `/e/<name>/…` 路由。
- Router 新增端点索引与恢复时间字段；Router / Gateway 改动需跑 benchmark（Harness §20）。
- 恢复探测会消耗极少量额度，只针对端点成员。
