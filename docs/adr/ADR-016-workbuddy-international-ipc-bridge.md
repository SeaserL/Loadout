# ADR-016: WorkBuddy International local IPC bridge

- **Status**: Accepted for the user-requested local integration (2026-10-17)
- **Amends**: ADR-009（仅 WorkBuddy 国际版本人已登录客户端的显式接入）

## Context

用户明确要求把国际版 `deepseek-v4.1-flash` 接入 Loadout，并要求使用已经登录的后台客户端。检查安装版本源码发现客户端提供带 HMAC 双向认证的 Named Pipe `wb.request/http.fetch`，由客户端自身添加登录凭证。2026-10-17 最小真实请求返回 HTTP 200、同名模型、预期文本和 SSE DONE。目录接口仍报版本头错误，不能声称完整目录验证通过。

两个 workbuddy2api 项目均不适合原样部署：primary 有跨模型重试、隐式会话串联、明文 session；secondary 会记录认证请求头。选用独立原创的小型本地 bridge，通过客户端现有 IPC 发请求，不复制第三方代理或官方客户端源码。

## Decision

- 编译内置 `workbuddy-bridge` Provider，slug `workbuddy`；trust 为 PRIVATE_ENDPOINT，逐账号显式 opt-in，不进入内置虚拟模型，只使用 exact name 或显式 Pool。
- Bridge 仅监听 127.0.0.1，并验证随机本地 API key。Loadout 保存 key 于 Credential Manager，bridge 端用当前 Windows 用户 DPAPI 保存；WorkBuddy token 始终留在原客户端内。
- 不刷新、不导出客户端 token，不修改客户端/浏览器 profile，不注入进程，不模拟设备/TLS/官方身份。仅通过客户端已存在的受认证 IPC 方法。
- 客户端必须保持运行和登录，账号切换后 bridge 拒绝服务，需重新绑定；首版只绑定一个当前桌面账号。
- 首版只暴露已实际验证的 `deepseek-v4.1-flash`。不 fallback 到其他模型，不注入 previous_response_id，不按 system prompt 合并对话。
- 上游 IPC 限制：50 秒、640 KiB 内联响应，先收完再返回。对外支持 Chat JSON 与缓冲 SSE；不宣称实时 token streaming 或原生 Responses stateful。首版文本/推理，tools、vision 未验收，拒绝而非假装支持。
- 不存请求/回答正文；上游错误用固定分类，避免回显敏感负载。
- 开关和部署以本次用户明确接入授权为依据，保留现有 alias 确认与审计机制；不自动为其它账号启用。

## Alternatives

- 原样安装第三方代理：凭证、exact-model 和会话隔离不符合要求。
- 新网页登录：用户不方便操作，已有客户端 IPC 可以复用登录。
- 解密已有 token：没有必要，扩大 secret 接触范围。
- Generic loopback Provider：会标记 LOCAL，无法表达私有订阅来源，故不使用。

## Consequences

### 用户登录自启补充（2026-10-18）

用户明确要求随 Loadout 自动启动。采用部署层单实例 Node 守护（HKCU Run），通过已有 CLI/IPC 检查 daemon 后启动桥接并恢复退出的桥接；独占 Named Pipe 和启动 Mutex 防止重复实例。进程创建复用 `spawn_detached`；短暂 CLI/PowerShell 检查使用私有 stdio 和隐藏窗口，不在推理热路径。保留官方客户端原自启与登录，启用 Loadout 既有 autostart。不改变 Core/Provider 公共接口，测试 daemon 不会自动拉起生产桥接。Loadout 停止不强杀桥接，以免中断在途请求。

没有 Router/Gateway/schema 变更。需要常驻 bridge 和新增内置 Provider；客户端 IPC 版本变化可能使 bridge 失效。技术可用不代表官方允许通用 API 转发，也不证明代理请求满足每日活动奖励条件。Windows 本地接入不适合作为公共服务部署。
