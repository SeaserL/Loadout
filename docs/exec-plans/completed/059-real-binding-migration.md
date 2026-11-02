# ExecPlan 059 — 真实绑定与隔离迁移恢复

## Goal
沿用058候选与已有迁移/凭据/备份逻辑，补齐旧API-key账号、WorkBuddy绑定及精确别名；默认别名按用户决定禁用。完成可迁部分的隔离迁入和恢复核对，缺失成员不缩池，不伪造可用结论。

## Context
058发布及240项隔离验收完成；实际旧库21账号、2池、1商汤端点。用户授权真实绑定和隔离迁移，不改正式状态。058首次失败目录清理仍由原计划跟踪，本轮不重试被拒绝的删除。

## Existing behavior
snapshot/inventory/compile/applyReady负责元数据与原生Combo/key映射；原生admin/vault及backup/restore已验。旧API key通过既有CredReadW读取，仅私有子进程管道传输。Codex订阅已有受控设备登录，不能复制正在由正式后台刷新的refresh token。WorkBuddy沿用已绑定官方加密会话只读连接器。

## Constraints
不新增运行时功能/补丁、不改UI、不重打包、不提交上传。源数据库和OS凭据只读；目标仅唯一隔离实例及一次临时恢复实例，独立OS vault前缀。无真实推理、无账号故障排查。不得扩账号/换模型；缺失成员对应池保持阻塞。

## Architecture impact
无产品架构变化。仅迁移执行记录、必要编排脚本和映射配置；直接调用058包内既有接口。用户明确默认别名先禁用，受限客户端权限只允许收窄；未映射旧来源明确保留。

## Files expected to change
scripts/fusion/下本轮编排与只读私有输入桥接；development/fusion/下脱敏映射/回执；docs/research/验收及待决清单。fusion/和上游运行时保持不变。

## Implementation stages
1. 读取元数据，核对精确账号别名、凭据类型、模型与WorkBuddy绑定；列独立登录及配置决定。
2. 从当前ZIP临时展开，使用已有工具私有迁入可用绑定；保持出站隔离与默认别名禁用。
3. 原生映射/权限读回、加密备份与新目录恢复；不以mock替代真实凭据绑定，不向外部发推理。
4. 停止并审计、清理恢复副本及临时明文以外材料，保留唯一候选ZIP与必要隔离状态/脱敏证据；明确仍待决定和外部问题。

## Tests
源元数据指纹及读取凭据前后内存比较；精确别名账号/模型一致、旧池不缺成员、禁用默认不出现于已启用映射；恢复前后非秘密结构及内存中的vault值相等。验证未授权账号/模型和默认别名被拒，避免发送被允许的真实推理。包SHA与源码一致性沿用058，不重复全量回归。

## Performance validation
无Router/Gateway代码变更，不重跑性能测试；不触发真实测速。

## Security considerations
真实API key仅在私有管道、进程内存、目标测试OS vault与加密备份中存在；不输出、不入JSON/日志/fixture。官方token不复制，旧正式凭据不刷新不删除；不以空allowedConnections表示禁用。加密恢复口令临时存测试OS vault，不进文件；恢复验证完成后清理临时备份/恢复实例。

## Migration considerations
先尊重完整源清单。用户授权默认别名禁用，精确别名按源账号alias解析；不足以应用全量plan时明确记录阻塞，已有独立可用路由仅在显式记录的收窄投影上应用。Codex订阅缺独立登录时GPT三成员池不启用。

## Known risks
存入真实API key不等于上游鉴权成功。WorkBuddy官方文件可能由桌面自行轮换，记录变化不归因于本次写入。复制订阅refresh token会影响正式会话，因此沿用受控独立登录；所需用户操作不得假装完成。真实凭据与配置暂存隔离vault是试验状态，不是正式切换。

## Definition of done
可迁绑定与精确映射得到真实隔离存储/恢复证据；阻塞池及待用户项明确，默认不启用，不增加业务补丁；仅一个候选ZIP，正式环境不变。

## Progress
- [x] 完成文档/既有抽象核对；用户选择默认别名先禁用。
- [x] 15真实API key与WorkBuddy旧账号绑定核对，共16连接；旧空base_url沿用原Provider descriptor。
- [x] 8路由、精确白名单/8RPM、真实vault加密恢复及拒绝默认/GPT/越界模型，7组检查通过。
- [x] 原实例停止并保留；恢复副本凭据0、临时目录删除；候选SHA不变，报告及状态索引就绪。

## Decisions
- 本轮不复制旧Codex或官方CLI refresh token；需要独立受控登录才能保持正式会话不受影响。
- 默认别名禁用，不用DeepSeek或任何其他模型代填。缺成员池保持完整阻塞记录。

- 用户追加决定：旧订阅transport开关未开启，继续停用，完整GPT池暂不下发；因此本轮不发起登录或复制订阅token。
- 结果：指定范围完成，未真实调用上游；fullMigrationReady仍false，保留其余来源及用户停用项。详见2026-11-01-real-binding-migration.md。
