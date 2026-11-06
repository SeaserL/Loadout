# 开发工作区

这个目录大多是本机工作材料，只有这几样进 Git：本文件、[upstreams.lock.json](upstreams.lock.json)（上游固定版本）、`fusion/tools/package*.json`（构建工具清单）、`fusion/notices/`（随包分发的许可证）、`fusion/ai-models.json` 和 `fusion/ai-pack/package*.json`（全量版 AI 组件包的模型和识字依赖，固定版本和哈希）。

| 目录 | 内容 | 进 Git？ |
|---|---|---|
| `upstreams/` | OmniRoute、Magpie、dsh-workbuddy-connect 的 checkout；后端工作副本在 `upstreams/omniroute/.claude/worktrees/loadout-phase11-v52`，由 `scripts/fusion/backend-source.py prepare` 创建 | 否 |
| `fusion/runs/` | 每次发布的工作目录和打包产物 | 否 |
| `fusion/isolated/` | 实例数据（SQLite、WebView） | 否 |
| `fusion/runtime/`、`fusion/legacy/`、`fusion/migrations/` | 历史运行包、旧控制台、迁移记录 | 否 |
| `fusion/evidence/`、`fusion/acceptance/`、`experiments/`、`source-deltas/` | 各阶段的验收证据和实验记录，含本机路径。结论写在 `docs/research/` | 否 |
| `exports/` | 导出的请求元数据 | 否 |
| `fusion/ai-cache/`、`fusion/ai-pack/node_modules/` | `scripts/fusion/ai-pack.mjs fetch` 下载的模型和识字依赖 | 否 |

产品源码不在这里：后端在 `backend/`，控制台在 `apps/fusion-console`，桌面壳在 `apps/fusion-desktop`，运行工具在 `fusion/`，测试在 `tests/fusion/`，发布脚本在 `scripts/fusion/`。
