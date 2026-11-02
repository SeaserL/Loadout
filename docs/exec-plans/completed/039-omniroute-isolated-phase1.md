# ExecPlan 039: OmniRoute第一阶段隔离验证

## Goal
实际验证独立后台、受保护凭证、Loadout账号路由规则能否通过少量集中适配成立，给出可复现结果与维护成本判断。

## Context
2026-10-30用户授权按038架构开始阶段B/C：不做UI、不动正式环境、优先确认适配面。固定OmniRoute v3.8.52，不更新到浮动版本。

## Existing behavior
038仅静态设计；已知上游可逆client key、明文回退、quota preflight与GUI退出联动不满足全部契约。Loadout正式源码和用户未提交改动保持原状。

## Constraints
仅在上游独立worktree和experiments/omniroute-phase1内修改；假凭证、mock loopback上游、随机端口、独立数据/home目录、测试凭证前缀。不得访问真实Provider、正式凭证、注册表自启或现有服务。允许为隔离测试安装依赖与构建；先禁用安装脚本再按需审核运行。无UI、无部署/提交/推送。

## Architecture impact
ADR-019授权范围扩展为隔离B/C试点；正式替换仍为Proposed。保留上游唯一选择与执行链，不通过自写router伪造兼容。区别组件测试、handler集成与真实后台E2E。

## Files expected to change
本计划、ADR-019范围注记、development/experiments/omniroute-phase1/**、报告、reference-map/notices；上游worktree的集中补丁保存为可重放patch。原始参考checkout保持不改。

## Implementation stages
1. 建隔离worktree、工具链/安装/出站约束。
2. 原生基线跑独立后台与路由，建立真实mock对照。
3. 凭证、client key和有限路由适配；失败路径/重启/泄露检查。
4. 集成回归、微基准、补丁面与升级接缝评估。
5. 停止自有进程、清理测试凭证，记录可复现产物与结论。

## Tests
假账号a/b/c：有序/轮转、429跳转、账号白名单/空集、点名首选与strict pin、模型不变、无效client key、后台重启、OS store不可用、落盘与日志哨兵。真实上游调用为0。上游相关focused测试优先，完整矩阵未跑则明确说明。

## Performance validation
执行隔离路由/HTTP mock基准，记录测量范围；不冒充正式负载或全面性能通过。

## Security considerations
Node测试子进程使用隔离home/APPDATA/DATA_DIR，不继承用户凭证配置；拦截非loopback出站。SecretStore只接受本次loadout-test前缀；finally清理。无正文落盘作为硬断言。失败原文不会写进报告。

## Migration considerations
只验证最小接缝，不自动接受全部上游默认功能。安全/账号契约若需广泛核心fork，报告具体失败并限制后续投入，不用更多自研层掩盖。

## Known risks
依赖规模、Windows原生模块构建、Node内存不可可靠清零、同步凭证读写与缓存、路由实现模式差异、上游测试/后台任务可能默认联网。

## Definition of done
运行证据能区分通过/失败/未覆盖；补丁和测试可重放；清晰说明哪些规则原生支持、哪些小适配、哪些大改；正式环境无变更。

## Progress
- [x] 阅读038设计/相关ADR与上游入口、DB/路由规则；确认基准。
- [x] worktree与隔离测试工具链：固定上游、Node 24、loopback出站限制、独立home/data与测试凭证service。
- [x] 基线与最小适配：基线发现空范围放开/明文持久化；OS store、hash-only与首选账号接缝已实现。
- [x] 行为/安全/性能验证：最终18/18实验检查、62/62上游focused回归；原生发布包healthz 200/headless dashboard 404。SSE提前keepalive反例已通过局部关闭修正；保留原始失败证据。
- [x] 清理、报告与结论：所有自有运行进程停止、7条测试凭证清零、6个外置SWC测试缓存清理；参考源码与实验依赖保留。

## Decisions
- 使用已获准的固定v3.8.52作为base，无需再次询问分支。
- 上游实验遵循.claude/worktrees位置；父仓库保存原创脚本、patch和结果，避免唯一改动留在ignored目录。
- 优先复用上游已声明的keytar 7.9.0；本次实测Windows凭证库可用，避免立即新增Rust/Node绑定。仅实验API-key推理和受声明token字段，不代表全部OAuth/网页会话完成迁移。
- 2026-10-30实测SSE提前keepalive后仍重试/换号；不能把主要非流式用例通过当成请求生命周期完全兼容。

## Outcome

本次隔离实验完成，正式融合验收未完成。报告：[第一阶段验证](../../research/2026-10-30-omniroute-phase1-validation.md)。7个原文件接缝（24+/6-）加2个模块（131行），没有第二套路由器。主要功能的小补丁实现得到验证；低维护结论未成立。

v3.8.52静态重放通过，反向试套相邻v3.8.51时4个目标文件冲突或不存在。最终发送前探针0聚合但197条普通SQL，未满足内存快照边界。带补丁的源码Next dev停在instrumentation编译；原生发布包与适配薄宿主分别通过，不能合称完整融合包通过。

结论：保留正式Loadout，OmniRoute保留为候选；下一道门槛为真实适配发布包与前向跨版本运行回归，在此之前不做UI或迁移。完整协议、管理安全、凭证所有写入/导出路径及Node清零差异见报告。

