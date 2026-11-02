# 融合发布入口

该流程构建和验证 1.0（后台、控制台、桌面窗口），不安装、不启用 8317 或自启、不读取 0.4 的凭据，不提交上传。

后端不在仓库里放上游源码：`backend/` 记录相对固定 OmniRoute 版本的补丁和 Loadout 自己的文件。`python scripts\fusion\backend-source.py prepare` 还原工作副本，改完用 `export` 写回，`assemble.mjs` 打包前用 `check` 确认两边一致。见 [backend/README.md](../../backend/README.md)。

只要一个能运行的包，不跑回归：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\fusion-release.ps1 -Action package -Node <Node v24.22.1 的 node.exe>
```

需要先准备后端工作副本（并在其中 `npm ci --ignore-scripts`）和 `development/fusion/tools/`（`npm ci`）。完整的 `release` 动作还要一个已有候选作为升级基线，所以只能在已有候选的机器上跑。

## 开发者入口

在仓库根目录执行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\fusion-release.ps1
```

顺序：控制台构建 → 核对当前候选SHA → 临时展开升级基线 → 固定源码backend-only构建 → 装配与清单 → ZIP逐项解压读回校验 → 同一成品集中回归 → 测试凭据/进程/源码一致性审计 → 正式旧库只读元数据预检 → 实际UI审查门槛 → 推广唯一候选并清理本次临时产物。

ZIP 校验核对成员集合、重复路径和每个文件的 SHA-256（包括解压 CRC），随后测试与 ZIP 内容逐字一致的装配目录；不再常驻第二份解包副本。UI 审查记录为工作区 `evidence/ui-review.json`，必须标注相同 releaseId、真实操作和截图，不能仅用构建通过替代。缺少记录时发布入口停在推广前，可审查后用 `-Action verify` 继续。

当前候选的位置和SHA见 `development/fusion/current.json`；ZIP放在 `dist/fusion/`，精简证据放在 `development/fusion/evidence/`。旧包仅在升级测试期间临时保留。失败不推广、不删除旧候选；失败工作目录保留，核对停止状态后再处理，不能把整个开发目录当成缓存删除。

验收用的编译版展开在 `dist/fusion/app/`（`current.json` 的 `runtime`），仓库根目录的 `Loadout.lnk` 以 `--instance development/fusion/isolated/loadout-fusion-isolated/main` 启动它。快捷方式含绝对路径，不进版本库。被替换的旧候选和工作目录由 `scripts/fusion/cleanup-superseded.ps1` 手动清理，可先加 `-WhatIf` 查看；它保留当前包、源码、证据、隔离数据和凭据。

只重新做实际预检：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\fusion-release.ps1 -Action preflight
```

默认旧库 `%LOCALAPPDATA%\Loadout\data\loadout.db`，可用 `-SourceDb` 指向另一个旧库。此操作会临时解包现有候选，仅读取账号/旧池/端点/客户端策略的元数据，不启动网关、不读vault、不发网络请求、不迁入。`metadataStableDuringRead` 比较两次元数据投影，并非证明整个正在使用的正式数据库字节未变化。

已完成构建/封包但验收中断的工作区可以从回归继续：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\fusion-release.ps1 -Action verify -RunRoot D:\Loadout\development\fusion\runs\RUN_ID
```

`verify`重新执行全部成品回归和预检，不修改候选工具。若修改了`fusion/`源码，审计会拒绝推广旧包，应重新装配并重做整包验收。源码、测试与工具必须在发布前冻结，不能用后来修改的源码冒充已验证包。

仅控制台/工具变化时，可在 `FUSION_ASSEMBLE_IN_PLACE=1` 下设置 `FUSION_REFRESH_FROM=<原构建工作区>` 运行 `assemble.mjs`：先完整验证旧清单，再比较编译时的上游 patch 和新增源码；一致才复用编译后台、替换静态文件与工具并重封清单。后端源码变化必须重编译。这个入口不绕过最终包回归和 UI 审查。

## 固定输入和维护边界

- OmniRoute唯一开发副本和revision集中在`scripts/fusion/config.mjs`；没有新增checkout。运行时补丁沿用057。
- Node固定为旧候选manifest的实际版本24.22.1。构建从已核验ZIP复用`node.exe`到`development/fusion/tools/`，不依赖PATH或宿主升级；`-Node`可显式指定同版本。对应官方LICENSE、来源URL及hash保留在notices。
- 既有构建依赖及锁文件移至`development/fusion/tools/`；不重复安装。SWC临时缓存使用受限ACL的独立`C:\loadout-fusion-swc-UUID`，只在编译期存在。不能放宽权限检查来绕过失败。
- WorkBuddy选定模块固定在`fusion/vendor/`，保留原MIT许可证及notice。活动回归在`tests/fusion/`，历史实验记录仅供溯源。
- 当前升级测试接受明确登记的 064、066 到 067 包对（同一OmniRoute v3.8.52）；换OmniRoute版本须另做完整适配升级验证。不能删除版本门禁来支持任意包。
- 发布构建沿用上游backend-only策略，UI临时stub后恢复；仍不宣称完整上游typecheck或全部原生单测通过。

## 如何读迁移预检

`activationReady`始终为false：该命令没有实际绑定或迁移权限。`structurallyReady`只代表提供的元数据计划能被既有编译器接受，不代表凭据有效。

- 旧池保留有序成员、模型和启用状态；不静默丢弃成员。
- 账号区分旧的只读订阅来源、尚未绑定、建议绑定但未核验、停用；真实API key/受控登录/桌面绑定沿用现有入口。
- `loadout/default|fast|realtime`和客户端引用的别名显式列出；不由fixture或当前某个“能用”的模型代填。
- 商汤429、CommandCode401和WorkBuddy自然轮换作为DEFERRED另列，不在预检中试调用、修账号或扩展客户端调试。

日常试用还需按这份清单完成实际绑定和明确别名，随后对有限范围的真实配置做使用与恢复验收。安装器、自启、更多API和新UI不是这些工作的前置条件。
