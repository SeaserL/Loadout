# WorkBuddy International (`workbuddy-bridge`)

- **Last verified**: 2026-10-17（Australia/Sydney）
- **Subscription Transport Status**: OPT-IN, PRIVATE_ENDPOINT，ADR-016。
- **Provider slug**: `workbuddy`；当前安装账号 `main`，绑定现有国际版桌面账号。
- **实现**：原创 Node loopback bridge → 官方运行中客户端 Named Pipe → WorkBuddy 国际版；不是第三方 workbuddy2api 原样安装。

## 验证结论

实际经客户端 IPC 调用 `deepseek-v4.1-flash`：HTTP 200、同名模型、预期 `LOADOUT_OK` 文本、SSE DONE。隔离 Loadout 和正式 Loadout 的 Chat JSON、缓冲 SSE 均通过。

完整上游目录 **UNKNOWN**：`/v3/config` 经 IPC 返回 400 / code 12403（版本 UA 检查）。没有伪造 UA 修复此接口。bridge 的 `/v1/models` 只展示经过最小 live 验证的单个模型，不代表实时发现全部模型。没有独立验证供应商内部权重身份、价格或“永久免费”。

## 使用

- Base URL：`http://127.0.0.1:8317/v1`
- API key：已有 Loadout client key，不是 WorkBuddy token，也不是 bridge 内部 key。
- 精确模型：`workbuddy/main/deepseek-v4.1-flash`
- 专用池：`loadout/workbuddy-flash`
- 未加入内置 `loadout/default|fast|realtime`，不跨模型 fallback。

```json
{"model":"workbuddy/main/deepseek-v4.1-flash","messages":[{"role":"user","content":"你好"}],"max_tokens":512,"stream":false}
```

## 能力与限制

- Windows；现有客户端必须运行并登录。每次发请求读取该客户端的本地 IPC ticket，仅内存使用，不导出 WorkBuddy token。
- 一个桌面账号、单并发。账号 UID 只做本地哈希绑定；切换账号返回错误，不能悄悄变成另一个账号。
- 文本 Chat，多轮由调用方传完整 messages；不注入 previous_response_id。工具和图像尚不支持，会明确拒绝。
- `max_tokens` 1–8192，默认 4096；请求体上限 256 KiB。
- 官方 IPC 50 秒超时、640 KiB 内联响应上限。SSE 是**完整收到上游后再返回**，不是实时逐 token 流；响应头 `x-loadout-stream-mode: buffered`（bridge 层）。
- 不提供原生 Responses stateful；Loadout 的无状态协议转换仍可用，但本次 live 验收仅 Chat。
- 每次验证结束撤销临时 client key；隔离实例使用临时目录、随机 pipe/instance、`loadout-e2e-<id>:` 凭证前缀，结束删除测试账号并停止。

## 安装与运行

当前 bridge 文件和 DPAPI 加密 key 位于 `%LOCALAPPDATA%\LoadoutWorkBuddyBridge`；监听 `127.0.0.1:18788`，所有端点要求本地 key，禁止带 Origin 的浏览器请求。Loadout 的该 key 副本在 Windows Credential Manager。

2026-10-18 已启用当前 Windows 用户登录自启：`Loadout` 启动 daemon；`LoadoutWorkBuddyBridge` 启动单实例 Node 守护；官方 `WorkBuddy.WorkBuddyAI` 原有自启保留。守护每 15 秒通过 CLI 的 IPC status 检查 daemon，仅在 daemon 可用时确保桥接运行。桥接意外退出后自动重启；不自动恢复过期登录、不强杀已有请求。Loadout 停止后桥接可继续驻留，但守护不补起桥接。

重复安装/配置（Loadout 路径变化后也需重新执行）：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/Install-WorkBuddyBridgeAutostart.ps1 -Loadout D:\Loadout\target\v3\release\loadout.exe
```

守护用独占 Named Pipe 防重复，启动脚本用用户/目录级 Mutex 串行检查认证健康和等待就绪；端口被其它或异常服务占用时不反复创建进程。`autostart-status.json` 只保存状态/时间/PID，不代表 WorkBuddy 上游登录或模型可用性。后台 launcher 使用 Windows GUI subsystem，避免登录时弹出控制台。原 Run 值、脚本备份保存在安装目录 `autostart-backup-*`。

手动检查/重新启动：

```powershell
powershell -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\LoadoutWorkBuddyBridge\Start-WorkBuddyBridge.ps1"
```

后台进程由 `loadout_platform::spawn_detached` 启动，源码 helper 为 `crates/platform/examples/spawn_detached.rs`。不修改官方客户端文件、登录状态、浏览器 cookie 或账号配置。

撤销新增自启可执行 `loadout autostart disable`，并删除 HKCU Run 下 `LoadoutWorkBuddyBridge` 值；守护本次会话仍运行到注销，或核对 `autostart-status.json` 的 PID 对应 `workbuddy_supervisor.mjs` 后停止该进程。不要删除官方 WorkBuddy 自启。2026-10-18 已验证重复启动、桥接退出后恢复以及恢复后的生产池 JSON/SSE HTTP 200；未实际重启电脑。

正式 daemon 使用既有 `D:\Loadout\target\v3\release` 位置，更新前已备份旧 CLI/daemon 到其 `workbuddy-backup-20261017` 子目录。数据库与配置通过原 daemon 的 backup 命令备份到默认 backups 目录。GUI 二进制未改，未进行视觉验收。

## 源码审查与许可

Primary `Zhengyuuuui/workbuddy2api@1cc32c9a98abb53796d8d4e8d477736fe1f0c4cc`：MIT，未复用代码。`codebuddy_proxy.py` 存在空响应切换 `default-model`、system prompt 指纹共享 conversation、明文 session、默认无入站鉴权、超量工具静默截断等行为。不适合原样部署。

Secondary `VentourF/WorkBuddy2api@ed6c2d9e380b53028a74a70da18d8f36b2fb8414`：`proxy/wbproxy.py` 的捕获日志包含请求头与正文；具体路径 license 未确认，不复用。

官方安装包 `resources/app.asar` 的 `main/server.js` 说明 `wb.request/http.fetch` 与 HMAC 握手；`main/file-authentication-storage.js` 说明账号元数据路径。只参考接口事实，未复制官方代码；本项目 scripts 为原创 Apache-2.0。外部 Python/Node 包依赖为零。

服务条款与代码许可分开：客户端存在内部接口不代表官方授权通用 API 转发；稳定性、账号限制仍可能变化。

## 国际版积分活动

截至检查日，[官方价格页](https://www.workbuddy.ai/pricing/) 与[官方 FAQ](https://www.tencentcloud.com/document/product/1256/77297) 仍列出每日活跃奖励：Free 30，Pro 50。当天在 WorkBuddy/CodeBuddy 客户端至少一次有效对话或任务，次日自动结算，无需手动领取。仅打开/登录不够，网页对话不计入；Team 不适用。活动限时，结束时间另行公告。

不是国内“加油站”签到接口；未执行领取、未注册自动签到任务，未确认 bridge API 流量是否计入，未读取用户具体积分到账记录。

## 验证命令

```powershell
node --test scripts/workbuddy_bridge.test.mjs
python scripts/workbuddy_bridge_key.py self-test
cargo test -p loadout-providers
cargo test -p loadout-core login_opt_in_and_routing
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
$env:LOADOUT_LIVE_TESTS='1'
python scripts/workbuddy_verify.py --loadout target\release\loadout.exe
python scripts/workbuddy_verify.py --loadout target\v3\release\loadout.exe --production
```

真实验证会发送极短 prompt，消耗以供应商计费为准。
