# 065 — 融合桌面控制中心重做

> 关闭于 2026-11-03：被 067 取代。用户否定了这一版界面，下面未勾选的项不再跟进。

## Goal
用户否定064的网页统计页呈现。交付可打开的隔离桌面窗口，以Magpie丝带流向为主视图，并将已经接通的OmniRoute能力分区组织为可操作管理流程。

## Context
064的247项后台回归与网页按钮验收并不代表桌面产品设计验收。唯一候选ZIP保留，正式安装/数据不变。参考现有本地Magpie截图masking-zh-light、routing-zh-light、usage-zh-light；OmniRoute固定v3.8.52为功能与API依据。

## Existing behavior
apps/fusion-console已有登录/CSRF、统计、账号更新、目录同步和路由只读。apps/desktop/src-tauri已有Tauri/WebView2/托盘；其默认启动正式daemon的逻辑不能用于本次隔离预览。

## Constraints
不访问Provider、不启用默认别名/订阅/GPT池，不提交上传，不生成候选ZIP副本。保留源码和既有未提交修改。当前真实隔离实例允许读取；写操作由用户在界面显式发起，自动验收使用合成fixture。自动探测保持关闭。

## Architecture impact
复用Tauri依赖，新增独立fusion binary/config，不进入旧Bridge/自动启动/通知/注册表代码。开发期Vite仅代理固定loopback后台管理API，拒绝外部Origin与推理路径；正式包仍同源静态页面，不增加第二套网关。独立WebView profile与托盘，关闭窗口隐藏、退出GUI后台继续运行。

## Files expected to change
apps/fusion-console/src；apps/desktop/src-tauri独立入口/config；scripts/fusion桌面预览入口；tests/fusion；ADR019与功能来源文档。

## Implementation stages
1. 对照实图、明确数据语义和桌面生命周期。
2. 重做流向与分区导航，复用原生模型/价格/配额/诊断API。
3. 编译隔离桌面入口，运行现有后台上的新版控制台。
4. 类型检查、凭证/Origin与桌面隔离测试、实际窗口交互和截图对照。

## Tests
流向聚合守恒/缺失记录不推断；原生读接口与失败态；CSRF透传和非本机拒绝；独立桌面配置/导航限制；桌面实际窗口、导航、筛选、编辑取消、托盘恢复。

## Performance validation
不修改Router/Gateway热路径；流图只消费已有聚合数据，限制可视节点并披露覆盖范围，不新增自动轮询或推理。

## Security considerations
不读取/打印正式凭据，不注入或绕过管理登录。开发桌面不暴露Tauri命令给远程内容；仅允许固定loopback页面。密码仍使用原生cookie/CSRF，不保存前端凭据。

## Migration considerations
本轮为桌面开发预览，不改变唯一064候选ZIP身份，不修改真实隔离实例的账号/路由及vault。后续封包前再统一升级验收，不能把开发预览称作已发布版本。

## Known risks
原生统计缺少完整客户端到模型关联，不能画假链路。部分原生功能在当前约束下只读。首次Tauri重建有临时编译占用，禁用增量与debug信息并只编译目标binary。

## Definition of done
隔离桌面实际启动；丝带流向及分区功能可见可用；真实后台只读、合成写测试通过；可打开入口和剩余限制清楚说明；唯一候选ZIP不变。

## Progress
- [x] Boot与参考实图核对；确认064产品呈现未通过用户验收。
- [x] 流向与细分管理页；删除重复导航，细线灰入/紫出，节点筛选与明细。
- [x] 独立桌面binary/config/profile/托盘与开发预览启动入口已编译，约12MB。
- [x] 浏览器交互与视觉复核、9项API安全/原生契约专项、Rust目标校验单测、clippy/fmt/TS构建通过；来源文档已更新。
- [ ] 实际桌面窗口/托盘及一键启动端到端：自动审批两次拒绝启动；用户暂不便手动启动，保持未验收。
- [ ] 单一完整桌面候选封包/升级：待原生桌面实跑后继续。原064 ZIP未动。
- [x] 合成测试实例/vault/56605预览服务已清理，真实后台及56604开发资源服务保留。
- [ ] 本次专用编译缓存清理：约2GB删除被自动审批拒绝，保留cleanup.json，不换工具绕过。

## Decisions
- 以Magpie多束细线穿过中心的图形为直接视觉目标，原生统计决定节点语义，不为视觉补造链路。
- 复用现有Tauri，不引入Electron运行时；隔离入口绝不调用旧production Bridge。

## 066 收口接续
完整桌面候选封包与原生启动已由 066 接手。实跑修复 Tauri 配置/单实例冲突；窗口隐藏/重开已核对，托盘/正常退出与截图仍待人工验收。当前状态以 066 计划及验收报告为准。
