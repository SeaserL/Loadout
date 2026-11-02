# 047：真实客户端、工具、长会话和凭证轮换验证

日期：2026-10-30，Australia/Sydney。采用046的OmniRoute v3.8.52完整候选与既有429补丁。不做UI、不改正式实例、安装、客户端配置或WorkBuddy登录，不调用CommandCode。

## 结论

**窄适配路线仍值得保留，但当前不能判定整个组合已可靠到可替换日常主网关。** Claude Code/OpenCode的受限文本和只读文件工具链已实际通过；三轮长上下文通过。Codex到WorkBuddy仍受上游渠道拒绝，商汤首请求仍429；真实凭证轮换没有发生。不要为了这些阻塞扩大私有补丁。

本轮后台新增hook **0**，046限流patch仍为单文件+25/-1，完整候选21732文件hash复核通过。新增工作限于实验连接、内存凭证reader和验收脚本。

## 实测矩阵

| 范围 | 结果 | 证据与限制 |
|---|---|---|
| Claude Code文本 | PASS | 官方已安装CLI，约3.01秒，合成marker正确 |
| Claude Code只读工具 | PASS | 约4.55秒；上游返回工具调用，客户端读临时acceptance.txt，下轮携带正确tool result，最终答复正确 |
| OpenCode文本 | PASS | 官方已安装CLI，约3.88秒，合成marker正确 |
| OpenCode只读工具 | PASS | 约4.44秒；完整工具往返与最终答复正确 |
| Codex文本 | BLOCKED | 消除本地stream限制后，真实上游HTTP400/CHANNEL_REJECTED |
| Codex只读工具任务 | BLOCKED | 同样被真实上游拒绝，未到工具执行环节 |
| 三轮完整history | PASS | 输入49805 / 99447 / 149089 bytes，约2.67 / 2.43 / 4.05秒，均找到最早合成marker |
| 凭证轮换机制 | PASS（离线） | 加密文件token替换、protector key替换、过期拒绝及新有效文件恢复 |
| 真实桌面token轮换 | NOT_OBSERVED | 两轮live中tokenChanges均0，不能把离线通过算成真实轮换通过 |
| 商汤真实调用 | BLOCKED | 固定一个账号、deepseek-v4-flash，第一次真实请求429，约1.67秒，随后停止 |

WorkBuddy真实上游总计 **11次**：9次成功、2次Codex渠道拒绝。两次最初Codex本地400未到上游，单独保留。商汤真实调用 **1次**。均未跑完整号池、换账号或偷偷换模型。

这些客户端运行使用独立HOME、无真实MCP和用户配置，工具任务限定只读合成文件。它们证明已安装真实CLI及其协议/工具往返，不代表用户全部插件、自定义规则、写文件、执行命令或所有工具均可用。长会话为三轮完整history压力样本，不是数小时CLI会话、断线续传、自动压缩或跨进程恢复验收。

## Codex定位：区分本地限制与上游拒绝

第一轮Codex失败没有上游调用。实验relay原要求入站stream=true，但OmniRoute的Responses路径会向兼容后端发送stream=false，因而命中STREAM_REQUIRED。

参考指定dsh-workbuddy-connect现有做法，窄relay只将发给WorkBuddy的stream设为true，返回原SSE；**OmniRoute原生负责消费SSE和转换给Responses客户端，没有新增自有协议转换器**。随后两次真实Codex请求均到达上游，分别约40912/41038 bytes、13个function工具，HTTP400，错误类别CHANNEL_REJECTED。回执只保留类别，没有保存原始错误正文或应用错误码，不能补写为已直接观测11128。

又用纯本地mock复现同一Codex请求：实际角色已经是`system/system/user/user`，工具类型为function，tool_choice为字符串，Codex正常返回marker。因此无需再加developer→system映射，不能误把该映射当作修复。本轮也未确定究竟是哪段上下文/参数触发上游检查；没有增删提示词、移除工具、切模型、伪造成功或试探一系列绕过规则。

该结果的含义是：**Codex→WorkBuddy仍不能列为日常可用链路**。标准字段兼容与私有指纹规则应分开，不能因指定项目包含指纹改写就默认搬入Loadout。

## 凭证轮换与安全边界

使用指定项目的WorkBuddyAtRestKeyProvider和解密/parser函数；每次真正dispatch前读取加密桌面auth及现有binding，当前进程固定最初绑定账号。key helper由原组件缓存，不每次请求启动CLI。仅解密access token，refresh token不解密；不调用refresh、不写桌面文件或DSH凭证副本。

离线边界检查12/12通过；另有真实临时加密文件的5/5检查：

1. 同一文件重复读取只启动一次mock key helper。
2. 同账号原子替换access token后读取新token，不启动第二次helper。
3. protector key实际换代时解析新key；随后重复读取不重复解析。
4. 过期token拒绝；替换为有效新文件后恢复。
5. 合成auth文件不含测试access/refresh token的明文。

文件轮换测试共读取成功6次、token变化3次、helper调用2次，对应两个protector key。另覆盖账号/binding变化、半写损坏文件、模型边界、鉴权、同case拒绝后关闭出口、取消传到fetch、响应模型不符及上游SSE兼容。

真实桌面凭证声明的有效期很长；本轮读到的access token未自然改变。**真实OAuth刷新端到端仍未验证**，没有为了制造这个事件主动退出、刷新或重新登录。Node字符串也没有安全清零承诺；只保持普通磁盘不新增真实凭证副本。

针对本轮514个运行文件扫描选定真实WorkBuddy access token和商汤API key的UTF-8/UTF-16明文，匹配文件数0。此检查仅针对选定当前凭据及指定运行目录，不是对所有泄漏形式的形式化证明。

## 商汤真实验证与仍然未知的部分

选第一个已启用原账号，固定已约定的deepseek-v4-flash与官方token.sensenova.cn端点。使用046候选，读回20秒minTime、1并发、3rpm及原生有界等待设置；专用测试combo最多一次尝试，出口最多3次真实调用，首次拒绝后关闭。输出上限256 tokens，合成极短输入。

首个请求429，出口立即停止，实际仅1次。没有扩大号池或换模型寻找成功。**因此，本轮不产生真实“429后再次请求至少20秒”的新增证据**；这个修复仍以046完整后台的三协议20秒反例回归和25秒长hint测试为依据。

用户已确认额度正常，本轮不将429重新诊断为账号坏了或余额不足。一次429无法区分平台负载、其他同账号流量、风控或其他上游限制。进程/数据隔离也不等于上游账号配额隔离；正式环境没有被停用。继续试探整个池子不能证明窄适配可靠，故在此停止。

## 使用范围与下一步门槛

- 保留OmniRoute候选及046节流patch，现有结果没有要求退回全量自研。
- WorkBuddy可继续作为Claude Code/OpenCode受限文本/只读任务的隔离候选；仍不正式切换。
- Codex→WorkBuddy保留BLOCKED；不新增私有提示词/指纹清洗。
- 商汤真实通路保留BLOCKED；等待明确可用窗口或上游状态变化，再用同一固定账号/模型的小预算验收，不能把mock通过称为风控解除。
- 真实token轮换保留NOT_OBSERVED；有自然轮换事件再补证，不能通过修改正式登录状态制造通过。

连接继续使用缓冲SSE，256KiB请求、1MiB响应、65秒单请求超时、单并发；不宣称实时token流或无限长请求。headerMs是上游响应头耗时，不当作模型TTFT。

## 隔离与证据

真实客户端原配置、WorkBuddy auth/binding文件hash前后一致；商汤源配置metadata前后一致。没有新增正式client key、替换安装、改变生产后台或推送提交。包含后续纯mock Codex实例的最终清理检查覆盖累计42个实验实例，残留凭据0、owned后台进程0。

回执及可重放命令见phase8 README。核心文件：

- `evidence/workbuddy-live.json`：第一轮完整矩阵。
- `evidence/workbuddy-codex.json`：SSE字段适配后的真实拒绝。
- `evidence/codex-shape.json`：纯mock请求形状和正常Codex结果。
- `evidence/sensenova-live.json`：一次429与停止。
- `evidence/offline.json`、`rotation-files.json`：17项离线检查。
- `evidence/audit.json`、`cleanup.json`：凭据扫描、候选完整性与清理。

没有因为失败而把测试删掉、放宽账号/模型边界或增加后台补丁。
