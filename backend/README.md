# 后端

Loadout 1.0 的网关是 [OmniRoute](https://github.com/cheaper-inference/OmniRoute) v3.8.52（`ad6f33f66bda`，MIT）加上这里的改动。仓库不放上游源码，只记录差异：

| 路径 | 内容 | 许可证 |
|---|---|---|
| `omniroute-v3.8.52.patch` | 对 27 个上游文件的修改（+272/-32） | 上游部分 MIT，见 `LICENSE-OmniRoute.txt` |
| `src/lib/loadoutPhase1/` | 账号策略、凭据存储和控制面（`control/`） | Apache-2.0 |
| `src/app/api/loadout/` | 控制面的 HTTP 入口 `POST /api/loadout/rpc` | Apache-2.0 |

## 补丁改了什么

| 范围 | 文件 | 目的 |
|---|---|---|
| 凭据 | `src/lib/db/encryption.ts`、`db/apiKeys/secretAtRest.ts`、`db/providers.ts`、`db/providers/deletion.ts`、`api/keys/[id]/reveal`、`api/settings/{export,import}-json`、`src/instrumentation-node.ts` | 密钥只进 Windows 凭据管理器，数据库只留引用；客户端密钥只存哈希；关掉明文查看、导出和导入 |
| 选路 | `src/sse/handlers/chat.ts`、`sse/services/{auth,credentialSelectionOptions}.ts` | 号池、总 API、`provider/account/model` 寻址：只收窄账号范围和设置首选账号，选路仍由 OmniRoute 执行 |
| 流式 | `api/v1/{chat/completions,messages,responses}` | 流式请求走普通路径，不走上游的保活包装 |
| 节流 | `open-sse/services/rateLimitManager.ts` | 429 之后保持同账号的最小间隔 |
| 订阅转发 | `open-sse/executors/default.ts`、`open-sse/handlers/chatCore.ts` | 逐账号开启的 Codex 订阅：请求头、请求规范化和强制流式 |
| 实时视图 | `open-sse/handlers/chatCore.ts`、`chatCore/streamingTail.ts`、`src/lib/usage/{usageHistory,pendingRequestScope}.ts` | 进行中的请求带上客户端名和已传字节数，不含内容 |
| 请求记录 | `src/lib/db/cleanup.ts` | 定时清理遵守 `CALL_LOG_RETENTION_DAYS`，否则 30 天前的记录会被删 |
| 入口 | `src/proxy.ts`、`next.config.mjs` | 管理入口白名单和 Host/Origin 检查；同一地址同时接 OpenAI 和 Anthropic 路径；总 API 的 `/e/<名称>/v1` |
| 安全默认 | `src/lib/db/settings.ts`、`api/providers`、`api/radar/settings` | 固定不可改的安全设置；新建账号时不自动探测上游；Radar 不接受赞助密钥 |
| 构建 | `tsconfig.json` | Next 构建时写回的类型路径和排版，没有功能改动 |

## 怎么用

```powershell
python scripts\fusion\backend-source.py prepare    # 按固定版本克隆上游，创建工作副本，应用这里的改动
python scripts\fusion\backend-source.py check      # 工作副本和这里是否一致
python scripts\fusion\backend-source.py export     # 把工作副本的改动写回这里
```

工作副本在被忽略的 `development/upstreams/omniroute/.claude/worktrees/loadout-phase11-v52`，构建从那里读。在那里改代码，改完运行 `export`。`scripts/fusion/assemble.mjs` 打包前会运行 `check`，不一致就拒绝打包，所以打出来的包总能对应到仓库里的源码。

这里的 TypeScript 文件依赖 OmniRoute 的内部模块，不能脱离工作副本单独编译。

`*.patch` 和 `src/` 在 `.gitattributes` 里标了 `-text`，换行符逐字节保留，否则补丁在 Windows 上应用不了。

换上游版本不是重新应用一次补丁就行：要另做完整的适配和升级验证，见 [ADR-019](../docs/adr/ADR-019-product-fusion-boundaries.md)。
