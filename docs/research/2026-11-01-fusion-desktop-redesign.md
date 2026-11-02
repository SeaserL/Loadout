# 065 — 桌面控制中心重做记录

## 当前结果

新界面及独立桌面入口已实现。桌面EXE约12MB，使用现有Tauri/WebView2，不复用旧正式Bridge，也不新增Electron或第二套网关。
根目录`Loadout-Fusion-Preview.cmd`负责复用/启动059隔离后台、开发静态资源服务和独立桌面窗口；仅本机，独立WebView profile，不安装、不写自启/快捷方式。
这仍是依赖当前源码与已展开后台的开发预览，不是新的完整Release。唯一064 ZIP保持不变。

## 功能与来源

| 部分 | 采用方式 | 当前边界 |
|---|---|---|
| 丝带流向 | 参照Magpie masking截图的细线束、灰色入段/紫色出段、中心节点；原创React/SVG图表 | 使用原生模型/账号聚合，支持输入/输出筛选与节点明细；不捏造客户端关联，不称实时trace |
| 分区导航 | 按OmniRoute接入、路由、模型资源、观测和运行职责拆分 | 12个入口；无装饰性空按钮 |
| 账号 | 既有原生Provider API | 改名/凭证更新/显式模型同步；后台安全规则不变 |
| 账号池/别名/客户端 | 原生combos、model-combo-mappings、keys | 展示绑定、精确模型和授权；暂不提供变更范围的UI |
| 免费目录/Radar | 原生catalog/status/sync与过期判断 | 显示抓取时间/源日期，显式同步，不维护第二份名单 |
| 价格 | 原生pricing与sync | 来源/日期/检索/同步；缺失价格不当免费 |
| 配额/预算 | 原生quota/provider-limits/budget | 真实额度缓存单列来源日期；不把队列推算百分比当余额；确认后修改每日预算，保持其他周期和账号权限 |
| 用量/诊断 | 原生usage/history/call-logs/route-explain | 只读元数据；重算候选不冒充历史执行轨迹 |
| 桌面入口 | Loadout既有Tauri依赖 | 独立应用标识、profile、托盘代码；生产daemon Bridge完全不进入此入口 |

## 验证

- 前端TypeScript/Vite构建通过；Rust指定binary构建、clippy -D warnings、cargo fmt通过。
- 桌面目标校验单测覆盖有效loopback、正式8317拒绝、远程/非loopback拒绝、推理路径拒绝、URL凭据/query/fragment拒绝。
- 9项API专项通过：登录、Origin/Host/cross-site、CSRF、禁止推理代理、原生价格/配额契约、预算其他周期和密钥scope不变、未知配额不伪装真实余额。
- 实际浏览器操作：登录、12入口、输入token守恒（4500+4800=9300）、节点高亮/明细、账号改名、确认模型同步并读回2模型、预算确认保存、价格检索/取消同步、Radar未知日期/空筛选、请求诊断。
- 自动操作仅使用本地合成fixture；未发真实Provider请求。真实059隔离元数据hash仍`86c176b2f83f70ae7049fd7420b8c5195328aebfb93e63dfcbf94d1aac1e5cca`，16账号/8路由/9密钥/3映射，call_logs仍1434。
- 唯一064候选SHA256仍`4bc3cb24fec6ec1b27edfdf044132b5999f5443eb89efc3a28806749e372ebe0`。
- 许可证检查0违规；扫描仍报告7个无独立license文件的既有条目，未把这一结果写成所有依赖均完成归档。

证据：`development/fusion/evidence/065-desktop/`；视觉审查见根目录`design-qa.md`。

## 未完成与下一步

桌面启动两次被自动审批拒绝（仅返回blocked by policy，无具体理由）。用户明确暂不方便手动启动，未换工具绕过。因此窗口显示、托盘隐藏/恢复/退出实跑以及启动脚本端到端仍未验收，不能宣布完整桌面版本可用。

下一步先实跑桌面窗口和托盘，再将已通过的界面和独立桌面壳纳入一个便携包，移除开发期Vite代理，统一做同源包回归/升级。无需继续堆后端功能。默认别名、订阅转发和GPT池继续停用；外部账号问题仍单列。

## 本轮产物和清理

合成fixture已停止并清理其测试vault，56605合成界面服务已停止。真实059后台56603保留，开发静态资源服务56604保留供桌面启动入口复用。当前候选ZIP仍只有1个。

最终桌面EXE保存在`.build/fusion-desktop/LoadoutFusion.exe`，约12MB。尝试删除已复制EXE后的专用编译目录`.build/fusion-desktop-target`被自动审批拒绝，只返回blocked by policy，未提供具体原因；没有换工具绕过。因此约2GB编译缓存仍留存，不声称已经清理。精确字节数及EXE SHA见`development/fusion/evidence/065-desktop/cleanup.json`。
