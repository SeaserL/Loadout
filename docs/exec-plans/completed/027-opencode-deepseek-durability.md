# ExecPlan 027: OpenCode DeepSeek 与持续会话验证

- Status: Completed, 2026-10-18

## Goal
按用户要求验证 deepseek-v4.1-flash-free 的连续调用与上下文持续性；证据通过后加入现有 Loadout bridge，并评估放宽单轮限制。
## Context
用户认为单轮文本 Agent 桥接限制过大。当前只开放 big-pickle，官方服务 2.0.32 使用内存数据库、每请求独立 session。
## Existing behavior
参考 ExecPlan 026 / ADR-017；现有代码与未提交修改全部保留。
## Constraints
只用未修改官方服务；不伪装身份、不批准工具、不调用付费 Provider；不把短测当长期 SLA。不把内存会话当重启持久存储。
## Architecture impact
先验证，不改 Router/Gateway。若支持多轮，用官方原生会话/历史机制，不能把串接 transcript 冒充无损 Chat。
## Files expected to change
验证脚本与 JSON receipt；通过后 bridge、Provider、测试、ADR-017 与 Provider 文档。
## Implementation stages
1. 精确 DeepSeek 请求与同会话连续多轮。
2. 空闲后召回、会话隔离、内存模式重启边界。
3. 验证官方历史/流式接口，选择可实现的升级；失败如实记录。
4. 通过后部署并从 Loadout 验证。
## Tests
显式 LOADOUT_LIVE_TESTS=1，小型合成请求，检查模型字段、finish/error、响应内容、耗时与会话清理。修改后 mock tests、fmt/clippy、相关构建。
## Performance validation
实测延迟与样本数量；不修改 Router/Gateway 时不执行其 benchmark。
## Security considerations
认证从部署 DPAPI 读取，不打印；不访问真实工程/用户对话，不批准 Agent 工具。重启测试使用隔离服务。
## Migration considerations
无数据库 migration；保持默认正文不落盘。
## Known risks
免费额度/模型映射随上游变化；官方 Agent 上下文与工具限制；API 参数未必可无损映射。
## Definition of done
精确模型与持续性有可核对结果，支持范围清楚，成功修改完成实际 Loadout 验证；没有长期稳定性或完整工具 API 的虚假承诺。
## Progress
- 已读取现有实现、ADR 与前次证据；开始 DeepSeek 实测。
- 原生会话 6 次推理完成；召回、空闲、隔离通过，中文因句号差异 1 项精确断言失败。
- import 初次缺少 assistant completed 时间导致历史丢失，自行生成 session ID 失败；保留官方创建 ID、完整 settled message 后召回通过。
- 修订 ADR-017，支持两模型与完整文本多轮，保留 Agent system update、工具拒绝、缓冲 SSE 边界。
- 隔离服务重启后旧会话 404，完整历史重传召回通过；临时进程清理完成。
- Python 8 tests、Provider 2 tests、workspace clippy、release workspace build 通过。12 次持续调用与 Loadout 验证进行中。
- 隔离与生产 Loadout 的 DeepSeek 完整历史 JSON/SSE 通过；原有 big-pickle 生产多轮 JSON/SSE 回归通过。已备份并更新 daemon/GUI，model sync 新增 1、更新 1。
- 64 条消息边界测试发现默认分页导致完整性检查失败，显式 limit=100 后最早 assistant 精确召回通过；Python 8 tests 复测通过，补丁已部署。连续测试中有一次受控 bridge 重启，不能称为单进程不间断运行测试。
- 连续测试 364.827 秒、12/12 成功，历史 2→24 条，延迟 min/median/max = 1.482/1.8755/7.527 秒；期间一次受控桥接重启，官方服务未重启。剩余 session=0、db 文件=0。
- 分页修复后生产 DeepSeek 多轮 JSON/SSE 再次通过。实际模型目录已含两模型，默认 builtin 池 bridge 条目为零。未做 GUI 截图验收。
- 无 Router/Gateway/schema 变化。新二进制备份指针 `%LOCALAPPDATA%/LoadoutOpenCodeBridge/deepseek-backup-path.txt`；未添加系统自启。机器重启后仍需显式启动脚本。
- 结果见 `docs/research/opencode-deepseek-{durability,history-import-settled,restart,history-limit-fixed,continuity,loadout}-20261018.json`；失败记录也保留。没有长期 SLA、完整工具 API、纯模型 system 语义或服务端持久存储承诺。
## Decisions
- 持久性拆为连续可用、会话上下文、空闲恢复、进程重启四项验证。
