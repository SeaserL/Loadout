# ExecPlan 024: WorkBuddy International API bridge

- **Status**: Completed（单模型文本接入；限制见 Provider notes）

## Goal

检查 Windows 上 WorkBuddy 国际版的 `deepseek-v4.1-flash` 真实可用性；通过后安装受控 bridge 并接入 Loadout 账号池。核实国际版每日积分活动。

## Context

2026-10-17 用户授权选择 workbuddy2api、先审源码和真实模型能力、可行后安装接入；需要登录时告知。已运行的国际版客户端不得中断。

## Existing behavior

Loadout generic OpenAI-compatible 可复用 Chat 协议，但 loopback 被标记 LOCAL，不能据此掩盖订阅私有端点来源。ADR-009 尚未允许 WorkBuddy Transport。已有逐账号 opt-in 与显式 Pool 可复用。

## Constraints

遵循 ADR-004/005/009/010。不得复制或刷新现有客户端 refresh token，不读取浏览器 cookie。账号登录单独进行；不打印 secret，不落盘请求内容。只使用本人账号，不注册账号或自动领取活动奖励。

## Architecture impact

可行性 gate 已通过。ADR-016 标记 PRIVATE_ENDPOINT、默认关闭、exact model、显式 Pool。没有更改 Router/Gateway；不把上游硬编码列表当成 live 证据。

## Files expected to change

本 ExecPlan、ADR-016、Provider notes/reference-map/index、providers 的 workbuddy_bridge/lib/registry、CLI 通用风险说明、platform 的 detached launcher example、scripts 下 IPC probe/bridge/key/start/verify 与 Node tests。第三方源码审查副本位于 D:/workbuddy，未导入 Loadout 源码。

## Implementation stages

1. 固定两个候选 commit，检查认证、模型、日志、协议与 Windows 行为。
2. 独立登录、动态模型目录、最小真实请求，区分目录存在与推理成功。
3. 通过后记录 ADR，保护凭证、配置 bridge 与 Loadout。
4. 验证 exact-model、stream/error 行为与运行状态。

## Tests

源码审查先行；默认测试仅 mock。真实请求显式 LOADOUT_LIVE_TESTS=1，仅短测试，不操作用户项目。若需登录暂停依赖步骤并向用户提供登录入口。

## Performance validation

当前仅审查，不改变 Router/Gateway；若后续改变则运行 Harness §20 benchmark。

## Security considerations

审查发现 primary 默认明文 session、无客户端鉴权、官方指纹模拟；secondary 可能记录完整请求头。原样部署尚不通过。先尝试如实标识的独立认证，不默认采用指纹伪装。现有 WorkBuddy 后台进程保持运行。

## Migration considerations

当前无 schema 变更。不得直接写生产 SQLite。

## Known risks

服务端权限/套餐/模型可能变化；目录展示不证明推理。第三方协议不代表官方第三方 API 授权。国际版奖励不能套用国内签到接口。

## Definition of done

固定来源、凭证隔离、目录尝试及真实推理证据、Loadout exact-model 调用通过；目录不可用明确 UNKNOWN，仅开放实际验证的单模型。不要求完整模型目录、tools/vision 或实时 token streaming。

## Progress

- 2026-10-17：完成仓库启动阅读；工作树初始干净。
- Primary `Zhengyuuuui/workbuddy2api` commit `1cc32c9a98abb53796d8d4e8d477736fe1f0c4cc`，MIT；当前仅源码审查。
- Secondary `VentourF/WorkBuddy2api` commit `ed6c2d9e380b53028a74a70da18d8f36b2fb8414`；未验证源文件 license，不复用代码。
- 确认官方国际站仍公布每日活跃奖励 Free 30 / Pro 50，客户端有效对话、次日结算；未执行领取。
- 未认证 GET /v3/config 返回 code=0，但 models 尚未取得有效列表；不能视为模型可用。
- 用户改为要求使用本机已登录客户端；独立网页登录已停止，没有创建会话或读取 refresh token。
- 官方安装包提供 wbipc Named Pipe `wb.request/http.fetch`。原创探针通过 ticket HMAC 双向认证，不提取 token；2026-10-17 live smoke HTTP 200、6 SSE data 帧、同名模型、LOADOUT_OK、DONE、0 error。
- 经客户端获取目录 HTTP 400 / code 12403（版本 UA 检查）；完整目录 UNKNOWN。仅开放真实请求已验证的目标模型。
- primary 源码进一步发现非流式空响应自动切换 default-model、system prompt 指纹合并对话；决定不部署第三方源码，改用官方 IPC 的原创最小 bridge。
- ADR-016 已记录当前用户授权范围；Provider、loopback bridge、DPAPI 本地 key、反例测试已实现。Node 5 项测试、Provider 2 项测试通过；release 编译通过。Clippy 首次发现测试 unwrap，已修正，待复验。
- 追加修复：上游空 function_call 占位不算工具调用；流式验收按 delta 拼接文本，不用原始 SSE 连续子串匹配。Node 7 项通过，Provider 全部 79 项通过，core opt-in/routing 测试通过，workspace/all-targets clippy 与 fmt 通过。
- 隔离 daemon 使用临时 data/随机 pipe/随机 instance/测试凭证前缀；普通与流式 Chat HTTP 200；测试账号移除、临时 client 撤销、daemon 停止。
- 正式部署前 route in-flight=0；数据库备份 `backups/loadout-20261016-194108.db`、配置备份 `backups/loadout-config-20261016-194108.json`（文件名由原 daemon 的 UTC 时间生成）。旧二进制备份到 `target/v3/release/workbuddy-backup-20261017`。
- 正式 daemon 更新、`workbuddy/main` 启用、专用池 `loadout/workbuddy-flash` 创建；正式 JSON 与 SSE 各一请求验证通过。原 WorkBuddy 后台未停止，未导出其 token。
- 安装目录 `%LOCALAPPDATA%/LoadoutWorkBuddyBridge`，端口 18788，仅 loopback；手动启动脚本已部署，未新增登录自启项。
- 当前交付边界：文本、单并发、缓冲 SSE；完整上游目录/积分到账/代理请求活动资格/GUI 视觉验收未确认。
- 最终正式池 `loadout/workbuddy-flash` 的 JSON 与 SSE 也均 HTTP 200；route trust 确认为 PRIVATE_ENDPOINT，in_flight=0。原 6 个 WorkBuddy 进程均仍存在；原有 3 个 client 的身份和 key prefix 保留，新增验收 client 已撤销。
- 手动启动脚本实测识别运行中的 bridge，不重复启动。CLI 通用低信任提示已改为兼容外部客户端登录，重新编译部署；未改变 GUI 或原有账号池。

## Decisions

- 最终未安装两个第三方代理，使用原创 bridge 调用官方现有 IPC；其硬编码目录不作为验收证据。
- 保留现有桌面登录；不提取客户端凭据。网页登录待授权流程已取消，并删除本次创建的 pending DPAPI 状态。
