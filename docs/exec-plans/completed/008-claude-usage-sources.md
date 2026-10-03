# ExecPlan 008: Claude Code Discovery Fix and Selectable Usage Sources

- **Status**: Completed
- **Created**: 2026-10-04
- **Phase**: 系统测试反馈修复（Provider / Discovery / Usage）

---

## 1. Goal

系统测试发现：只有 Codex 能读到用量；Claude Code 自动识别失败，也读不到用量。

根因（2026-10-04 在测试机上确认）：用户只通过 **Claude 桌面应用**（Code 标签页）使用 Claude Code。

- `claude` 不在 PATH 上：桌面应用自带的 CLI 位于 `%APPDATA%\Claude\claude-code\<version>\<hash>\claude.exe`。
- `~/.claude/.credentials.json` 不存在：桌面应用把登录保存在自己的加密存储里。Loadout 不读取、也不应读取该存储（Anthropic 禁止第三方收集或中转 Claude.ai 凭证，见 §2）。
- `discover()` 只看上述两处，因此返回 `None`；`fetch_usage()` 只有私有端点一条路径，同样失败。

但桌面应用会把订阅用量采样写入 `%APPDATA%\Claude\plan-usage-history.json`：

- 格式：`{version: 2, samples: [{t: <unix ms>, org: <uuid>, u: {fh: <5h %>, sd: <7d %>}}]}`；
- 应用运行期间约 15 分钟采样一次；无 reset 时间。

本计划交付：

1. Discovery 识别桌面应用和原生安装器（`~/.local/bin/claude.exe`）。
2. Usage 新增本地文件来源（桌面应用的采样文件），并保留原有私有端点。
3. 用户可按 Provider 手动选择用量来源，作为降低风险的开关。

## 2. Decisions

- **用量来源设置** `usage_source`（存于 `providers.config_json`）：
  - `auto`（默认）：有 CLI 凭证文件时走私有端点（原行为）；否则或失败时读桌面应用的本地采样。
  - `endpoint`：只用私有端点（`PRIVATE_ENDPOINT`）。
  - `local`：只读本地文件（`LOCAL_FILES`），**不联网、不读任何 token**，即低风险模式。
  - `off`：不读取用量。
- **为什么默认仍是 `auto`**：用户要求先修好原扫描功能，低风险模式作为可手动切换项。docs/providers/claude-code.md 记录各来源的风险，UI 中标明哪一项不联网。
- **截图 OCR 不采用**：桌面应用已经把同样的数字写入本地 JSON。OCR 需要应用停留在用量页并截屏，脆弱，而且会干扰用户操作（见 memory：不要在用户使用时做 GUI 自动化）。按 ccusage 方式从会话日志估算 token 用量，只能得到 token 数而不是额度百分比，记为后续可选项。
- **本地采样的时效**：
  - 采样早于窗口长度时（5h 窗口超过 5 小时、7d 窗口超过 7 天）丢弃该窗口，因为旧百分比已无意义。
  - 其余窗口的 Confidence 记为 `Estimated`。
  - 本地采样不含 reset 时间。
- **路径来源**：桌面应用目录取 `home\AppData\Roaming\Claude`，与测试隔离约定一致（不读环境变量 `APPDATA`，否则测试会读到真实用户数据）。
- **隐私**：`org` 是账号标识，不进入日志、错误信息或 fixture。
- **设置修改接口**：新增 IPC `provider.set_settings`（合并 JSON，`null` 删除键），以及 CLI `loadout provider set <slug> <key> <value>` 与桌面端的用量来源下拉框。修改后重新校验配置，并让用量轮询立即生效。

## 3. Stages

1. Provider：
   - `claude_code.rs` 的 discovery（桌面应用、原生安装器）；
   - 本地采样解析；
   - `usage_source` 分派；
   - 单元测试。
2. Control plane：`provider.set_settings`（IPC + core + dispatcher + desktop bridge）、CLI、测试。
3. Desktop UI：Provider / 用量页的用量来源选择，中英文 i18n。
4. 文档：docs/providers/claude-code.md、README、system-test-plan；在测试机上实测扫描、导入与用量读取。

## 4. Tests

- 只有桌面应用目录时：discover 返回 installed + logged_in，executable 指向自带 `claude.exe`。
- 本地采样解析：
  - 取最新样本；
  - 过期窗口被丢弃；
  - 空文件或损坏文件给出可读错误；
  - 错误与 Debug 输出不含 `org`。
- `usage_source = local`：在存在 CLI 凭证的情况下也不发网络请求（指向不可达 URL 的 HTTP client 不会被调用）。
- `provider.set_settings`：合并、删除键、未知 provider 报错、非对象参数报错。

## 5. Definition of Done

- [x] §1 全部完成，测试覆盖 §4
- [x] clippy / fmt / 测试 / 前端构建通过
- [x] 测试机实测：扫描能识别 Claude Code，导入后能看到 5h / 7d 用量
- [x] 文档更新；本文件移入 `completed/`

## 6. Progress

- [x] Stage 1 — provider（2026-10-04）
  - Discovery 识别 PATH、`~/.local/bin/claude.exe`，以及桌面应用自带的 `claude.exe`（取最新版本）；有桌面应用采样即视为已登录（`auth = desktop-app`）。
  - 新增本地采样来源：取最新样本，丢弃比窗口更旧的窗口，不保留 `org`。
  - `usage_source` 分派：`auto` / `endpoint` / `local` / `off`。
  - 私有端点的 base URL 可由 provider 的 `base_url` 覆盖，测试因此可以指向不可达地址，保证不访问真实服务。
- [x] Stage 2 — control plane（2026-10-04）
  - SDK：`ProviderModule::usage_sources()`、`ProviderConfig::usage_source()`、`USAGE_SOURCE_SETTING`。
  - IPC `provider.set_settings`：合并设置，`null` 删除键；按模块声明校验 `usage_source`；改完重建路由。
  - `provider.kinds` 返回 `usage_sources`；设为 `off` 时轮询跳过该 provider。
  - CLI `loadout provider set <slug> KEY=VALUE…`；`provider kinds` 增加 USAGE SOURCES 列。
- [x] Stage 3 — desktop UI（2026-10-04）
  - Accounts 页的 provider 卡片增加"用量来源"下拉框（自动 / 接口（联网）/ 本地文件（不联网）/ 关闭），中英文文案。
- [x] Stage 4 — docs and verification（2026-10-04）
  - 更新 docs/providers/claude-code.md（来源表、条款风险复核）、README、system-test-plan。
  - 测试机实测，使用隔离 daemon（临时数据目录、独立 pipe、`loadout-e2e-` 凭据前缀；只读取真实 home）：
    - 扫描得到 Claude Code（desktop-app）与 Codex；
    - 导入后 Weekly 89%（LOCAL_FILES / ESTIMATED）；5h 窗口因样本约 19 小时前而被正确丢弃；
    - 非法的 `usage_source` 被拒绝。
  - 发现：桌面应用只在运行期间的部分时段写入采样，5h 数据可能缺失。这是该来源固有的局限，已写入文档。

- [x] Follow-up（2026-10-04，v0.2.1）：用户的 daemon 仍然找不到 Claude Code。
  - 原因：Claude 桌面应用以 MSIX 方式安装，`%APPDATA%\Claude` 被重定向到 `%LOCALAPPDATA%\Packages\Claude_<id>\LocalCache\Roaming\Claude`。
  - 之前的"测试机实测"是从 Claude 应用内部的 shell 启动的，那里能看到重定向后的路径，所以结论是假阳性。
  - 修复：同时检查打包路径，取用量记录最近写入的那一个，并补了单元测试。
  - 教训：从 Claude Code 会话里验证"其他进程看到的文件系统"并不可靠，涉及 `%APPDATA%` 的结论要以用户自己启动的进程为准。
