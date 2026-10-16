# ExecPlan 018: 内置 API 平台预设（SenseNova 等 14 家）

- **Status**: Completed
- **Completed**: 2026-10-17（v0.3.0）
- **Created**: 2026-10-17
- **Related**: ADR-005（Provider 模块与文档）、ADR-003 / ADR-010（模型寻址与不换模型）、ADR-009（Tier A）

---

## 1. Goal

"添加账号"里的 API 提供方太少。新增 14 个内置提供方，用户选中后只需粘贴 API key：

- 国内：**SenseNova 商汤日日新**（`sensenova`，项目所有者明确要求）、Moonshot（Kimi API）、智谱 BigModel、阿里云百炼、火山方舟、硅基流动、MiniMax、阶跃星辰、百度千帆；
- 国际：xAI、Google Gemini API、Mistral、Groq、Cerebras。

## 2. Context

- 现有内置 API 提供方（`deepseek`、`openrouter`、`openai` 等）各占一个模块文件；新增平台大多只是"OpenAI Chat + 可选 Anthropic Messages + Bearer key"，逐个建模块会产生大量重复代码。
- 调研只用官方文档（每个平台的链接写在 `docs/providers/<slug>.md`）。另外用无效 key（`sk-invalid000`）探测了 `/models` 路由是否存在：千帆、MiniMax 路由存在（403/401 而非 404）；智谱、百炼、方舟先校验鉴权再路由，结果不确定。

## 3. Existing behavior

- `Registry::builtin()` 注册编译期模块；桌面端"添加账号"下拉框来自 `provider.kinds`，即注册表，按 `kind` 排序，无需改前端。
- `common::list_openai_models` 固定请求 `{base}/models`。

## 4. Constraints

- Capability-first：平台差异只在 `presets.rs` 的表里，不改公共接口（ADR-005）。
- 热路径：唯一的逐请求逻辑是 `prepare_request` 里的字段改名（纯内存，无 I/O）。没有改 Router / Gateway，无需 benchmark。
- Secret 只在 `Endpoint` 的敏感 header 中；测试 fixture 不含真实 key。

## 5. Architecture impact

新增 `crates/providers/src/presets.rs`：

- `ApiPreset`：描述符 + Messages 相对 origin 的路径 + 目录路径 + 文档模型列表（目录缺失时回退）+ `max_tokens` 字段 + 可选余额接口；`static PRESETS: [ApiPreset; 14]`。
- `Preset(&'static ApiPreset)` 实现 `ProviderModule`，注册表遍历 `Preset::all()` 注册。
- `common::list_openai_models_at`：允许自定义目录路径（硅基流动用 `/models?sub_type=chat`）。

## 6. Files expected to change

`crates/providers/src/{presets.rs,common.rs,lib.rs,registry.rs}`、`apps/loadout/src/bin/loadout/provider_cmd.rs`（帮助文本）、`docs/providers/*.md`（14 份 + README）、`docs/testing/system-test-plan.md`。

## 7. Implementation stages

1. 调研 SenseNova 与其他平台官方文档 ✅
2. `presets.rs` + 注册 + 单元测试 ✅
3. Provider 文档、README、系统测试计划、CLI 帮助 ✅
4. 用真实 key 验证 SenseNova（项目所有者）✅

## 8. Tests

`presets::tests`：
- `every_preset_is_consistent`：slug 唯一且等于 kind、默认 Base URL 合法、Chat / Messages URL 与鉴权头、Responses 拒绝、余额能力与 driver 一致；
- `sensenova_endpoints`、`sensenova_kimi_takes_max_completion_tokens`；
- `chat_model_filter`：按 id 与按目录元数据（`output_modalities`、`capabilities.completion_chat`、`active`）过滤；
- `catalog_filters_and_strips_prefix`（mock 上游；Gemini 的 `models/` 前缀）；
- `missing_catalog_falls_back_but_bad_key_does_not`：404 回退到文档模型，401 报错，无文档列表时 404 报错；
- `siliconflow_asks_for_chat_models_only`、`parses_moonshot_balance`、`moonshot_balance_uses_the_origin`。

`registry::tests` 增加 `sensenova` 固定 slug 断言。

## 9. Performance validation

不涉及 Router / Gateway；`prepare_request` 只对配置了模型前缀的预设（当前仅 SenseNova 的 `kimi-k3`）做一次字段改名。

## 10. Security considerations

- Messages 同时发送 `x-api-key` 与 `Authorization: Bearer`（各平台文档不一），两者都是 `secret_header`，`Debug` 脱敏。
- 余额请求只发往 Base URL 同 origin。

## 11. Migration considerations

无 schema 变更。新 kind 是新增行；旧数据不受影响。

## 12. Known risks

- 智谱、百炼的模型列表回退值取自 2026-10-17 的官方模型总览，会随新模型过时；目录接口可用时以目录为准。
- 火山方舟没有回退列表：`/api/v3/models` 不可用时同步报错，需要 `loadout model add` 手动添加。
- SenseNova 的 Anthropic 端点是否接受 `x-api-key` 只从文档推断；Loadout 同时发 Bearer，不依赖它。
- SenseNova Token Plan 的 5 小时 / 每周额度没有 API key 可读的接口，只能估算。
- 硅基流动余额接口已于 2026-08-14 下线，替代接口公布后再接入。
- 桌面端对固定 slug 的提供方不显示 Base URL 输入框，国际站需用 CLI `--base-url`。

## 13. Definition of done

- 14 个提供方出现在 `loadout provider kinds` 与桌面端"添加账号"中；
- `cargo fmt`、`cargo clippy --workspace --all-targets -D warnings`、`cargo test --workspace` 通过；
- 用真实 SenseNova key 完成模型同步与一次对话。

## 14. Progress

- [x] 2026-10-17 调研（官方文档 + 无效 key 路由探测）
- [x] 2026-10-17 `presets.rs`、注册、测试；fmt / clippy 通过
- [x] 2026-10-17 文档
- [x] 2026-10-17 真实 SenseNova key 验证（11 个账号，总 API 实测可用）

## 15. Decisions

- 表驱动预设而非每家一个模块：平台差异都是数据（URL、路径、模型列表、余额解析函数）。需要复杂逻辑的平台（如带额度窗口）仍单独建模块。
- 暂不接 Responses：各平台 Responses 的有状态语义不一（ADR-006），Chat 已覆盖；Responses 请求由网关转换为 Chat。
- 不导入 SenseNova 目录价格：币种与单位未文档化，宁可未知也不写错（ExecPlan 011 原则）。
- 未纳入：腾讯混元（旧平台迁往 TokenHub，域名写法不一）、Together（目录是裸数组）、Fireworks（目录是账号级接口）。
