# ExecPlan 026: OpenCode 官方常驻服务验证

- Status: Completed, 2026-10-18

## Goal
先验证未修改的官方 OpenCode 免费模型可经本地 HTTP 服务调用并转换为 API；通过后接入 Loadout。禁止完整部署 9router。

## Context
2026-10-18 用户明确授权先验证、成功后接入。此前普通 HTTP 请求被 FreeTierError 拒绝，不能把目录可见当作推理成功。

## Existing behavior
Loadout 有 ProviderModule、CLI bridge trust、逐账号 opt-in、通用面板与 HTTP Transport。已有 WorkBuddy 变更未提交，本任务保留。

## Constraints
官方程序不修改身份或注入伪装工具；仅隔离目录、loopback、随机认证；不读取现有凭证，不使用付费 Provider，不操作真实工程文件。正式接入前须明确代理语义与正文落盘边界。

## Architecture impact
验证仅通过官方 serve API。成功后再记录 Provider ADR；使用常驻进程，禁止每请求启动 CLI。

## Files expected to change
本计划；验证脚本与脱敏结果；Provider、注册、部署文件；IPC/Core 的通用 opt-in 元数据与面板消费。

## Implementation stages
1. 固定官方版本并核对发行物 SHA256。
2. 在独立 XDG/config/data/workspace 启动已认证的服务。
3. 实测精确免费模型，检查错误对象、文本、模型、会话隔离与清理。
4. 验证 API 转换；通过后设计最小集成，未通过则保留证据并停止部署。

## Tests
Live 显式 opt-in；最小合成提示，无敏感输入。正式实现需 mock、失败处理、身份认证及请求隔离测试。

## Performance validation
记录实测耗时。未修改 Router/Gateway；若修改，执行规定 benchmark。

## Security considerations
官方 Agent 拥有工具执行能力，验证工作区为空且权限 deny。Basic auth 只在 loopback；密码不写日志/结果。未通过时结束本任务创建的进程。

## Migration considerations
无数据库 migration。

## Known risks
官方版本变化、免费模型限制；工具权限关闭可能影响免费通道；Agent 回复不是无损模型 API；官方服务可能存储对话。

## Definition of done
验证结果可复查；成功才接入并验证 Loadout API/面板；失败明确说明未集成及具体阻塞，清理测试进程。

## Progress
- 已核对官方 server 文档及 v1.19.3 发行物 SHA256：e4ce9117f55fc068414513789f8c7d1049f855d7518bb4e350b9170a3a20d8c3。
- 本机 PATH 未发现 opencode；下载至临时验证目录，未全局安装。
- v1.19.3 三个模型均上游 403，不能用旧版判断整条路径。
- 官方 v2 npm 2.0.32 Windows SHA512 integrity 已核对；ask 权限会话 big-pickle 成功，deny 与无会话 generate 失败。
- 内存数据库模式两次独立请求成功（3.05 / 4.83 秒），工具请求 422（1.40 秒），未批准工具，剩余 session 为零；data 仅零字节日志，没有 db。
- 已实现 Provider、原生 API 文本 bridge、DPAPI 部署与启动脚本；Python 5 个测试和 Rust 2 个 Provider 测试通过。
- Loadout 隔离 daemon 的 exact-model JSON 与 SSE 均返回合成标记；初次 SSE 暴露 stream_options 兼容问题，已修复并复测通过。
- 面板此前只对 OAuth/browser 显示 opt-in，补充 capability-driven IPC bool，使 API-key bridge 也可在面板启停，无 slug 分支。
- Provider 81 tests、Core 58 tests、IPC 16 tests 通过；前端构建、workspace clippy 通过。正在最终构建与生产接入。
- release workspace build 通过；最终隔离链路 JSON/SSE 与面板 opt-in metadata 通过。
- 生产 daemon / GUI 已备份后更新到 `target/v3/release`；配置 `opencode-free/main` 并按本次用户授权 opt-in。
- 生产 exact-model JSON 与 SSE 均返回 `LOADOUT_OFFICIAL_API_OK`，临时 client 已撤销；默认内置池中 bridge 数量为 0，原有 Provider 均保留。
- 部署官方 2.0.32 实例最终 session 为 0，数据库文件为 0；两台临时测试服务已停止，测试认证文件已移除。
- 面板前端构建与已部署 GUI 进程、实际 IPC 数据通过验证；未进行截图/视觉验收。
- 没有 Router/Gateway 热路径改动，不需要性能 benchmark；没有数据库 schema 变更，不需要 migration。

## Deployment receipt
部署目录 `%LOCALAPPDATA%\LoadoutOpenCodeBridge`；18789 bridge / 18790 official，仅 loopback。旧程序和 SQLite online backup 的目录记录在该目录 `backup-path.txt`。模型入口 `opencode-free/main/big-pickle`，Loadout 原网关 `http://127.0.0.1:8317/v1`。

本次不添加系统自启。重启电脑后运行 `powershell -NoProfile -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\LoadoutOpenCodeBridge\Start-OpenCodeBridge.ps1"`。当前服务已运行。回滚时关闭该账号 Transport、停止仅属于该目录的两个 PID，并恢复备份程序；不要删除/恢复用户其它账号数据。

## Decisions
- 使用官方独立 Windows 二进制与 Loadout spawn_detached helper；不安装 9router。
