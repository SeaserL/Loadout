# ExecPlan 051 — 本地开发产物清理

## Goal
按用户授权删除旧实验包、重复副本和不再需要的编译缓存，完整保留当前融合源码及全部未提交修改，不提交上传。

## Context
2026-10-31盘点约120.5GB去重文件数据。主要为Webpack/Rust缓存和阶段性完整包。用户要求长期只保留当前源码与实际使用的release。

## Existing behavior
OmniRoute有7个阶段工作副本；旧实验完整包至少16份。当前源码在phase11工作副本、fusion和实验脚本。当前桌面进程实际从dist/Loadout-0.4.0-windows-x64运行，该目录须保留。

## Constraints
不触碰已安装程序、正式数据、OS凭据、运行进程或其他任务。只删除清单内D:/Loadout的已核对绝对路径。不重建、不新增功能、不联网、不commit/push。

## Architecture impact
无运行时行为改变。旧worktree未提交内容以固定HEAD、完整binary diff和新增/修改文件原文保留为当前源码材料；当前phase11工作副本保留。

## Files expected to change
development/source-deltas/精简源码保全；development/experiments/cleanup-051/清单与校验；删除明确生成目录；更新来源/构建说明和本计划。

## Implementation stages
1. 核对进程、Git和生成路径，保存源码hash与未提交差异。
2. 删除旧包、旧build/cache、旧依赖副本；清理已保全源码的旧worktree。
3. 校验当前源码及未提交修改不变，记录空间变化和必要后续工作。

## Tests
源码文件SHA256前后比较；旧worktree差异可应用到其固定HEAD的临时index；保全新增文件逐字校验；当前Git状态及运行桌面路径保持。只做清理完整性检查，不跑产品回归或真实Provider。

## Performance validation
无运行时修改，不需要benchmark；记录D盘free前后和删除清单。并发系统写入可能影响free净变化，单独说明。

## Security considerations
不读取凭证；保留实验evidence与轻量runs记录。删除前检查祖先无reparse跳转；删除树中链接本身，禁止沿链接清理目标。

## Migration considerations
无正式迁移。历史测试hash保留，但被删归档不能再声称本地仍可下载。构建缓存需按需重建。

## Known risks
旧源码准备脚本依赖早期worktree，清理后应通过保全diff还原固定源码，不能照旧重跑历史抽取脚本。保留当前开发依赖和所需构建工具。

## Definition of done
明确目标均已删除或有具体保留理由；源码hash无意外变化；所有未提交修改可恢复；正式环境不变；释放空间可核对。

## Progress
- [x] 核对范围及活跃桌面进程，保留整个dist。
- [x] 保存并验证7个工作副本的全部未提交差异和原文件；固定基线的patch应用检查通过。
- [x] 执行17个明确目标的清理，保留当前phase11工作区、当前依赖和整个dist；未停止桌面进程。
- [x] 28731个保留文件SHA256无变化，当前融合工作区Git状态不变；目录统计与状态文档已完成。

清理耗时约10分钟，D盘可用空间由101410603008增至219329744896 bytes，净增加117919141888 bytes。磁盘净变化包含同期间其他系统写入的影响，最终项目剩余量另以after.json记录。产品源码复核完成后仅补充本次文档状态说明，没有重新构建或运行真实Provider/客户端。

最终项目去重文件数据6285724755 bytes（约6.29GB，统计点后说明文档有少量增量），253904个文件；此前约120.53GB。剩余development约3.58GB（当前源码、依赖、基准Git仓库与参考源码），experiments约2.16GB（工具依赖、回执和保留的隔离测试记录/状态），dist约0.41GB（当前有桌面进程使用，整体保留）。本轮不进一步删除测试记录或隔离客户端状态，避免把源码/日志保全扩大成客户端环境清理。

结果文件：删除清单、源码与空间复核、源码差异保全。所有新增材料仍是本地未提交文件。

## Decisions
保留当前phase11工作副本与一套开发依赖。旧未提交修改保留小型源码材料，不保留全量构建目录。当前及旧实验包均为工作产物，验收回执保留，包可删除后按需重建。
