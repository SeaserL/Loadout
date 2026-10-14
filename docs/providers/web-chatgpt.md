# ChatGPT web page as a browser session (`web-chatgpt`)

- **Last Verified Date**: 2026-10-15（只针对本地 mock 页验证；真实 chatgpt.com 的 live test 待项目所有者执行，见 ExecPlan 017 Stage 8）
- **Module**: `crates/providers/src/web_chatgpt.rs`（站点知识）；`crates/browser`（浏览器、CDP、会话执行）
- **Decision**: [ADR-014](../adr/ADR-014-browser-session-transport.md)，[ExecPlan 017](../exec-plans/active/017-web-chatgpt-transport.md)

## Subscription Transport Status

**OPT-IN, `WEB_SESSION`**（Tier D，最低可信等级）。两层开关：

1. 全局设置 `web_session.enabled`，默认关闭；
2. 每个账号添加时做两步确认（勾选"我了解风险"，再输入账号别名）。

不进入 `loadout/realtime|fast|default`；只能用精确名称，或由用户手动加入 Pool。

## ToS Risk

OpenAI Terms of Use（<https://openai.com/policies/terms-of-use/>，2026-10-15 读取）在 "What you cannot do" 中列出：不得以自动化或程序化方式提取数据或 Output（"automatically or programmatically extract data or Output"）。

项目所有者于 2026-10-15 确认：完成风险确认后允许接入，风险由用户承担（ADR-014 §9）。

- 账号可能被限制、暂停或封禁，且没有预先通知。
- 使用可见窗口只能降低被反爬拦截的概率，不改变条款本身的约束。
- 对话会保存在 ChatGPT 的历史记录里，和用户亲手输入的一样。

## 原则（ADR-014 §2）

- 使用 Windows 自带的、**未修改的** Microsoft Edge，专用 profile，默认可见窗口。
- 不伪装：
  - 不修改 `navigator.webdriver`（远程调试时 Edge 自己设为 `true`）；
  - 不改 User-Agent；headless 时 UA 本来就带 `HeadlessChrome`；
  - 不做 TLS 指纹伪装，不用代理。
- 不处理验证页：出现验证页或登录失效时，会话进入"需要你处理"，由用户在窗口里自己完成。
- 不读取 cookie：CDP 方法白名单禁止 `Network.getCookies`、`Storage.*`、`Fetch.*` 等，Loadout 也从不读取 profile 目录里的文件。
- 页面的所有请求都由页面自己的脚本发出。Loadout 只做两件事：像用户一样输入文字，以及被动读取页面收到的答案流。

## 使用

```text
loadout config set web_session.enabled true             显示风险说明后开启
loadout account web add web-chatgpt main                风险说明 → "I understand" → 输入别名；随后打开专用窗口，在窗口里自己登录
loadout account web status web-chatgpt main             就绪 / 忙 / 需要你处理 / 冷却中
loadout account web show web-chatgpt main               把窗口带到前台
loadout account web recheck web-chatgpt main            登录或完成验证后，立即重新检查
loadout account web bind web-chatgpt main <对话链接>     指定默认对话（--clear 解除）
loadout account web config web-chatgpt main --mode stateless --min-interval 10 --daily-cap 150
loadout account transport web-chatgpt main off          停止路由（profile 保留）
loadout account remove web-chatgpt main                 关闭浏览器并删除 profile 目录
```

桌面端：账号页 →「ChatGPT 网页会话」。

模型名：

- `web-chatgpt/main/auto`：页面当前默认的模型；
- `web-chatgpt/main/<slug>`：页面模型列表里的其他模型，例如 `gpt-5`。

模型列表在页面加载时被动读取（页面自己请求的 `/backend-api/models`），读到前只有 `auto`。

## 顾问回路的用法

对 Loadout 来说，顾问回路就是一次普通 API 调用：本地程序把 agent 的输出作为 user 消息发给 `web-chatgpt/main/auto`，网页端的回答原样作为模型输出返回。

Chat、Messages、Responses 三个入口都可用，流式和非流式都支持。回路本身（提取下一轮 prompt、轮数、何时停止）由调用方实现。Loadout 不解析、不审核回答。

## 对话映射（ADR-014 §5）

| 模式 | 行为 |
|---|---|
| `persistent`（默认） | 多次请求接在同一段网页对话里。已有对话只输入最新一条 user 消息；新开对话时输入完整历史（展平）。 |
| `stateless` | 每次请求开一个临时对话（`?temporary-chat=true`），输入完整历史。 |

`persistent` 模式下用哪段对话：

1. 请求带显式对话 ID（`x-loadout-session-id`、`x-opencode-session`、`x-claude-code-session-id`、`session_id`、`conversation_id` 请求头，或请求体的 `prompt_cache_key`）时，用该 ID 对应的对话，首次出现时新开一段。这个映射只保存在内存中，daemon 重启后会重新开新对话。
2. 否则用账号的默认对话：用户绑定的对话，或第一次请求自动新开并记住的对话（保存在 `accounts.web_session_json`）。

system / instructions 只在新开对话时，以 `[System instructions from the API caller]` … `[End of system instructions]` 标记作前缀输入；已有对话里会被忽略。

## 降级与限制

- **只支持文本**：
  - 请求带图像或 tools 时跳过该目标；
  - `temperature`、`top_p`、`stop`、`reasoning`、`response_format` 不会被静默丢弃，而是拒绝（精确名称请求返回 400，说明是哪个参数）；
  - `max_tokens` 接受，但不执行（Messages 入口必带）。
- **只输出回答正文**：思考过程、搜索与工具的中间消息不输出。
- **用量**：没有 token 数，按约 4 字符 / token 估算（`ESTIMATED`）。
- **一次一个请求**：每个账号同一时间只处理一个请求。
- **最小间隔与每日上限**：默认 10 s、150 次 / 天，按 UTC 日计数，可用 `account web config` 修改。
- **冷却**：额度耗尽（429 带 `clears_in`，或答案流里的限额消息）时冷却到 reset；没有 reset 时间时冷却 1 小时。
- **需要你处理**：登出、出现验证页，或连续 3 次失败时，停止路由并发桌面通知；登录后点「我已处理」或运行 `account web recheck`。
- **用户正在使用窗口**：输入框里有文字，或页面正在生成回答时，请求视为忙，转向下一个目标（或返回可重试错误），不会覆盖用户的输入。
- **客户端断开**：点击页面的停止按钮，等页面回到空闲后才处理下一个请求。
- **不测速**：每次请求都会消耗网页额度，所以不做主动测速，延迟数据只来自真实请求。
- **首字超时**：600 s（推理模型可能思考很久）。
- **窗口由 daemon 管理**：手动关闭窗口后，daemon 会在几秒内重新打开它（失败时退避，最长 5 分钟）。要停用，请关闭该账号的转发（`account transport … off`）或关掉 `web_session.enabled`；daemon 停止时窗口随之关闭。

## 技术说明

- **浏览器**：每个账号一个常驻的 `msedge.exe`。
  - profile 在 `<数据目录>\browser-profiles\<account id>`，DACL 只授权当前用户与 SYSTEM；
  - 进程放在 daemon 持有的 Job 里（`KILL_ON_JOB_CLOSE`），daemon 退出时浏览器随之结束；
  - 关闭时先发 `Browser.close` 让 Edge 写回 profile。
- **CDP**：`--remote-debugging-pipe --remote-debugging-io-pipes=<r>,<w>`，只用匿名管道，不开 TCP 调试端口，其他本地进程无法连接。子进程只继承这两个句柄。
- **后台节流**：启动参数关闭后台节流（`--disable-background-timer-throttling` 等），窗口最小化时流式输出不应停顿（待 live test 确认）。
- **页面识别**（`PAGE_STATE_JS`，固定的只读表达式）：
  - 输入框 `#prompt-textarea`；
  - 发送按钮 `[data-testid="send-button"]`，停止按钮 `[data-testid="stop-button"]`；
  - 登录按钮 `[data-testid="login-button"]` 或 `/auth` 路径视为登出；
  - Cloudflare 标题或 `#challenge-form` 视为验证页。
- **输入**：聚焦输入框，`Input.insertText` 一次插入，再点击发送按钮（找不到时按 Enter）。
- **答案流**：页面发出的 `POST …/backend-api/conversation` 或 `…/backend-api/f/conversation`。
  - 用 `Network.streamResourceContent` 被动读取；
  - 解析 `delta_encoding` v1（`{"p","o","v","c"}` 补丁与简写）和旧版累积消息；
  - 只取 `author.role == assistant`、`content_type == text`、`recipient` 为空或 `all`、`channel` 不是 `analysis` / `commentary` 的消息。
- **模型选择**：通过 URL 参数 `?model=<slug>`；slug 只允许 `[A-Za-z0-9._-]`。实际回答的模型从答案流读取，写入 debug 日志。

## 待 live test 确认

页面结构和流格式依据公开的社区资料整理（见 reference-map §10.12，只作为"页面收到什么"的知识），尚未用已登录账号核对。项目所有者 live test 时需要确认：

1. 选择器（输入框、发送 / 停止 / 登录按钮）；
2. `Input.insertText` 的多行文本在 ProseMirror 输入框里是否保持换行（mock 页中空行会多出一个换行）；
3. 答案流 URL 与格式；`?model=`、`?temporary-chat=true` 是否仍然有效；
4. 不伪装的可见窗口能否稳定通过 Cloudflare（headless 预计会被拦截）；
5. 窗口最小化时流式是否停顿；
6. 常驻 Edge 的内存占用。

结论写回本文件并更新最后验证日期。

## References

- ChromeDevTools/devtools-protocol（BSD-3-Clause）：方法与事件定义。
- chromedp `pipe_windows.go`（MIT）：Windows 上传递管道句柄的做法（只参考概念，未复制代码）。
- reference-map §10.12 中的社区项目：只作为概念参考；它们使用的伪装、cookie 复用、页外令牌生成等手段被 ADR-014 §2 禁止。
