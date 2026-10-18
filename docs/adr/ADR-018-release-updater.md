# ADR-018: Loadout stable release updates

Status: Accepted, 2026-10-18

Core后台每6小时检查固定SeaserL/Loadout的GitHub latest正式release。只接受高于当前三段数字版本、同tag下命名匹配的Windows x64 zip与sha256；HTTPS固定GitHub下载源、有限重定向至GitHub资产CDN、超时和体积上限。校验SHA256后缓存，不执行远端脚本。SHA256不是独立签名，信任边界是仓库发布权限与HTTPS。

UI经IPC读取状态、手动检查/安装；settings控制自动检查、下载和安装，默认检查/下载启用，自动安装关闭。安装会停止并排空daemon请求并重启，开启自动安装意味着授权该行为。测试隔离daemon不自动联网。开发目录不自动覆盖，只允许标准LOCALAPPDATA/Programs/Loadout安装目录。

安装助手为随当前应用编译的固定PowerShell源码，spawn_detached启动，仅提取白名单文件，检查zip内路径、大小和hash，不执行包内install.ps1。备份旧二进制，失败时恢复；用户数据库和凭据不搬移，旧数据schema向后兼容仍需各版本迁移验证。

OpenCode保护补充ADR017：当前正式发布不启用双worker。全桥接串行执行、最小间隔120秒，识别原生403后暂停30分钟并持久化冷却截止时间；本地429带Retry-After。禁止自动恢复探针，冷却到期只放行下一次用户请求；后台元数据和官方title本地hook不计为用户推理。上述是保守策略，非已验证的上游配额。

Private repository authentication: current SeaserL/Loadout visibility is private. Core reads only the existing `git:https://github.com` Windows credential, serialized with other Credential Manager operations, with no writes or interactive login. The token is wrapped in Secret and sent only to this repository's api.github.com endpoints. Binary asset API requests use octet-stream; cross-host CDN redirects strip Authorization. Missing/expired login yields a visible error and no install. Default tests never read this credential; the ignored, explicitly opted-in GitHub download test does so without changing installation or making model calls.
