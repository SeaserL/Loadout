// SPDX-License-Identifier: Apache-2.0
//
// UI translations. Simplified Chinese is the default; the choice is remembered per user in
// the WebView's local storage (a display preference, not configuration). Messages coming
// from the daemon (errors, setting descriptions it does not know) stay in English.

import { Fragment, type ReactNode, createContext, useCallback, useContext, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export type Lang = "zh-CN" | "en";
export const LANGS: { id: Lang; label: string }[] = [
  { id: "zh-CN", label: "简体中文" },
  { id: "en", label: "English" },
];
const STORAGE_KEY = "loadout.lang";
const DEFAULT_LANG: Lang = "zh-CN";

const en = {
  // navigation / shell
  "nav.dashboard": "Dashboard",
  "nav.accounts": "Accounts & usage",
  "nav.models": "Models",
  "nav.routes": "Routes",
  "nav.clients": "Client keys",
  "nav.stats": "Statistics",
  "nav.logs": "Request log",
  "nav.settings": "Settings",
  "shell.running": "Gateway running",
  "shell.connecting": "Connecting…",
  "shell.stopped": "Gateway stopped",
  "shell.notRunning": "The Loadout gateway is not running.",
  "shell.notRunningHint": "Start it to manage accounts and serve requests. It keeps running when this window closes.",
  "shell.start": "Start gateway",
  "shell.starting": "Starting…",
  "shell.recovered": "The database was damaged and has been replaced by an empty one.",
  "shell.recoveredHint":
    "The damaged file was kept at {file}. Restore your configuration under Settings → Backup → Import, using the newest file in {dir}.",
  "shell.language": "Language",

  // common
  "common.cancel": "Cancel",
  "common.close": "Close",
  "common.save": "Save",
  "common.add": "Add",
  "common.remove": "Remove",
  "common.edit": "Edit",
  "common.enable": "Enable",
  "common.disable": "Disable",
  "common.enabled": "enabled",
  "common.disabled": "disabled",
  "common.done": "Done",
  "common.create": "Create",
  "common.yes": "yes",
  "common.no": "no",
  "common.copy": "Copy",
  "common.name": "Name",
  "common.left": "{n}% left",
  "common.resets": "resets {when}",
  "common.never": "never",
  "common.now": "now",
  "common.inMin": "in {n} min",
  "common.inHours": "in {h} h {m} min",
  "common.inDays": "in {n} d",
  "common.secAgo": "{n}s ago",
  "common.minAgo": "{n} min ago",
  "common.hoursAgo": "{n} h ago",
  "common.daysAgo": "{n} d ago",
  "common.showDisabled": "show disabled",
  "common.filter": "Filter",

  // tiers / states
  "tier.ULTRA_FAST": "Ultra fast",
  "tier.FAST": "Fast",
  "tier.NORMAL": "Normal",
  "tier.UNMEASURED": "Unmeasured",
  "tier.pinned": "pinned",
  "state.HEALTHY": "Healthy",
  "state.UNKNOWN": "Unknown",
  "state.RECOVERING": "Recovering",
  "state.DEGRADED": "Degraded",
  "state.COOLDOWN": "Cooling down",
  "state.RATE_LIMITED": "Rate limited",
  "state.QUOTA_EXHAUSTED": "Quota exhausted",
  "state.AUTH_FAILED": "Auth failed",
  "accountType.api_key": "API key",
  "accountType.subscription": "subscription",
  "accountType.local": "local",
  "status.ok": "ok",
  "status.error": "error",
  "status.client_closed": "client closed",

  // dashboard
  "dash.title": "Dashboard",
  "dash.slogan": "All your AI, one endpoint.",
  "dash.copyAddress": "Copy address",
  "dash.requestsToday": "Requests today",
  "dash.tokensToday": "Tokens today (in / out)",
  "dash.spend": "API spend today · month",
  "dash.failed": "Failed requests (24 h)",
  "dash.virtuals": "Virtual models",
  "dash.noRoutes": "No routes yet.",
  "dash.noRouteYet": "no route yet",
  "dash.accounts": "Accounts",
  "dash.noAccounts": "No accounts yet. Add one under Accounts & usage.",
  "dash.updated": "updated {when}",
  "dash.noData": "no data yet",
  "dash.estimated": "usage estimated",
  "dash.thisMonth": "{cost} this month",

  // accounts
  "acc.title": "Accounts & usage",
  "acc.refresh": "Refresh usage",
  "acc.scan": "Scan this PC",
  "acc.add": "Add account",
  "acc.empty":
    "No providers yet. Add an API account, or scan this PC for Codex, Claude Code, Gemini CLI and Kimi Code logins.",
  "acc.removeProvider": "Remove provider",
  "acc.confirmRemoveProvider": "Remove {slug} with all its accounts, keys and models?",
  "acc.noAccounts": "No accounts.",
  "acc.usageOnly": "usage only",
  "acc.usageOnlyHint": "Subscription logins are used for usage display only (ADR-009)",
  "acc.modelsRouted": "{on}/{all} models routed",
  "acc.sync": "Sync models",
  "acc.changeKey": "Change key",
  "acc.setKey": "Set key",
  "acc.plan": "Plan",
  "acc.confirmRemove": "Remove {name}? Its key is deleted from Windows Credential Manager.",
  "acc.balance": "Balance",
  "acc.topup": "top-up {n}",
  "acc.granted": "granted {n}",
  "acc.planName": "Plan: {name}",
  "acc.perMonth": "{price}/month",
  "acc.renews": "renews {date}",
  "acc.today": "Loadout today: {n} req · {cost}",
  "acc.month": "this month: {n} req · {cost}",
  "acc.value": "value captured {x}×",
  "acc.valueHint": "Equivalent API value of this month's requests ÷ subscription price",
  "acc.usageFrom": "usage {source} {confidence} · {when}",
  "acc.usageEstimated": "usage estimated from Loadout's own requests",
  "acc.addTitle": "Add an API account",
  "acc.kind": "Provider type",
  "acc.choose": "Choose…",
  "acc.slug": "Provider name",
  "acc.slugHint": "Used in model names, e.g. my-llm/main/model-id. Lowercase letters, digits, dashes.",
  "acc.baseUrl": "Base URL",
  "acc.baseUrlHint": "Including the version path, e.g. https://api.example.com/v1",
  "acc.alias": "Account name",
  "acc.aliasHint": "Distinguishes several accounts of one provider, e.g. main, work.",
  "acc.key": "API key",
  "acc.keyOptional": "API key (optional)",
  "acc.keyHint": "Stored in Windows Credential Manager; never in Loadout's database or logs.",
  "acc.adding": "Adding…",
  "acc.errKind": "Choose a provider type.",
  "acc.errSlug": "Enter a short name for this provider.",
  "acc.syncFailed": "Account added, but the model list could not be fetched: {error}",
  "acc.scanTitle": "AI tools on this PC",
  "acc.scanIntro":
    "Loadout only reads whether these tools are installed and signed in. Importing a subscription login shows its quota; requests are never sent with subscription credentials.",
  "acc.scanning": "Scanning…",
  "acc.tool": "Tool",
  "acc.installed": "Installed",
  "acc.signedIn": "Signed in",
  "acc.use": "Use",
  "acc.expired": "expired",
  "acc.requestsAndUsage": "requests + usage",
  "acc.imported": "imported",
  "acc.import": "Import",
  "acc.keyTitle": "API key for {name}",
  "acc.newKey": "New API key",
  "acc.newKeyHint": "Replaces the stored key and clears any authentication error.",
  "acc.planTitle": "Plan of {name}",
  "acc.planIntro":
    "For renewal reminders and the subscription value shown on this page. The renewal date is separate from quota resets.",
  "acc.planNameField": "Plan name",
  "acc.price": "Price (USD per month)",
  "acc.nextRenewal": "Next renewal",
  "acc.errPrice": "Enter a valid price.",

  // models
  "models.title": "Models",
  "models.addManual": "Add model manually",
  "models.empty": "No models. Sync an account's models under Accounts & usage, or add one manually.",
  "models.name": 'Name (use this as "model")',
  "models.protocols": "Protocols",
  "models.context": "Context",
  "models.price": "Price in / out ($/M)",
  "models.tier": "Tier",
  "models.manual": "manual",
  "models.copy": "Copy",
  "models.tierMeasured": "Measured",
  "models.tierPinned": "{tier} (pinned)",
  "models.priceTitle": "Prices of {name}",
  "models.priceIntro": "USD per million tokens, used for cost statistics.",
  "models.input": "Input",
  "models.output": "Output",
  "models.errPrice": "Prices must be non-negative numbers.",
  "models.addTitle": "Add a model manually",
  "models.account": "Account",
  "models.upstreamId": "Upstream model id",
  "models.serves": "Protocols the upstream serves it with",
  "models.contextOptional": "Context window (tokens, optional)",

  // routes
  "routes.title": "Routes",
  "routes.measureAll": "Measure unmeasured routes",
  "routes.measuring": "Measuring…",
  "routes.newPool": "New pool",
  "routes.intro":
    "Clients choose a route by the model name. {realtime} uses only ultra-fast routes, {fast} ultra-fast and fast ones, {default} everything (fastest first). The first available route in a list serves the request; the next ones take over when it fails or runs out of quota.",
  "routes.tabVirtual": "Virtual models & pools",
  "routes.tabHealth": "Route health & latency",
  "routes.pool": "pool · {ordering}",
  "routes.builtin": "built-in",
  "routes.ordering.manual": "manual",
  "routes.ordering.latency": "fastest first",
  "routes.measure": "Measure",
  "routes.noFastYet": "No measured route is fast enough yet. Measure routes or send some requests.",
  "routes.none": "No routes.",
  "routes.route": "Route",
  "routes.protocol": "Protocol",
  "routes.state": "State",
  "routes.ttft": "TTFT p50 / p90",
  "routes.samples": "Samples",
  "routes.okFail": "OK / fail",
  "routes.lastMeasured": "Last measured",
  "routes.lastError": "Last error",
  "routes.until": "until {time}",
  "routes.benchTitle": "Latency measurement",
  "routes.benchNothing": "Nothing to measure: every route already has latency data.",
  "routes.result": "Result",
  "routes.total": "Total",
  "routes.tierNow": "Tier now",
  "routes.failed": "failed",
  "routes.benchNote":
    "A tier needs at least 3 measurements; real requests count too. Each measurement is a tiny request (“Reply with exactly: OK”, at most 16 output tokens).",
  "routes.editPool": "Edit loadout/{name}",
  "routes.poolNameHint": "Clients use it as loadout/<name>.",
  "routes.order": "Order",
  "routes.orderManual": "As listed below",
  "routes.orderLatency": "Fastest first (tier, then speed)",
  "routes.models": "Models",
  "routes.addModel": "Add a model…",
  "routes.deletePool": "Delete pool",

  // clients
  "clients.title": "Client keys",
  "clients.new": "New client key",
  "clients.intro": "Every app that calls Loadout gets its own key. Point the app at {base} and use the key as its OpenAI or Anthropic API key.",
  "clients.empty": "No client keys yet.",
  "clients.key": "Key",
  "clients.created": "Created",
  "clients.status": "Status",
  "clients.today": "Today",
  "clients.policy": "Policy",
  "clients.revoked": "revoked {time}",
  "clients.active": "active",
  "clients.revoke": "Revoke",
  "clients.confirmRevoke": "Revoke the key of {name}? Apps using it stop working immediately.",
  "clients.usageToday": "{tokens} tok · {cost}",
  "clients.noRestrictions": "no restrictions",
  "clients.pDefault": "default {model}",
  "clients.pOnly": "only {models}",
  "clients.pTokens": "{n} tokens/day",
  "clients.pCost": "{cost}/day",
  "clients.pRpm": "{n}/min",
  "clients.nameHint": "Which app uses this key, e.g. opencode, cursor, my-script.",
  "clients.keyFor": "Key for {name}",
  "clients.copyNow": "Copy it now — it is shown only once.",
  "clients.hashOnly": "Loadout stores only a hash.",
  "clients.copyKey": "Copy key",
  "clients.openaiApps": "OpenAI-compatible apps",
  "clients.anthropicApps": "Anthropic-compatible apps",
  "clients.modelNames":
    "Model names: loadout/default, loadout/fast, loadout/realtime, a pool, or an exact provider/account/model from the Models page.",
  "clients.policyTitle": "Policy of {name}",
  "clients.defaultModel": "Default model",
  "clients.defaultModelHint":
    "Used when the app sends a model name Loadout does not know (for apps that cannot change it). E.g. loadout/default.",
  "clients.allowed": "Allowed models",
  "clients.allowedHint": "One per line; end with * for a prefix, e.g. loadout/* . Empty allows all.",
  "clients.tokensPerDay": "Tokens per day",
  "clients.costPerDay": "Cost per day (USD)",
  "clients.rpm": "Requests per minute",
  "clients.limitsNote": "Daily limits reset at 00:00 UTC. Requests over a limit get HTTP 429.",
  "clients.errLimits": "Limits must be positive numbers (leave empty for no limit).",

  // stats
  "stats.title": "Statistics",
  "stats.last24h": "last 24 hours",
  "stats.lastDays": "last {n} days",
  "stats.requests": "Requests",
  "stats.failed": "Failed",
  "stats.tokens": "Tokens in / out",
  "stats.cost": "Estimated cost",
  "stats.by": "By {group}",
  "stats.group.model": "Model",
  "stats.group.route": "Route",
  "stats.group.provider": "Provider",
  "stats.group.client": "Client",
  "stats.group.day": "Day (UTC)",
  "stats.empty": "No requests in this period.",
  "stats.costCol": "Cost",
  "stats.avgTtft": "Avg TTFT",
  "stats.avgTps": "Avg tok/s",
  "stats.note":
    "Costs are estimated from model prices (Models page) and the token counts reported by upstreams. Request history is kept for 400 days.",

  // logs
  "logs.title": "Request log",
  "logs.privacy":
    "Metadata only: prompts, responses and tool arguments are never stored. Entries older than 400 days are deleted automatically.",
  "logs.search": "Model or route contains…",
  "logs.allClients": "all clients",
  "logs.anyTime": "any time",
  "logs.lastHour": "last hour",
  "logs.last24h": "last 24 h",
  "logs.last7d": "last 7 days",
  "logs.failedOnly": "failed only",
  "logs.upTo": "up to {n}",
  "logs.empty": "No matching requests.",
  "logs.time": "Time",
  "logs.client": "Client",
  "logs.modelRoute": "Model → route",
  "logs.protocol": "Protocol",
  "logs.status": "Status",
  "logs.tokens": "Tokens in / out",
  "logs.total": "Total",
  "logs.cost": "Cost",
  "logs.converted": "(converted)",
  "logs.tries": "{n} tries",

  // settings
  "settings.title": "Settings",
  "settings.gateway": "Gateway",
  "settings.address": "Address",
  "settings.version": "Version",
  "settings.versionValue": "{version} (schema {schema}, pid {pid})",
  "settings.since": "Running since",
  "settings.dataDir": "Data folder",
  "settings.autostart": "Start at sign-in",
  "settings.autostartLabel": "Start the gateway when I sign in to Windows",
  "settings.stop": "Stop gateway",
  "settings.confirmStop": "Stop the gateway? Apps using Loadout lose access until it is started again.",
  "settings.appearance": "Appearance",
  "settings.language": "Language",
  "settings.options": "Options",
  "settings.setting": "Setting",
  "settings.value": "Value",
  "settings.restart": "applies after restarting the gateway",
  "settings.on": "on",
  "settings.off": "off",
  "settings.default": "Default",
  "settings.backup": "Backup",
  "settings.backupIntro":
    "The configuration export contains providers, accounts, models, pools, client keys (as hashes, so existing keys keep working) and settings — never API keys. After importing on another PC, enter each API key again.",
  "settings.pathPlaceholder": "File path (optional for export; default: the data folder's backups)",
  "settings.export": "Export configuration",
  "settings.import": "Import",
  "settings.copyDb": "Copy database",
  "settings.exported": "Exported {providers} providers, {accounts} accounts, {models} models, {pools} pools, {clients} client keys to {path}",
  "settings.imported": "Imported {providers} providers, {accounts} accounts, {models} models, {pools} pools, {clients} client keys, {settings} settings.",
  "settings.needKeys": " Enter the API key again for: {accounts}.",
  "settings.skipped": " Skipped: {items}.",
  "settings.dbCopied": "Database copied to {path}",
} as const;

export type MessageKey = keyof typeof en;

const zh: Record<MessageKey, string> = {
  "nav.dashboard": "总览",
  "nav.accounts": "账号与用量",
  "nav.models": "模型",
  "nav.routes": "路由",
  "nav.clients": "客户端密钥",
  "nav.stats": "统计",
  "nav.logs": "请求日志",
  "nav.settings": "设置",
  "shell.running": "网关运行中",
  "shell.connecting": "连接中…",
  "shell.stopped": "网关已停止",
  "shell.notRunning": "Loadout 网关没有运行。",
  "shell.notRunningHint": "启动后才能管理账号并处理请求。关闭本窗口不会停止网关。",
  "shell.start": "启动网关",
  "shell.starting": "正在启动…",
  "shell.recovered": "数据库已损坏，已用一个空数据库替代。",
  "shell.recoveredHint": "损坏的文件保留在 {file}。请在 设置 → 备份 → 导入 中，选择 {dir} 里最新的文件恢复配置。",
  "shell.language": "语言",

  "common.cancel": "取消",
  "common.close": "关闭",
  "common.save": "保存",
  "common.add": "添加",
  "common.remove": "删除",
  "common.edit": "编辑",
  "common.enable": "启用",
  "common.disable": "停用",
  "common.enabled": "已启用",
  "common.disabled": "已停用",
  "common.done": "完成",
  "common.create": "创建",
  "common.yes": "是",
  "common.no": "否",
  "common.copy": "复制",
  "common.name": "名称",
  "common.left": "剩余 {n}%",
  "common.resets": "{when}重置",
  "common.never": "从未",
  "common.now": "现在",
  "common.inMin": "{n} 分钟后",
  "common.inHours": "{h} 小时 {m} 分钟后",
  "common.inDays": "{n} 天后",
  "common.secAgo": "{n} 秒前",
  "common.minAgo": "{n} 分钟前",
  "common.hoursAgo": "{n} 小时前",
  "common.daysAgo": "{n} 天前",
  "common.showDisabled": "显示已停用",
  "common.filter": "筛选",

  "tier.ULTRA_FAST": "超快",
  "tier.FAST": "快",
  "tier.NORMAL": "普通",
  "tier.UNMEASURED": "未测量",
  "tier.pinned": "已固定",
  "state.HEALTHY": "正常",
  "state.UNKNOWN": "未知",
  "state.RECOVERING": "恢复中",
  "state.DEGRADED": "不稳定",
  "state.COOLDOWN": "冷却中",
  "state.RATE_LIMITED": "被限流",
  "state.QUOTA_EXHAUSTED": "额度耗尽",
  "state.AUTH_FAILED": "认证失败",
  "accountType.api_key": "API key",
  "accountType.subscription": "订阅",
  "accountType.local": "本地",
  "status.ok": "成功",
  "status.error": "失败",
  "status.client_closed": "客户端中断",

  "dash.title": "总览",
  "dash.slogan": "All your AI, one endpoint.",
  "dash.copyAddress": "复制地址",
  "dash.requestsToday": "今日请求",
  "dash.tokensToday": "今日 token（输入 / 输出）",
  "dash.spend": "API 花费 今日 · 本月",
  "dash.failed": "失败请求（24 小时）",
  "dash.virtuals": "虚拟模型",
  "dash.noRoutes": "还没有路由。",
  "dash.noRouteYet": "暂无路由",
  "dash.accounts": "账号",
  "dash.noAccounts": "还没有账号，请在“账号与用量”中添加。",
  "dash.updated": "{when}更新",
  "dash.noData": "暂无数据",
  "dash.estimated": "用量为估算",
  "dash.thisMonth": "本月 {cost}",

  "acc.title": "账号与用量",
  "acc.refresh": "刷新用量",
  "acc.scan": "扫描本机",
  "acc.add": "添加账号",
  "acc.empty": "还没有提供方。添加一个 API 账号，或扫描本机上已登录的 Codex、Claude Code、Gemini CLI 和 Kimi Code。",
  "acc.removeProvider": "删除提供方",
  "acc.confirmRemoveProvider": "删除 {slug} 及其所有账号、密钥和模型？",
  "acc.noAccounts": "没有账号。",
  "acc.usageOnly": "仅用量",
  "acc.usageOnlyHint": "订阅登录只用于显示用量（ADR-009）",
  "acc.modelsRouted": "{on}/{all} 个模型参与路由",
  "acc.sync": "同步模型",
  "acc.changeKey": "更换密钥",
  "acc.setKey": "设置密钥",
  "acc.plan": "订阅计划",
  "acc.confirmRemove": "删除 {name}？其密钥会从 Windows 凭据管理器中删除。",
  "acc.balance": "余额",
  "acc.topup": "充值 {n}",
  "acc.granted": "赠送 {n}",
  "acc.planName": "计划：{name}",
  "acc.perMonth": "{price}/月",
  "acc.renews": "{date} 续费",
  "acc.today": "Loadout 今日：{n} 次请求 · {cost}",
  "acc.month": "本月：{n} 次请求 · {cost}",
  "acc.value": "订阅价值 {x}×",
  "acc.valueHint": "本月请求的等价 API 价值 ÷ 订阅价格",
  "acc.usageFrom": "用量来源 {source} {confidence} · {when}",
  "acc.usageEstimated": "用量由 Loadout 自己的请求记录估算",
  "acc.addTitle": "添加 API 账号",
  "acc.kind": "提供方类型",
  "acc.choose": "请选择…",
  "acc.slug": "提供方名称",
  "acc.slugHint": "用于模型名，例如 my-llm/main/model-id。只能用小写字母、数字和连字符。",
  "acc.baseUrl": "Base URL",
  "acc.baseUrlHint": "包含版本路径，例如 https://api.example.com/v1",
  "acc.alias": "账号名称",
  "acc.aliasHint": "用来区分同一提供方的多个账号，例如 main、work。",
  "acc.key": "API key",
  "acc.keyOptional": "API key（可选）",
  "acc.keyHint": "保存在 Windows 凭据管理器中，从不写入 Loadout 的数据库或日志。",
  "acc.adding": "正在添加…",
  "acc.errKind": "请选择提供方类型。",
  "acc.errSlug": "请为这个提供方填写一个简短的名称。",
  "acc.syncFailed": "账号已添加，但获取模型列表失败：{error}",
  "acc.scanTitle": "本机的 AI 工具",
  "acc.scanIntro": "Loadout 只检查这些工具是否已安装、已登录。导入订阅登录后可查看其额度；请求永远不会使用订阅凭证发送。",
  "acc.scanning": "正在扫描…",
  "acc.tool": "工具",
  "acc.installed": "已安装",
  "acc.signedIn": "已登录",
  "acc.use": "用途",
  "acc.expired": "已过期",
  "acc.requestsAndUsage": "请求 + 用量",
  "acc.imported": "已导入",
  "acc.import": "导入",
  "acc.keyTitle": "{name} 的 API key",
  "acc.newKey": "新的 API key",
  "acc.newKeyHint": "替换已保存的密钥，并清除认证错误状态。",
  "acc.planTitle": "{name} 的订阅计划",
  "acc.planIntro": "用于续费提醒和本页显示的订阅价值。续费日期与额度重置时间是分开的。",
  "acc.planNameField": "计划名称",
  "acc.price": "价格（美元/月）",
  "acc.nextRenewal": "下次续费",
  "acc.errPrice": "请输入有效的价格。",

  "models.title": "模型",
  "models.addManual": "手动添加模型",
  "models.empty": "没有模型。请在“账号与用量”中同步账号的模型，或手动添加。",
  "models.name": "名称（作为请求中的 model）",
  "models.protocols": "协议",
  "models.context": "上下文",
  "models.price": "价格 输入 / 输出（$/百万）",
  "models.tier": "档位",
  "models.manual": "手动",
  "models.copy": "复制",
  "models.tierMeasured": "按测量",
  "models.tierPinned": "{tier}（固定）",
  "models.priceTitle": "{name} 的价格",
  "models.priceIntro": "单位为美元 / 百万 token，用于费用统计。",
  "models.input": "输入",
  "models.output": "输出",
  "models.errPrice": "价格必须是非负数。",
  "models.addTitle": "手动添加模型",
  "models.account": "账号",
  "models.upstreamId": "上游模型 id",
  "models.serves": "上游为该模型提供的协议",
  "models.contextOptional": "上下文窗口（token，可选）",

  "routes.title": "路由",
  "routes.measureAll": "测量未测速的路由",
  "routes.measuring": "正在测量…",
  "routes.newPool": "新建 Pool",
  "routes.intro":
    "客户端通过 model 名选择路由。{realtime} 只使用超快路由，{fast} 使用超快和快的路由，{default} 使用全部（最快的在前）。列表中第一条可用的路由处理请求；它失败或额度用完时，由后面的路由接替。",
  "routes.tabVirtual": "虚拟模型与 Pool",
  "routes.tabHealth": "路由健康与延迟",
  "routes.pool": "Pool · {ordering}",
  "routes.builtin": "内置",
  "routes.ordering.manual": "手动顺序",
  "routes.ordering.latency": "最快优先",
  "routes.measure": "测速",
  "routes.noFastYet": "还没有足够快的已测量路由。请先测速或发送一些请求。",
  "routes.none": "没有路由。",
  "routes.route": "路由",
  "routes.protocol": "协议",
  "routes.state": "状态",
  "routes.ttft": "首 token 时间 p50 / p90",
  "routes.samples": "样本",
  "routes.okFail": "成功 / 失败",
  "routes.lastMeasured": "最近测量",
  "routes.lastError": "最近错误",
  "routes.until": "直到 {time}",
  "routes.benchTitle": "延迟测量",
  "routes.benchNothing": "无需测量：所有路由都已有延迟数据。",
  "routes.result": "结果",
  "routes.total": "总耗时",
  "routes.tierNow": "当前档位",
  "routes.failed": "失败",
  "routes.benchNote": "至少 3 次测量才会分档，真实请求也会计入。每次测量只发送一个很小的请求（“Reply with exactly: OK”，最多 16 个输出 token）。",
  "routes.editPool": "编辑 loadout/{name}",
  "routes.poolNameHint": "客户端以 loadout/<名称> 使用。",
  "routes.order": "顺序",
  "routes.orderManual": "按下方列表顺序",
  "routes.orderLatency": "最快优先（先档位，再速度）",
  "routes.models": "模型",
  "routes.addModel": "添加模型…",
  "routes.deletePool": "删除 Pool",

  "clients.title": "客户端密钥",
  "clients.new": "新建客户端密钥",
  "clients.intro": "每个调用 Loadout 的应用使用自己的密钥。把应用的接口地址设为 {base}，并把密钥作为它的 OpenAI 或 Anthropic API key。",
  "clients.empty": "还没有客户端密钥。",
  "clients.key": "密钥",
  "clients.created": "创建时间",
  "clients.status": "状态",
  "clients.today": "今日",
  "clients.policy": "策略",
  "clients.revoked": "已于 {time} 吊销",
  "clients.active": "有效",
  "clients.revoke": "吊销",
  "clients.confirmRevoke": "吊销 {name} 的密钥？使用它的应用会立即无法访问。",
  "clients.usageToday": "{tokens} token · {cost}",
  "clients.noRestrictions": "无限制",
  "clients.pDefault": "默认 {model}",
  "clients.pOnly": "仅 {models}",
  "clients.pTokens": "{n} token/天",
  "clients.pCost": "{cost}/天",
  "clients.pRpm": "{n} 次/分钟",
  "clients.nameHint": "哪个应用使用这个密钥，例如 opencode、cursor、my-script。",
  "clients.keyFor": "{name} 的密钥",
  "clients.copyNow": "请立即复制——密钥只显示这一次。",
  "clients.hashOnly": "Loadout 只保存它的哈希值。",
  "clients.copyKey": "复制密钥",
  "clients.openaiApps": "OpenAI 兼容应用",
  "clients.anthropicApps": "Anthropic 兼容应用",
  "clients.modelNames": "模型名：loadout/default、loadout/fast、loadout/realtime、某个 Pool，或“模型”页面中的精确名 provider/account/model。",
  "clients.policyTitle": "{name} 的策略",
  "clients.defaultModel": "默认模型",
  "clients.defaultModelHint": "当应用发送 Loadout 不认识的模型名时使用（适用于无法修改模型名的应用），例如 loadout/default。",
  "clients.allowed": "允许的模型",
  "clients.allowedHint": "每行一个；以 * 结尾表示前缀，例如 loadout/* 。留空表示不限制。",
  "clients.tokensPerDay": "每日 token 上限",
  "clients.costPerDay": "每日费用上限（美元）",
  "clients.rpm": "每分钟请求数",
  "clients.limitsNote": "每日上限在 UTC 00:00（北京时间 08:00）重置。超出上限的请求返回 HTTP 429。",
  "clients.errLimits": "上限必须是正数（留空表示不限制）。",

  "stats.title": "统计",
  "stats.last24h": "最近 24 小时",
  "stats.lastDays": "最近 {n} 天",
  "stats.requests": "请求数",
  "stats.failed": "失败",
  "stats.tokens": "Token 输入 / 输出",
  "stats.cost": "估算费用",
  "stats.by": "按{group}",
  "stats.group.model": "模型",
  "stats.group.route": "路由",
  "stats.group.provider": "提供方",
  "stats.group.client": "客户端",
  "stats.group.day": "日期（UTC）",
  "stats.empty": "这段时间没有请求。",
  "stats.costCol": "费用",
  "stats.avgTtft": "平均首 token 时间",
  "stats.avgTps": "平均 tok/s",
  "stats.note": "费用根据模型价格（“模型”页面）和上游报告的 token 数估算。请求历史保留 400 天。",

  "logs.title": "请求日志",
  "logs.privacy": "只记录元数据：提示词、回复和工具参数从不保存。超过 400 天的记录会被自动删除。",
  "logs.search": "模型或路由包含…",
  "logs.allClients": "全部客户端",
  "logs.anyTime": "全部时间",
  "logs.lastHour": "最近 1 小时",
  "logs.last24h": "最近 24 小时",
  "logs.last7d": "最近 7 天",
  "logs.failedOnly": "只看失败",
  "logs.upTo": "最多 {n} 条",
  "logs.empty": "没有符合条件的请求。",
  "logs.time": "时间",
  "logs.client": "客户端",
  "logs.modelRoute": "模型 → 路由",
  "logs.protocol": "协议",
  "logs.status": "状态",
  "logs.tokens": "Token 输入 / 输出",
  "logs.total": "总耗时",
  "logs.cost": "费用",
  "logs.converted": "（已转换）",
  "logs.tries": "尝试 {n} 次",

  "settings.title": "设置",
  "settings.gateway": "网关",
  "settings.address": "地址",
  "settings.version": "版本",
  "settings.versionValue": "{version}（数据库版本 {schema}，进程 {pid}）",
  "settings.since": "启动时间",
  "settings.dataDir": "数据目录",
  "settings.autostart": "开机启动",
  "settings.autostartLabel": "登录 Windows 时自动启动网关",
  "settings.stop": "停止网关",
  "settings.confirmStop": "停止网关？在重新启动之前，使用 Loadout 的应用都将无法访问。",
  "settings.appearance": "外观",
  "settings.language": "界面语言",
  "settings.options": "选项",
  "settings.setting": "设置项",
  "settings.value": "值",
  "settings.restart": "重启网关后生效",
  "settings.on": "开",
  "settings.off": "关",
  "settings.default": "恢复默认",
  "settings.backup": "备份",
  "settings.backupIntro":
    "配置导出包含提供方、账号、模型、Pool、客户端密钥（仅哈希，原有密钥继续可用）和设置——从不包含 API key。在另一台电脑导入后，需要为各账号重新输入 API key。",
  "settings.pathPlaceholder": "文件路径（导出时可留空，默认存入数据目录的 backups）",
  "settings.export": "导出配置",
  "settings.import": "导入",
  "settings.copyDb": "复制数据库",
  "settings.exported": "已导出 {providers} 个提供方、{accounts} 个账号、{models} 个模型、{pools} 个 Pool、{clients} 个客户端密钥到 {path}",
  "settings.imported": "已导入 {providers} 个提供方、{accounts} 个账号、{models} 个模型、{pools} 个 Pool、{clients} 个客户端密钥、{settings} 项设置。",
  "settings.needKeys": " 请为以下账号重新输入 API key：{accounts}。",
  "settings.skipped": " 已跳过：{items}。",
  "settings.dbCopied": "数据库已复制到 {path}",
};

/** Chinese descriptions of the daemon's settings (the daemon sends English). */
const settingDescriptionsZh: Record<string, string> = {
  "gateway.port": "本地网关在 127.0.0.1 上监听的 TCP 端口",
  "gateway.allowed_origins": "允许调用网关的浏览器来源，用逗号分隔（例如 http://localhost:3000）；留空则拒绝所有浏览器调用",
  "upstream.first_token_timeout_secs": "等待上游返回第一个输出 token 的秒数，超时后切换到下一条路由",
  "routing.max_attempts": "每个请求最多尝试的路由数（ADR-003）",
  "routing.quota_reserve_percent": "任一额度窗口剩余低于该百分比时，不再把请求发往该账号（0 表示关闭）",
  "latency.ultra_fast_ms": "P90 首 token 时间低于该值（毫秒）的路由为“超快”（ADR-003）",
  "latency.fast_ms": "P90 首 token 时间低于该值（毫秒）的路由为“快”",
  "benchmark.auto_probe": "为近期没有流量的路由在后台发送极小的测速请求",
  "benchmark.probe_interval_hours": "同一路由两次自动测速之间的间隔（小时）",
  "benchmark.max_probe_output_price": "输出价格高于该值（美元 / 百万 token）的模型不自动测速（价格未知的会测）",
};

const dictionaries: Record<Lang, Record<MessageKey, string>> = { "zh-CN": zh, en };

function readStoredLang(): Lang {
  try {
    const v = window.localStorage.getItem(STORAGE_KEY);
    if (v === "zh-CN" || v === "en") return v;
  } catch {
    // Storage unavailable: use the default.
  }
  return DEFAULT_LANG;
}

let current: Lang = readStoredLang();

export function currentLang(): Lang {
  return current;
}

/** Locale for dates and numbers. */
export function locale(): string {
  return current === "zh-CN" ? "zh-CN" : "en-US";
}

type Params = Record<string, string | number>;

/** Translates `key`, replacing `{name}` placeholders. */
export function t(key: MessageKey, params?: Params): string {
  let text = dictionaries[current][key] ?? en[key];
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      text = text.split(`{${k}}`).join(String(v));
    }
  }
  return text;
}

/** Like `t`, but placeholders may be React elements (e.g. monospace names). */
export function tr(key: MessageKey, params: Record<string, ReactNode>): ReactNode {
  const text = dictionaries[current][key] ?? en[key];
  const parts = text.split(/(\{[a-z_]+\})/i);
  return parts.map((part, i) => {
    const m = /^\{([a-z_]+)\}$/i.exec(part);
    return <Fragment key={i}>{m && m[1] && m[1] in params ? params[m[1]] : part}</Fragment>;
  });
}

/** Translation of a known value (state, tier, …), or the value itself. */
export function tv(prefix: "state" | "tier" | "accountType" | "status", value: string): string {
  const key = `${prefix}.${value}` as MessageKey;
  return key in en ? t(key) : value;
}

export function settingDescription(key: string, fallback: string): string {
  return current === "zh-CN" ? (settingDescriptionsZh[key] ?? fallback) : fallback;
}

function syncTray(lang: Lang) {
  // The tray menu lives in the Rust side; outside Tauri (e.g. a plain browser) this fails.
  void invoke("set_tray_language", { lang }).catch(() => undefined);
}

const LangContext = createContext<{ lang: Lang; setLang: (l: Lang) => void }>({
  lang: current,
  setLang: () => undefined,
});

export function LangProvider(props: { children: ReactNode }) {
  const [lang, setLangState] = useState<Lang>(current);
  const setLang = useCallback((l: Lang) => {
    current = l;
    try {
      window.localStorage.setItem(STORAGE_KEY, l);
    } catch {
      // Not remembered, but still applied for this session.
    }
    document.documentElement.lang = l;
    syncTray(l);
    setLangState(l);
  }, []);
  const value = useMemo(() => ({ lang, setLang }), [lang, setLang]);
  return <LangContext.Provider value={value}>{props.children}</LangContext.Provider>;
}

/** Re-renders the caller when the language changes. */
export function useLang() {
  return useContext(LangContext);
}

/** Applies the stored language to the document and the tray at startup. */
export function initLang() {
  document.documentElement.lang = current;
  syncTray(current);
}
