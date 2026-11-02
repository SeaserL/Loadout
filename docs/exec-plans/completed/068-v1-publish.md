# 068 — 1.0.0 发布到 GitHub

## Goal
用户验收 067 后要求：同步版本号，作为大版本上传到 GitHub，并完全重做 README 的功能展示。

## Context
仓库公开。工作区里 1.0 的全部代码都没有提交过，后端补丁只存在于被忽略的上游工作副本里，`development/` 混着本机路径和验收证据。

## Existing behavior
Rust 线 0.4.0，Fusion 桌面壳 0.5.0，设置页显示 `3.8.52 · 067.1-<hash>`。README 以 0.4 为主。

## Constraints
上传到公开仓库的内容不能有密钥、真实账号名、本机路径。截图只用合成数据。不动本机 0.4 安装和 8317。重启用户正在运行的 Loadout 要用户当场同意。不发 Release（用户没有要求，且 0.4 内置更新器每 6 小时查 Release）。

## Architecture impact
- 版本号统一为 1.0.0，来源是 `Cargo.toml`、两个 `tauri.conf.json` 和 `apps/desktop/package.json`。包清单新增 `productVersion`，supervisor 传给后端，设置页显示 `1.0.0 · OmniRoute 3.8.52`。`adapterProfile` 仍是 `067.1`：它是升级门禁的内部标识，不面向用户，改它要重做升级登记。
- 新增 `backend/`：相对固定 OmniRoute 版本的补丁加 Loadout 自己的文件。`scripts/fusion/backend-source.py` 负责 `prepare`、`export`、`check`，`assemble.mjs` 打包前强制 `check`。
- `fusion-release.ps1` 新增 `-Action package`：不需要升级基线，给干净机器用。
- ADR-019 改为 Accepted，范围写在 ADR 里。

## Files expected to change
版本文件、`backend/`、`scripts/fusion/`、`fusion/supervisor.mjs`、`tests/fusion/control-test.mjs`、`.gitignore`、`.gitattributes`、`README.md`、`docs/`、`AGENTS.md`、`THIRD_PARTY_NOTICES.md`、`development/README.md`。

## Implementation stages
1. 盘点会上传的内容，扫描密钥和个人信息。
2. 版本号和设置页显示；后端补丁导入 `backend/`，在全新工作副本上验证。
3. 收紧 `.gitignore`：`development/` 只留 README、锁文件、工具清单和许可证。去掉指向本机证据的文档链接。
4. 重新构建，合成数据截图，重写 README。
5. 全量回归、Rust 检查、干净检出构建、再次扫描。
6. 提交、推送。

## Tests
`tests/fusion/control-test.mjs` 断言设置页版本串。全量回归和审计。`cargo fmt`、`clippy -D warnings`、`cargo test --workspace`、许可证检查。`backend-source.py prepare` 在全新工作副本上还原后与构建用的副本逐字节一致。

## Performance validation
没有改请求路径和 Router，不需要跑 benchmark。

## Security considerations
发布前后各扫描一次：密钥格式、真实账号和客户端名（从本机实例读取后比对，不写入仓库）、本机用户名、邮箱。`development/` 里的旧证据含本机路径，不上传。

## Migration considerations
无数据变更。本机实例可以直接换包。0.4 的数据不受影响。

## Known risks
- 从零构建的步骤没有在干净机器上完整跑过，只验证了后端还原、Rust 工作区和前端的干净检出。
- 这份计划开始时 GitHub 上还没有 1.0 的发布包。后来按用户要求发布了，见下面的 Release 一节。

## Definition of done
main 上的 README 显示新内容；本机包显示 1.0.0；回归通过；扫描干净。

## Progress
- [x] 盘点和首次扫描。`development/` 之外没有命中。
- [x] 版本号和设置页显示。
- [x] 后端导入 `backend/`，全新工作副本验证一致。
- [x] `.gitignore`、文档链接、ADR、计划归档。
- [x] 截图和 README。
- [x] 回归和检查。
- [x] 提交，推送到分支 `release/v1.0.0`，开 PR。

## Verification
本机候选 `fusion-v3.8.52-isolated-067.1-d807f8fa38fc`，`productVersion` 1.0.0，`Loadout.exe` 文件版本 1.0.0。

| 项目 | 结果 |
|---|---|
| 集中回归（`run-suite.mjs`） | 275/275，18 组全部通过；控制面用例新增设置页版本串断言 |
| 审计（`audit.mjs`） | 297 个测试实例，残留 0，活动 0，源码不一致 0 |
| 后端记录 | `prepare` 在全新工作副本上还原后，补丁和新增文件与构建用的副本逐字节一致 |
| Rust 和前端 | `cargo fmt --check`、`clippy -D warnings`、`cargo test --workspace`、`npm run build`、`npm run fusion:ui`、桥接和更新合约、benchmark 编译、许可证检查（348 个包 0 违规）都通过 |
| 发布前扫描 | 密钥格式、邮箱（只有 Node 许可证里的）、本机用户名 0 处。本机实例的 24 个账号和客户端名：真实别名 0 处；命中的只有 `legacy-2`、`endpoint-sensenova-1` 这类迁移时生成的客户端名，和一个模型名里的 `ling` |
| 截图 | 来自合成实例，账号名是编的（`work`、`team`、`backup`）。地址栏里的随机端口和模拟上游地址，在截图前替换成默认端口和预设的真实地址 |

`loadout-browser` 的 `web_session` 有一次失败，当时和其他编译任务同时运行；单独重跑和完整重跑都通过，判断是负载下的偶发失败，没有深究。

未覆盖：从零构建的步骤没有在干净机器上完整跑过（README 写明了）；本机还在运行上一个候选 `c04d850e3c7d`，换包要用户同意。

## Release（2026-11-03）

用户要求先确认 0.4 能否覆盖更新到 1.0，确实不能就照常发 Release，并检查当前版本有没有更新器。

| 问题 | 结论 | 依据 |
|---|---|---|
| 0.4 能否覆盖更新到 1.0 | 不能 | 用 `packaging/update.ps1` 的测试模式安装真实的包：退出码 1，`Unexpected or duplicate archive path`，程序文件未动。脚本只接受三个固定程序和白名单文件，还要校验 `loadout --version` |
| 发布后 0.4 会怎样 | 更新页显示 `Missing or duplicate Windows release asset`，不下载，网关不受影响 | `crates/core/src/updates.rs`：先按固定名字找资产，找不到就报错 |
| 1.0 有没有更新器 | 没有 | 控制台过滤了 `updates.*` 设置，后端和桌面壳没有更新逻辑，`isolation-guard` 还拦截了 `api.github.com` |

发布前查到并修掉一个问题：不带参数启动的新实例第一次拿到的是随机端口，不是 README 写的 8318。之后 `supervisor` 会把端口存进 `instance.json`，所以它不会每次重启都变；提交 `e22719be` 的说明和当时的说法都夸大了这一点。现在新安装优先用 8318，被占用才换一个。

发布结果：`v1.0.0` 是 Latest，资产 `Loadout-1.0.0-portable-windows-x64.zip`（包 `4d5bfb8d0288`）和 `.sha256`。PR #1 的 CI 第一次因 `port_conflict_is_explained` 失败，重跑通过；合并后 `main` 上因 `web_session_answers_through_the_gateway` 失败。两个都是 Rust 线的旧测试，同样的代码在别的运行里通过，判断是 CI 机器慢造成的。

包内容扫描：没有本机用户名、邮箱、真实账号名、令牌或可疑文件名。`PRIVATE KEY` 只出现在库代码和文档里，没有真实密钥。编译产物里有 `D:\\Loadout` 这个构建路径，不是秘密，没有处理。

## Decisions
- 不把 `development/` 的证据传上去：约 58 MB，1700 多处本机用户名，对读者没有用，结论都在 `docs/research/`。
- 一开始没有发 Release：当时用户没有要求，0.4 的更新器又会看到它。用户确认升级路径后要求照常发布。
- 后端放顶层 `backend/` 而不是 `fusion/`：`fusion/` 会被整个复制进发布包。
