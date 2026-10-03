# ExecPlan 009: Desktop UI Refresh

- **Status**: Active
- **Created**: 2026-10-04
- **Phase**: 系统测试反馈（桌面端美观度与实用性）

---

## 1. Goal

项目所有者反馈：桌面端在美观度和实用性上都比较粗糙。参考 EasyCLIProxyAPI（router-for-me，MIT，Tauri 2 + React 19，与本项目技术栈几乎一致）的视觉与交互，重做 `apps/desktop` 前端，不改变 IPC 能力边界（ADR-002）。

参考资料（不在仓库内）：`%TEMP%\claude\D--Loadout\d355a0f2-5be7-4d5e-a1fe-33038599c681\scratchpad\gui\`

- EasyCLIProxyAPI 截图：`easy_zh_1..5.png`、`easy_en_1..5.png`（首页、OAuth、API 接入、用量、Agents）；
- 部分源码：`easy\`；
- Quotio Desktop 额度卡片截图：`quotiodesktop_quota.png`。

## 2. Decisions

- **只借鉴设计与交互模式，不整体拷贝文件。**
  - 它的 `styles.css` 有 235 KB 且由工具生成，只提取 design token 的取值思路。
  - 确需拷贝的小组件按 reference-map §11 记入 `THIRD_PARTY_NOTICES.md`（MIT）。
  - **不拷贝** provider / 客户端 logo（第三方商标）。
- **图标库**：`lucide-react`（ISC，已在许可证允许列表内）。不引入组件库，继续手写 CSS。
- **图表**：手写 SVG，小而够用，不引入图表库。
- **Design tokens**（`:root` 和 `[data-theme=dark]`）：
  - 浅色：暖白背景、白色卡片、细边框、12px 圆角、柔和阴影、单一强调色（teal 系）；
  - 深色：一套完整的对应值。
- **主题**：浅色 / 深色 / 跟随系统三态切换，持久化到 localStorage（try/catch）；Tauri 窗口背景色同步，避免白闪。
- **字体**：中文优先 `"Microsoft YaHei UI", "Noto Sans SC"`，英文 `"Segoe UI Variable", "Segoe UI"`；URL 和 key 用等宽字体 `"Cascadia Code", Consolas`。中文界面不使用全大写小标签。
- **外壳**：
  - 浅色侧边栏（约 232px），图标 + 文字导航，当前项用强调色淡底；
  - 底部放 daemon 状态点、主题切换、语言切换。
- **daemon 未运行时**：导航保留，各页面显示锁定占位（说明 + "启动 daemon"按钮），不再整页空白。
- **首页**：
  - "运行状态"卡：状态 pill、PID / 版本 / 端口、启动 / 停止 / 重启（按现有 IPC 能力；没有的不做）；
  - "接入地址"卡：OpenAI / Anthropic / Responses 各一行，地址加复制按钮；client key 提示；
  - 下方放单值 KPI 磁贴。
- **账号页**：
  - 顶部 provider 网格（名称 + 账号数），下方是可搜索的列表；
  - 每行：启用开关、测速 / 同步按钮、图标按钮、删除走统一确认框；
  - 订阅账号用额度卡展示（每个窗口一条进度条，"剩余 %"按绿 / 黄 / 红着色，显示重置倒计时）；
  - ExecPlan 008 的"用量来源"下拉框保留。
- **统计 / 日志**：
  - 统一的筛选栏；KPI 行；按天的 token 堆叠柱状图（SVG）；token 构成条（input / output / cache）；
  - 日志页复用同一筛选栏。
- **通用组件**：Segmented tabs、Switch、ConfirmDialog（焦点陷阱、Esc 关闭，替换全部 `confirm()`）、Toast、CopyField、Icon button、Pill。
- **i18n**：所有新文案同时提供 zh 和 en；默认简体中文不变。
- **范围外**：新手引导向导、客户端一键配置，以后单独计划；Codex 订阅登录界面属于 ExecPlan 010，本计划只需预留"Codex 订阅"卡片位置。

## 3. Stages

1. Design tokens、主题切换、外壳（侧边栏、图标、锁定占位）、通用组件。
2. 首页、账号页（含额度卡）。
3. 统计、日志、模型、路由、客户端、设置页套用新组件。
4. 收尾：深浅色和窄窗口（最小宽度）检查、`npm run build`、`cargo build -p loadout-desktop`、许可证检查脚本、文档（README 截图说明可选）。

## 4. Tests

- `npm run build`（含 tsc 类型检查）通过。
- `python scripts\third_party_licenses.py --check` 通过（新增依赖的许可证在允许列表内）。
- 手动视觉检查：用隔离 daemon（`LOADOUT_PIPE_NAME` 指向测试 pipe）启动 `loadout-app`，截图检查深浅色各页面。**不要**在用户的真实实例上做 GUI 自动化。

## 5. Definition of Done

- [ ] §2 全部完成
- [ ] 构建、许可证检查通过；无新的 clippy 警告
- [ ] 本文件 Progress 更新，移入 `completed/`

## 6. Progress

- [x] Stage 1 — tokens, theme, shell, shared components（2026-10-04）
  - `styles.css` 重写：`:root` / `:root[data-theme=dark]` 两套 token（暖白 `#f6f7f5`、白卡片、`#e2e8e3` 边框、12px 圆角、teal `#0f766e` / 深色 `#5dd3c7`）；`theme.ts` 管理浅色 / 深色 / 自动，localStorage（try/catch），跟随系统变化。
  - 新 Tauri 命令 `set_window_theme`（`lib.rs`）：同步窗口背景色与标题栏主题；启动时先按系统主题设置窗口背景，避免白闪。不涉及 daemon IPC 与 bridge 允许列表。
  - `components.tsx`：Switch、Segmented、IconButton、Badge（pill）、Kpi、FilterBar、QuotaBar、CopyField、Toast（`useToast` / `useCopy`）、带焦点陷阱与 Esc 的 Modal、`ConfirmProvider` + `useConfirm()`（Promise 式，替换全部 `window.confirm()`）、LockedPage、ThemeSwitch。
  - 外壳：240px 浅色侧边栏、lucide 图标导航；daemon 未运行时导航项显示锁、页面显示锁定占位与"启动网关"按钮；底部为 daemon 状态、主题切换、语言切换。
  - 教训：页面占位的 `.locked` 类与导航项的 `locked` 状态类同名，导航项被套上了 `flex-direction: column`。状态类与组件类不要重名，占位改为 `.locked-panel`。
- [x] Stage 2 — dashboard, accounts（2026-10-04）
  - 首页：运行状态卡（状态 pill、地址 / PID / 版本 / 已运行 / 数据目录，停止 / 重启 / 刷新；重启 = 现有 `daemon_stop` + `daemon_start`）、接入地址卡（OpenAI / Responses / Anthropic 三行 + 复制按钮 + client key 提示）、KPI 磁贴、虚拟模型与账号额度两张卡。
  - 账号页：provider 网格（点击筛选）+ 搜索；每个 provider 一节（保留 ExecPlan 008 的"用量来源"下拉框），账号为卡片：启用开关、测速（对该账号已启用模型调用现有 `bench.run`）/ 同步 / 密钥 / 套餐图标按钮，删除走确认框；额度窗口用 QuotaBar（剩余 % 绿 / 黄 / 红 + 重置倒计时）。
  - "Codex 订阅"卡片位置：provider 网格末尾留了代码注释位，ExecPlan 010 在此加入登录卡片。
  - 视觉检查方法：浏览器面板 + vite dev server，注入临时的 `window.__TAURI_INTERNALS__` 模拟 IPC（fixture 数据，文件在仓库外的 scratchpad），无需在桌面上模拟点击。
- [x] Stage 3 — other pages（2026-10-04）
  - 统计：统一筛选栏（时间范围 + 分组 Segmented）、6 个 KPI（请求、成功率、输入、输出、费用、加权平均首 token 时间）、按天 token 堆叠柱状图（`charts.tsx`，手写 SVG + ResizeObserver，按 UTC 日期补齐空白天）、token 构成条、分组明细表。daemon 不区分缓存 token，构成条只有输入 / 输出，并在卡片里说明。
  - 日志：同一套 FilterBar（搜索、客户端、时间范围、条数、"只看失败"开关）；时间列当天只显示时刻，悬停显示完整时间。
  - 模型：搜索 + "显示已停用"开关 + 计数；启用改为 Switch；复制、删除（手动模型，走确认框）为图标按钮。
  - 路由：Tabs 改为 Segmented 放在标题旁；虚拟模型卡片中的路由列表带序号；Pool 编辑对话框用图标按钮排序，删除 Pool 新增确认框。
  - 客户端：新增"接入应用"卡（两个 Base URL + 复制），吊销走确认框；新密钥对话框用 CopyField。
  - 设置：自动启动与布尔设置项改为 Switch；外观卡加入主题切换；停止网关走确认框。
  - 教训：表格里的长模型名需要 `overflow-wrap: anywhere` 才不会把表格撑出卡片，但不能全局加（模型页的名称会被拆得很碎），改为按列使用 `.break`。
- [ ] Stage 4 — polish and verification
