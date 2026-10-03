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

- [ ] Stage 1 — tokens, theme, shell, shared components
- [ ] Stage 2 — dashboard, accounts
- [ ] Stage 3 — other pages
- [ ] Stage 4 — polish and verification
