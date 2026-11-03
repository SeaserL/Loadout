# 070 — 首次启动修复与开机自启开关

## Goal
用户反馈三类问题，版本号暂不变：

1. 发布的 1.0.0 在干净电脑上首次启动必定失败（`TARGET_EXISTS`），且无法自愈。已在多台机器上出现，另一个 agent 的排查报告和我们的定位一致。
2. 界面：导入、导出 API 账号的图标对调了；内容区左侧的圆角让侧栏在角落露出一块。
3. 软件里没有开机自启开关。本机现在的自启是 `wscript` 启动一个脚本，在系统的“启动应用”里显示为 Windows Script Host，用户找不到。
4. 在 VPS 上用发布的 1.0.0 导入 API 账号文件后，账号页有账号，模型页是空的。
5. 模型页难用：启用、停用只能一个个点；建分组要在长列表里一个个拖；长分组不折叠，超出栏位；页内标签切换时整页抖动。

## Context
- `apps/fusion-desktop/src/main.rs`：窗口启动时先在实例目录里建 `webview`（WebView2 的配置目录），然后才调用 `tools/desktop.mjs prepare-session`。
- `fusion/runtime.mjs` 的 `Instance.create` 用 `safeFresh` 要求实例目录不存在。默认实例 `<包的上一级>\loadout-fusion-isolated\desktop-current` 第一次启动时，目录已经被窗口建好，于是拒绝创建。之后每次启动都一样。
- 测试不能启动真实窗口（`Loadout.exe` 单实例），`desktop-test` 只测了“先 prepare”的顺序，所以没抓到。
- ADR-002 已定：`Loadout.exe` 自启动为可选项，默认最小化到托盘。0.4 用 `HKCU\...\Run` 下的 `Loadout` 值；测试只用 `Loadout-Test-*`。

## Existing behavior
- 实例目录只有 `webview` 时，`prepare` 报 `TARGET_EXISTS`，窗口显示启动失败。
- `operation.lock` 只按进程号判断占用。关机时后台被直接结束，锁文件留下；开机后如果这些进程号被别的程序用上，`status` 一直是 `unavailable`，`start` 报 `INSTANCE_ALREADY_ACTIVE`。
- 没有自启开关；`Loadout.exe` 一启动就开窗口。

## Constraints
- 不改 OmniRoute 的选路和数据结构。
- 测试不写真实的 Run 键，不启动真实窗口。
- 只删除我们自己写的自启值；同名但内容不同的值不动。
- 版本号仍是 1.0.0，是否发 1.0.1 由用户决定。

## Architecture impact
- **实例目录的归属**：`webview` 是窗口在实例目录里的固定部分，现有实例都有它。建实例时承认它：目录不存在，或只含 `webview` 这个真实文件夹（不是链接）时可以创建；其他内容仍然拒绝，避免覆盖数据。比把配置目录挪出实例目录改动小，也不会让已有用户丢掉界面设置（主题、动画速率存在 WebView 的 localStorage 里）。
- **锁的归属**：锁里的进程号还要是 `node.exe` 才算占用（`tasklist` 查映像名，只在控制端口无响应时才查）。
- **开机自启**：`Loadout.exe --background` 只启动后台、放托盘，不建窗口；托盘或再次启动时才建窗口。控制面 RPC `system.autostart` / `system.set_autostart` 读写 `HKCU\...\Run` 的 `Loadout` 值，内容是 `"<包>\Loadout.exe" --instance "<实例>" --background`。注册表通过 PowerShell 读写，值经环境变量传入，输出强制 UTF-8，路径里有中文也不乱码。测试用实例配置 `autostartKey` 指向 `HKCU:\Software\Loadout-Test\<id>`，`runtime.mjs` 只接受这个前缀。
- 控制台：设置页“开机启动”开关；账号页图标；内容区去掉圆角。
- **导入**：`keys.import` 原来特意不拉模型（`sync_models: false`，注释说等用户在账号卡片上点刷新），没人会想到这一步，导入的账号全是 0/0。改为导入后立刻同步，失败的账号写进 `sync_errors`（字段早已存在，控制台也会显示）。已经这样导入过的用户，模型页空着时给一个“同步所有账号的模型”。
- **批量**：新 RPC `model.set_enabled_many`，整份目录只算一次、只写一次覆盖表。原来每个 `model.set_enabled` 都要把全部模型重算一遍，逐个点几十个很慢。
- **模型页**：行、账号标题、搜索栏各有勾选框，选中后出现批量栏（启用、停用、加入分组、新建分组）。新建分组对话框列出“多个账号都有”的同名模型，点一下选中它在所有账号里的副本，也可以搜索后全选；勾选后新建会带上这些模型，同名时自动起名。分组超过 6 个时折叠为前 5 个。
- **标签切换**：`main` 预留滚动条位置（`scrollbar-gutter: stable`），标签淡入去掉位移。原因：不同标签内容高度不同，滚动条出现或消失时整页横移；面板整体上移又带着左侧固定栏一起动。

## Files expected to change
`fusion/runtime.mjs`、`fusion/desktop.mjs`、`apps/fusion-desktop/src/main.rs`、`backend/src/lib/loadoutPhase1/control/{autostart.ts,index.ts}`、`apps/fusion-console/src/{api.ts,pages/Settings.tsx,pages/Accounts.tsx,pages/Help.tsx,i18n.tsx,styles.css}`、`tests/fusion/{desktop-test.mjs,control-test.mjs}`、`scripts/fusion/run-suite.mjs`、`scripts/fusion/finish.ps1`、文档。

## Implementation stages
1. 首次启动：`safeFresh` 允许窗口的 `webview`；`desktop.mjs` 传入。复现后再修。
2. 锁：进程号加映像名校验。
3. 界面两处小问题。
4. `--background` 和自启 RPC、设置页开关。
5. 还原构建环境（清理后需要 `backend-source.py prepare` 和几处 `npm ci`），重新构建、打包。
6. 测试、端到端、全量回归、审计。

## Tests
- `desktop-test`：实例目录里只有窗口建的 `webview\EBWebView` 时首次启动成功；有其他内容时仍拒绝；锁里的进程号属于别的程序时照常启动。
- `control-test`：自启开关写入、读回、关闭只删自己的值，别人的同名值保留；全程只用测试键。导入后账号直接有模型，不再需要手动同步。批量启停一次生效，未知模型名整批拒绝。
- `main.rs` 单元测试：`--background` 参数识别。
- 用发布的 1.0.0 包按“先建 webview”的顺序复现 `TARGET_EXISTS`，修复后的包同样顺序通过。

## Performance validation
不在请求路径上。`tasklist` 只在控制端口无响应且锁里的进程号存在时调用。

## Security considerations
自启值只由后台根据自身路径生成，RPC 不接受调用方传入的命令或键。测试键前缀固定。关闭时只删除内容等于我们命令的值。

## Migration considerations
无数据迁移。本机用 `wscript` 的临时自启，在用户打开新开关后被覆盖，`dist\fusion\start-backend.vbs` 随后删除。

## Known risks
- 开机自启要真实重启才能完整验证。
- `--background` 下后台启动失败时没有窗口，只能在托盘提示里看到原因。

## Definition of done
干净目录按窗口顺序首次启动成功；设置页能开关自启，系统“启动应用”里显示为 Loadout；两处界面问题修好；回归和审计通过。

## Progress
- [x] 首次启动：先用发布的 1.0.0 复现（只有 `webview\EBWebView` 时 `prepare` 报 `TARGET_EXISTS`，实例没建），再修。
- [x] 锁
- [x] 界面两处小问题
- [x] 自启
- [x] 导入后同步模型、批量启停、模型页多选和建组、长分组折叠、标签切换
- [x] 构建环境：清理后用 `backend-source.py prepare` 和三处 `npm ci` 还原，约 2 分半。
- [x] 升级测试的基线变成当前候选（067.1）后报 `UPGRADE_PAIR_UNSUPPORTED`：`backup.mjs` 的升级名单没有“067.1 到 067.1”。同一上游、同一配置的数据结构一致，放行这一组合，其余仍拒绝。以后每次发布都会用当前候选当基线，所以必须修。
- [x] 测试和回归
- [x] 用户验收通过（2026-11-04），新包已换到本机实例。本机的开机启动仍是临时的 `wscript` 脚本，等用户在设置里打开开关后删除。

## Verification
包 `fusion-v3.8.52-isolated-067.1-202c64223977`，2026-11-04：

- 用户报告的场景：新包解压到全新目录，实例目录里先建好 `webview\EBWebView`，再按默认实例运行 `desktop.mjs prepare`：成功，实例建好，窗口的配置目录保留。
- 全量回归 280/280（desktop 10、control 23），审计无残留；真实包更新端到端全部通过。
- 合成数据走查（无头 Edge，私有配置）：10 个“像 1.0.0 导入那样没有模型”的账号，模型页显示“同步所有账号的模型”，一次同步出 37 个模型；搜索后全选，批量栏“已选 9 个”，新建分组自动命名并“创建（9 个）”；9 个成员的分组折叠为 5 个加“还有 4 个”；“多个账号都有”列出 deepseek-v4-flash×9 等，点 glm-5.2 选中 8 个；批量停用 8 个一次生效；四个标签间切换时内容区宽度不变（1119 px）；导入、导出图标为下载、上传；内容区圆角为 0。
- 自启脚本对测试键写入、读回含中文路径的值一致；控制面测试证明真实 Run 键全程未变。
- Rust：`cargo fmt --check`、`clippy -D warnings` 通过，3 个单元测试通过。

没有验证：真实开机自启（要重启电脑），`--background` 下的托盘和窗口按需创建（测试不能启动 `Loadout.exe`），真实 VPS 上的导入（需要那台机器）。

## Decisions
- 修复方式选“承认实例目录里的 `webview`”，不挪配置目录：已有实例本来就是这个布局，挪走会丢界面设置，且要多一套迁移。
- 自启做在 `Loadout.exe` 本身，不用 `wscript`：系统“启动应用”按程序显示名称，用户才找得到。
- 报告里“显式实例路径用随机端口”不改：显式路径用于测试和高级用法，默认安装仍优先 8318。“停止后立即启动时无进度”不改：只在少见的时机出现，不影响正常使用。
