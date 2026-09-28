# ADR-011: Local Gateway Security

- **Status**: Proposed
- **Date**: 2026-09-29

## Context

Gateway 监听本机端口。"只监听 127.0.0.1"并不等于安全：

- 本机任意进程都可以访问该端口；
- 浏览器中的任意网页可以向 `127.0.0.1` 发请求，并可能借助 **DNS rebinding** 读取响应；
- Gateway 背后是用户付费的 API 额度。

产品文档 §11 已要求即使 localhost 也使用 client key。

## Decision

### 1. 监听

- 默认 `127.0.0.1:8317`，端口可配置。
- 不提供 `0.0.0.0` / LAN 监听选项（产品文档 §30、§46 非目标）。

### 2. Client key（必需）

- 每个请求都必须携带 client key，接受：`Authorization: Bearer <key>`（OpenAI 风格）与 `x-api-key: <key>`（Anthropic 风格）。
- Key 格式：`lo-` 前缀 + 随机串；存储方式见 ADR-004 §4。
- 每把 key 绑定一个 Client，Client 可配置：默认 Virtual Model / Pool、allowed models、每日 token / 费用上限、请求速率上限、payload logging 策略（产品文档 §11 / §38）。
- 限额检查使用内存计数器，不在热路径查询数据库。

### 3. 防浏览器攻击

- **Host 校验**：只接受 `Host` 为 `127.0.0.1:<port>`、`localhost:<port>`、`[::1]:<port>` 的请求，其余返回 421 / 403（防 DNS rebinding）。
- **Origin 校验**：带 `Origin` 头的请求默认拒绝；用户可为特定 Origin 显式加白名单（例如本地 Web 工具）。
- 默认不返回任何 CORS 头。

### 4. 管理面

- v1 管理操作只通过 Named Pipe IPC（ADR-002），Gateway 端口上不暴露 `/_loadout/*`。

### 5. 上游出站

- 仅 HTTPS（`LOCAL` Trust Level 的本地 Provider 除外，如 Ollama）。
- rustls，使用系统证书存储。
- 用户自定义 OpenAI-compatible 端点使用 HTTP 时显示警告。

## Alternatives

| 方案 | 否决原因 |
|---|---|
| localhost 不鉴权 | 本机任意进程与浏览器页面都可消耗用户额度 |
| 仅依赖 Origin 校验 | 非浏览器进程不发送 Origin，无法防护本机恶意程序 |

## Consequences

- 每个客户端都需要配置 client key；UI 需要提供"复制配置"功能以降低门槛（例如为 OpenCode、Command Code 生成配置片段）。
- 浏览器中的 Web 应用默认无法调用 Gateway，需要显式加白名单。
