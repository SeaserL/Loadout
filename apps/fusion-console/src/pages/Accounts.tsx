// SPDX-License-Identifier: Apache-2.0

import { type ReactNode, useEffect, useMemo, useState } from "react";
import {
  Download,
  GripVertical,
  ExternalLink,
  KeyRound,
  LogIn,
  Plus,
  Receipt,
  RefreshCw,
  ScanSearch,
  Search,
  ShieldAlert,
  Stethoscope,
  Trash,
  Upload,
  Zap,
} from "lucide-react";
import {
  api,
  type AccountCheckResult,
  type AccountInfo,
  type AccountUsage,
  type ModelInfo,
  type OAuthStart,
  type ProviderInfo,
  type ProviderKind,
} from "../api";
import {
  Badge,
  Button,
  Card,
  CopyField,
  Empty,
  ErrorText,
  Field,
  IconButton,
  Modal,
  Notice,
  Page,
  QuotaBar,
  Switch,
  useConfirm,
  useToast,
} from "../components";
import { ago, date, num, resetIn, usd } from "../format";
import { useAction, usePoll } from "../hooks";
import { t, tv, type MessageKey } from "../i18n";
import { KeysExportDialog, KeysImportDialog, SubpoolBar, SubpoolManager, subpoolApi } from "./Subpools";
import { type GripProps, usePointerReorder } from "../reorder";
import {
  EndpointButton,
  EndpointDialog,
  type EndpointInfo,
  HealthBadge,
  accountHealth,
  endpointApi,
} from "./Endpoints";
import type { TargetStatus } from "../api";

/** Toast text and tone for an `account.check` result (ExecPlan 014). */
function checkMessage(name: string, r: AccountCheckResult): [string, "ok" | "bad"] {
  const known = ["invalid_key", "account_disabled", "rate_limited", "no_credit", "unreachable", "unsupported", "missing_key"];
  let text =
    r.status === "ok"
      ? r.models_listed != null
        ? t("acc.check.ok", { name, n: r.models_listed })
        : t("acc.check.okNoList", { name })
      : t((known.includes(r.status) ? `acc.check.${r.status}` : "acc.check.error") as MessageKey, { name });
  if (r.status !== "ok" && r.message) text += ` ${r.message}`;
  if (r.cleared) text += ` ${t("acc.check.cleared")}`;
  return [text, r.status === "ok" ? "ok" : "bad"];
}

type Dialog =
  | { kind: "add" }
  | { kind: "scan" }
  | { kind: "key"; account: AccountInfo }
  | { kind: "plan"; account: AccountInfo; usage?: AccountUsage }
  | { kind: "login"; alias?: string }
  | { kind: "transport"; account: AccountInfo; web?: boolean }
  | { kind: "subpools" }
  | { kind: "keysExport" }
  | { kind: "keysImport" }
  | { kind: "endpoint"; provider: ProviderInfo };

/** Built-in provider that signs in through Loadout (ADR-012). */
const SUBSCRIPTION_SLUG = "codex-sub";

/** Accounts checked at once by "Check all": enough to be quick, few enough not to burst a provider. */
const CHECK_ALL_CONCURRENCY = 3;

/** A check that proves the account works: its key, or (without a model list) a model answering. */
const checkPassed = (r: AccountCheckResult) =>
  r.status === "ok" || (r.status === "unsupported" && r.models.some((m) => m.ok));

/** Progress of a provider-wide check. */
interface CheckAllProgress {
  slug: string;
  done: number;
  total: number;
}

export function Accounts() {
  const providers = usePoll(api.providers, 10000);
  const accounts = usePoll(api.accounts, 10000);
  const usage = usePoll(() => api.usage(false), 10000);
  const kinds = usePoll(api.providerKinds, 60000);
  const models = usePoll(api.models, 30000);
  const subpools = usePoll(subpoolApi.state, 10000);
  // Account health and dedicated endpoints (ExecPlan 021).
  const statuses = usePoll(api.routeStatus, 5000);
  const endpoints = usePoll(endpointApi.list, 10000);
  const action = useAction();
  const toast = useToast();
  const [dialog, setDialog] = useState<Dialog>();
  const [only, setOnly] = useState<string>();
  const [query, setQuery] = useState("");
  const [checking, setChecking] = useState<CheckAllProgress>();

  const reloadAll = async () => {
    await Promise.all([
      providers.reload(),
      accounts.reload(),
      usage.reload(),
      models.reload(),
      subpools.reload(),
      statuses.reload(),
      endpoints.reload(),
    ]);
  };

  // Accounts outside the active sub-pool are not routable (ExecPlan 019).
  const activePool = subpools.data?.subpools.find((p) => p.active);
  const outside = (a: AccountInfo) => !!activePool && !activePool.accounts.includes(`${a.provider}/${a.alias}`);
  const switchPool = (name: string | null) =>
    void action.run(async () => {
      await subpoolApi.activate(name);
      await reloadAll();
      toast(t("sp.switched", { name: name ?? t("sp.main") }), "ok");
    });

  const usageOf = (a: AccountInfo) =>
    usage.data?.find((u) => u.provider === a.provider && u.alias === a.alias);

  const byProvider = useMemo(() => {
    const map = new Map<string, AccountInfo[]>();
    for (const a of accounts.data ?? []) {
      map.set(a.provider, [...(map.get(a.provider) ?? []), a]);
    }
    return map;
  }, [accounts.data]);

  const q = query.trim().toLowerCase();
  const matches = (a: AccountInfo) =>
    !q ||
    `${a.provider}/${a.alias}`.includes(q) ||
    (a.display_name ?? "").toLowerCase().includes(q) ||
    (usageOf(a)?.account_label ?? "").toLowerCase().includes(q);

  const onAction = (f: () => Promise<unknown>) =>
    void action.run(async () => {
      await f();
      await reloadAll();
    });

  // The board in the user's order (ExecPlan 023); the ChatGPT kinds share one card.
  const isChatGpt = (p: ProviderInfo) => (CHATGPT_KINDS as readonly string[]).includes(p.kind);
  const chatGpt = (providers.data ?? []).filter(isChatGpt);
  const items: string[] = [];
  for (const p of providers.data ?? []) {
    const id = isChatGpt(p) ? CHATGPT_ITEM : p.slug;
    if (!items.includes(id)) items.push(id);
  }
  if (!items.includes(CHATGPT_ITEM)) items.push(CHATGPT_ITEM);
  const providerOf = (id: string) => (providers.data ?? []).find((p) => p.slug === id);
  const slugsOf = (id: string) => (id === CHATGPT_ITEM ? chatGpt.map((p) => p.slug) : [id]);
  const board = usePointerReorder({
    scope: "main",
    scopeAttr: "board",
    itemAttr: "board-item",
    ids: items,
    onReorder: (ids) =>
      onAction(async () => {
        await api.reorderProviders(ids.flatMap(slugsOf));
        toast(t("acc.providersReordered"), "ok");
      }),
  });
  const visibleItem = (id: string) =>
    (!only || only === id) &&
    (!q ||
      id.includes(q) ||
      (id === CHATGPT_ITEM && ("chatgpt codex".includes(q) || chatGpt.some((p) => p.slug.includes(q)))) ||
      slugsOf(id).some((slug) => (byProvider.get(slug) ?? []).some(matches)));
  const shown = items.filter(visibleItem);
  const ctx: BoardCtx = {
    kindOf: (p) => kinds.data?.find((k) => k.kind === p.kind),
    accountsOf: (slug) => byProvider.get(slug) ?? [],
    usageOf,
    outside,
    statuses: statuses.data,
    endpointOf: (slug) => endpoints.data?.find((e) => e.provider === slug),
    busy: action.busy,
    onAction,
    onBench: (a) => bench(a),
    checking,
    onCheckAll: (p, list) => checkAll(p, list),
    onDialog: setDialog,
    onReorderAccounts: (provider, aliases) =>
      onAction(async () => {
        await api.reorderAccounts(provider, aliases);
        toast(t("acc.reordered"), "ok");
      }),
  };

  const bench = (a: AccountInfo) =>
    void action.run(async () => {
      const names = (models.data ?? [])
        .filter((m: ModelInfo) => m.provider === a.provider && m.account === a.alias && m.enabled)
        .map((m) => m.name);
      if (names.length === 0) {
        toast(t("acc.benchNone"), "info");
        return;
      }
      const results = await api.bench(names);
      const ok = results.filter((r) => r.ok).length;
      toast(t("acc.benchDone", { ok, total: results.length }), ok === results.length ? "ok" : "bad");
    });

  // One click for every card of a provider: the key check plus a probe of each enabled model,
  // which also refreshes health and latency (ExecPlan 033).
  const checkAll = (p: ProviderInfo, list: AccountInfo[]) =>
    void action.run(async () => {
      const passed: string[] = [];
      const failed: string[] = [];
      let modelsOk = 0;
      let modelsTotal = 0;
      let next = 0;
      const worker = async () => {
        for (;;) {
          const a = list[next++];
          if (!a) return;
          try {
            const r = await api.checkAccount(a.provider, a.alias, a.enabled);
            (checkPassed(r) ? passed : failed).push(a.alias);
            modelsTotal += r.models.length;
            modelsOk += r.models.filter((m) => m.ok).length;
          } catch {
            failed.push(a.alias);
          }
          setChecking((s) => s && { ...s, done: s.done + 1 });
        }
      };
      setChecking({ slug: p.slug, done: 0, total: list.length });
      try {
        await Promise.all(Array.from({ length: Math.min(CHECK_ALL_CONCURRENCY, list.length) }, worker));
      } finally {
        setChecking(undefined);
      }
      await reloadAll();
      let text = t("acc.checkAllDone", { name: p.display_name, ok: passed.length, total: list.length });
      if (modelsTotal > 0) text += ` ${t("acc.checkAllModels", { ok: modelsOk, total: modelsTotal })}`;
      if (failed.length > 0) text += ` ${t("acc.checkAllFailed", { list: failed.join(", ") })}`;
      toast(text, failed.length === 0 && modelsOk === modelsTotal ? "ok" : "bad");
    });

  return (
    <Page
      title={t("acc.title")}
      help="accounts"
      actions={
        <>
          <Button
            icon={<RefreshCw size={16} />}
            disabled={action.busy}
            onClick={() => void action.run(async () => api.usage(true).then(() => usage.reload()))}
          >
            {t("acc.refresh")}
          </Button>
          <Button icon={<Download size={16} />} onClick={() => setDialog({ kind: "keysImport" })}>
            {t("keys.import")}
          </Button>
          <Button icon={<Upload size={16} />} onClick={() => setDialog({ kind: "keysExport" })}>
            {t("keys.export")}
          </Button>
          <Button icon={<ScanSearch size={16} />} onClick={() => setDialog({ kind: "scan" })}>
            {t("acc.scan")}
          </Button>
          <Button kind="primary" icon={<Plus size={16} />} onClick={() => setDialog({ kind: "add" })}>
            {t("acc.add")}
          </Button>
        </>
      }
    >
      <ErrorText error={action.error ?? providers.error?.message ?? accounts.error?.message} />
      {(providers.data ?? []).length === 0 && !providers.loading && (
        <Card>
          <Empty
            action={
              <div className="row" style={{ justifyContent: "center" }}>
                <Button icon={<ScanSearch size={16} />} onClick={() => setDialog({ kind: "scan" })}>
                  {t("acc.scan")}
                </Button>
                <Button kind="primary" icon={<Plus size={16} />} onClick={() => setDialog({ kind: "add" })}>
                  {t("acc.add")}
                </Button>
              </div>
            }
          >
            {t("acc.empty")}
          </Empty>
        </Card>
      )}

      {(providers.data ?? []).length > 0 && (
        <>
          <SubpoolBar
            state={subpools.data}
            busy={action.busy}
            onSwitch={switchPool}
            onManage={() => setDialog({ kind: "subpools" })}
          />
          {/* Filter chips: one per card, in board order. */}
          <div className="board-toolbar">
            <label className="search">
              <Search size={16} aria-hidden />
              <input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t("acc.search")}
                aria-label={t("acc.search")}
              />
            </label>
            <div className="provider-chips" role="group" aria-label={t("acc.filterProviders")}>
              <button
                type="button"
                className={`chip ${only ? "" : "active"}`}
                aria-pressed={!only}
                onClick={() => setOnly(undefined)}
              >
                {t("acc.allProviders")}
              </button>
              {items.map((id) => {
                const name = id === CHATGPT_ITEM ? t("cg.title") : (providerOf(id)?.display_name ?? id);
                const n = slugsOf(id).reduce((sum, slug) => sum + (byProvider.get(slug)?.length ?? 0), 0);
                return (
                  <button
                    key={id}
                    type="button"
                    className={`chip ${only === id ? "active" : ""}`}
                    aria-pressed={only === id}
                    title={t("acc.showOnly", { name })}
                    onClick={() => setOnly(only === id ? undefined : id)}
                  >
                    <span className="ellipsis">{name}</span>
                    <span className="chip-count">{n}</span>
                  </button>
                );
              })}
            </div>
          </div>
        </>
      )}

      {shown.length > 0 && (providers.data ?? []).length > 0 && (
        <div className="provider-board" data-board="main">
          {shown.map((id) =>
            id === CHATGPT_ITEM ? (
              <ChatGptSection
                key={id}
                providers={chatGpt}
                visible={matches}
                ctx={ctx}
                boardGrip={board.grip(id)}
                boardClass={board.stateClass(id)}
              />
            ) : (
              providerOf(id) && (
                <ProviderSection
                  key={id}
                  provider={providerOf(id) as ProviderInfo}
                  accounts={(byProvider.get(id) ?? []).filter(matches)}
                  ctx={ctx}
                  boardGrip={board.grip(id)}
                  boardClass={board.stateClass(id)}
                />
              )
            ),
          )}
        </div>
      )}
      {shown.length === 0 && (providers.data ?? []).length > 0 && <Empty>{t("acc.noMatch")}</Empty>}

      {dialog?.kind === "add" && (
        <AddAccountDialog
          providers={providers.data ?? []}
          onClose={() => setDialog(undefined)}
          onDone={() => void reloadAll()}
          onSignIn={() => setDialog({ kind: "login" })}
          onScan={() => setDialog({ kind: "scan" })}
        />
      )}
      {dialog?.kind === "scan" && <ScanDialog onClose={() => setDialog(undefined)} onDone={() => void reloadAll()} />}
      {dialog?.kind === "key" && (
        <KeyDialog account={dialog.account} onClose={() => setDialog(undefined)} onDone={() => void reloadAll()} />
      )}
      {dialog?.kind === "login" && (
        <LoginDialog alias={dialog.alias} onClose={() => setDialog(undefined)} onDone={() => void reloadAll()} />
      )}
      {dialog?.kind === "transport" && (
        <TransportDialog
          account={dialog.account}
          web={dialog.web}
          onClose={() => setDialog(undefined)}
          onDone={() => void reloadAll()}
        />
      )}
      {dialog?.kind === "subpools" && (
        <SubpoolManager
          state={subpools.data}
          accounts={accounts.data ?? []}
          onClose={() => setDialog(undefined)}
          onDone={() => void reloadAll()}
        />
      )}
      {dialog?.kind === "keysExport" && (
        <KeysExportDialog accounts={accounts.data ?? []} onClose={() => setDialog(undefined)} />
      )}
      {dialog?.kind === "keysImport" && (
        <KeysImportDialog onClose={() => setDialog(undefined)} onDone={() => void reloadAll()} />
      )}
      {dialog?.kind === "endpoint" && (
        <EndpointDialog
          provider={dialog.provider}
          accounts={byProvider.get(dialog.provider.slug) ?? []}
          endpoint={endpoints.data?.find((e) => e.provider === dialog.provider.slug)}
          statuses={statuses.data}
          onClose={() => setDialog(undefined)}
          onDone={() => void reloadAll()}
        />
      )}
      {dialog?.kind === "plan" && (
        <PlanDialog
          account={dialog.account}
          usage={dialog.usage}
          onClose={() => setDialog(undefined)}
          onDone={() => void reloadAll()}
        />
      )}
    </Page>
  );
}

/** A drag handle for cards (ExecPlan 022/023). */
function DragGrip(props: { grip: GripProps | undefined; label: string }) {
  if (!props.grip) return null;
  return (
    <span className="drag-grip" role="button" tabIndex={0} aria-label={props.label} title={props.label} {...props.grip}>
      <GripVertical size={16} aria-hidden />
    </span>
  );
}

/** Header controls of a provider: dedicated API, usage source, removal. */
function ProviderControls(props: {
  provider: ProviderInfo;
  /** Shown before the other controls. */
  leading?: ReactNode;
  usageSources: string[];
  /** The provider serves inference with API keys, so a dedicated endpoint makes sense. */
  inference: boolean;
  endpoint: EndpointInfo | undefined;
  busy: boolean;
  onAction: (f: () => Promise<unknown>) => void;
  onDialog: (d: Dialog) => void;
}) {
  const p = props.provider;
  const confirm = useConfirm();
  const source = typeof p.settings?.usage_source === "string" ? p.settings.usage_source : "auto";
  return (
    <div className="provider-controls">
      {props.leading}
      {props.inference && (
        <EndpointButton endpoint={props.endpoint} onOpen={() => props.onDialog({ kind: "endpoint", provider: p })} />
      )}
      {props.usageSources.length > 0 && (
        <label className="select-label" title={t("acc.usageSourceHint")}>
          {t("acc.usageSource")}
          <select
            value={source}
            disabled={props.busy}
            onChange={(e) => {
              const value = e.target.value;
              props.onAction(() => api.setProviderSettings(p.slug, { usage_source: value }));
            }}
          >
            {["auto", ...props.usageSources, "off"].map((v) => (
              <option key={v} value={v}>
                {tv("usageSource", v)}
              </option>
            ))}
          </select>
        </label>
      )}
      <IconButton
        label={t("acc.removeProvider")}
        danger
        disabled={props.busy}
        onClick={() =>
          void confirm({
            title: t("acc.removeProviderTitle", { slug: p.slug }),
            message: t("acc.confirmRemoveProvider", { slug: p.slug }),
            confirmLabel: t("acc.removeProvider"),
            danger: true,
          }).then((ok) => ok && props.onAction(() => api.removeProvider(p.slug)))
        }
      >
        <Trash size={16} />
      </IconButton>
    </div>
  );
}

/** What every card on the board needs from the page. */
interface BoardCtx {
  kindOf: (p: ProviderInfo) => ProviderKind | undefined;
  accountsOf: (slug: string) => AccountInfo[];
  usageOf: (a: AccountInfo) => AccountUsage | undefined;
  /** Not in the active sub-pool (ExecPlan 019). */
  outside: (a: AccountInfo) => boolean;
  /** Route health, for the account badges (ExecPlan 021). */
  statuses: TargetStatus[] | undefined;
  endpointOf: (slug: string) => EndpointInfo | undefined;
  busy: boolean;
  onAction: (f: () => Promise<unknown>) => void;
  onBench: (a: AccountInfo) => void;
  /** A provider-wide check in progress. */
  checking: CheckAllProgress | undefined;
  onCheckAll: (p: ProviderInfo, accounts: AccountInfo[]) => void;
  onDialog: (d: Dialog) => void;
  onReorderAccounts: (provider: string, aliases: string[]) => void;
}

const isInference = (k: ProviderKind | undefined) => k?.auth === "api_key" || k?.auth === "optional_api_key";

function ProviderSection(props: {
  provider: ProviderInfo;
  /** Accounts shown (after the search filter). */
  accounts: AccountInfo[];
  ctx: BoardCtx;
  /** Board drag (provider order, ExecPlan 023). */
  boardGrip: GripProps | undefined;
  boardClass: string;
}) {
  const p = props.provider;
  const c = props.ctx;
  const kind = c.kindOf(p);
  const signIn = kind?.auth === "oauth";
  const web = kind?.auth === "browser_login";
  const all = c.accountsOf(p.slug).map((a) => a.alias);
  // Account order is also the failover order (ExecPlan 022).
  const cards = usePointerReorder({
    scope: p.slug,
    scopeAttr: "section",
    itemAttr: "alias",
    ids: all,
    onReorder: (aliases) => c.onReorderAccounts(p.slug, aliases),
  });
  // One account fills a regular board cell; several share one wider panel.
  const n = props.accounts.length;
  const span = n <= 1 ? "single" : n >= 4 ? "multi span-all" : `multi span-${n}`;
  // The cards that have a check button (see AccountCard); one click checks them all.
  const optIn = signIn || web || !!kind?.requires_transport_opt_in;
  const checkable = web ? [] : props.accounts.filter((a) => optIn || a.account_type !== "subscription");
  const progress = c.checking?.slug === p.slug ? c.checking : undefined;
  return (
    <section className={`provider-group ${span} ${props.boardClass}`} data-board-item={p.slug}>
      <div className="provider-head">
        <DragGrip grip={props.boardGrip} label={t("acc.dragProviderHint")} />
        <div className="provider-title">
          <div className="row" style={{ gap: 8, minWidth: 0 }}>
            <h2 className="ellipsis" title={p.display_name}>
              {p.display_name}
            </h2>
            {!namesSlug(p) && <span className="mono small muted nowrap">{p.slug}</span>}
          </div>
          {p.base_url && (
            <div className="mono small muted ellipsis" title={p.base_url}>
              {p.base_url}
            </div>
          )}
        </div>
        <ProviderControls
          provider={p}
          leading={
            checkable.length > 1 && (
              <Button
                icon={<Stethoscope size={16} />}
                disabled={c.busy}
                title={t("acc.checkAllHint")}
                onClick={() => c.onCheckAll(p, checkable)}
              >
                {progress
                  ? t("acc.checkAllProgress", { done: progress.done, total: progress.total })
                  : t("acc.checkAll")}
              </Button>
            )
          }
          usageSources={kind?.usage_sources ?? []}
          inference={isInference(kind)}
          endpoint={c.endpointOf(p.slug)}
          busy={c.busy}
          onAction={c.onAction}
          onDialog={c.onDialog}
        />
      </div>
      {props.accounts.length === 0 ? (
        <Empty>{t("acc.noAccounts")}</Empty>
      ) : (
        <div className="account-grid" data-section={p.slug}>
          {props.accounts.map((a) => (
            <div key={a.alias} data-alias={a.alias} className={`account-slot ${cards.stateClass(a.alias)}`}>
              <AccountCard
                account={a}
                usage={c.usageOf(a)}
                outside={c.outside(a)}
                health={accountHealth(c.statuses, a)}
                signIn={signIn}
                requiresTransportOptIn={kind?.requires_transport_opt_in}
                managed={kind?.auth === "external_managed"}
                web={web}
                busy={c.busy}
                onAction={c.onAction}
                onBench={c.onBench}
                onDialog={c.onDialog}
                grip={cards.grip(a.alias)}
              />
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

/** A display name such as "SenseNova 商汤日日新" already tells which slug it is. */
function namesSlug(p: ProviderInfo) {
  const plain = (text: string) => text.toLowerCase().replace(/[^a-z0-9]/g, "");
  return plain(p.slug) !== "" && plain(p.display_name).startsWith(plain(p.slug));
}

/** Kinds shown together as one ChatGPT card: usage from the Codex CLI, the opt-in Codex
 * subscription transport (ADR-012) and the opt-in web session (ADR-014). */
export const CHATGPT_KINDS = ["codex", "codex-subscription"] as const;
export const CHATGPT_ITEM = "chatgpt";

/**
 * One card for everything ChatGPT. It starts as the Codex usage; the subscription transport
 * and the web session join it as they are turned on, and the card widens. The quota is shown
 * once, from the first account that reports it; the others only show their own requests.
 */
function ChatGptSection(props: {
  providers: ProviderInfo[];
  /** Accounts shown, after the search filter. */
  visible: (a: AccountInfo) => boolean;
  ctx: BoardCtx;
  boardGrip: GripProps | undefined;
  boardClass: string;
}) {
  const c = props.ctx;
  const byKind = (kind: string) => props.providers.find((p) => p.kind === kind);
  const features = [
    { kind: "codex", title: t("cg.usage"), provider: byKind("codex") },
    { kind: "codex-subscription", title: t("cg.proxy"), provider: byKind("codex-subscription") },
  ];
  const accountsOf = (p: ProviderInfo | undefined) => (p ? c.accountsOf(p.slug).filter(props.visible) : []);
  const aliases: string[] = [];
  for (const f of features) for (const a of accountsOf(f.provider)) if (!aliases.includes(a.alias)) aliases.push(a.alias);
  const widest = Math.max(
    1,
    ...aliases.map((alias) => features.filter((f) => accountsOf(f.provider).some((a) => a.alias === alias)).length),
  );
  const span = widest <= 1 ? "single" : `multi span-${widest}`;
  const has = (kind: string) => accountsOf(features.find((f) => f.kind === kind)?.provider).length > 0;

  return (
    <section className={`provider-group chatgpt-group ${span} ${props.boardClass}`} data-board-item={CHATGPT_ITEM}>
      <div className="provider-head">
        <DragGrip grip={props.boardGrip} label={t("acc.dragProviderHint")} />
        <div className="provider-title">
          <h2 className="ellipsis">{t("cg.title")}</h2>
        </div>
      </div>
      {aliases.map((alias) => {
        const present = features
          .map((f) => ({ ...f, account: accountsOf(f.provider).find((a) => a.alias === alias) }))
          .filter((f): f is typeof f & { account: AccountInfo; provider: ProviderInfo } => !!f.account && !!f.provider);
        // The quota of a ChatGPT account is one number: show it from the first source that has it.
        const primary =
          present.find((f) => (c.usageOf(f.account)?.windows.length ?? 0) > 0) ?? present[0];
        return (
          <div key={alias} className="cg-features">
            {present.map((f) => {
              const kind = c.kindOf(f.provider);
              return (
                <div key={f.kind} className="cg-feature">
                  <div className="cg-feature-head">
                    <span className="strong">{f.title}</span>
                    <span className="mono small muted nowrap">{f.provider.slug}</span>
                    <ProviderControls
                      provider={f.provider}
                      usageSources={kind?.usage_sources ?? []}
                      inference={false}
                      endpoint={undefined}
                      busy={c.busy}
                      onAction={c.onAction}
                      onDialog={c.onDialog}
                    />
                  </div>
                  <AccountCard
                    account={f.account}
                    usage={c.usageOf(f.account)}
                    outside={c.outside(f.account)}
                    health={accountHealth(c.statuses, f.account)}
                    signIn={kind?.auth === "oauth"}
                    web={kind?.auth === "browser_login"}
                    busy={c.busy}
                    onAction={c.onAction}
                    onBench={c.onBench}
                    onDialog={c.onDialog}
                    hideUsage={f !== primary}
                  />
                </div>
              );
            })}
          </div>
        );
      })}
      {(!has("codex-subscription")) && (
        <div className="cg-add">
          {!has("codex-subscription") && (
            <Button icon={<LogIn size={16} />} onClick={() => c.onDialog({ kind: "login" })} title={t("sub.tileHint")}>
              {t("cg.addProxy")}
            </Button>
          )}
        </div>
      )}
    </section>
  );
}

function AccountCard(props: {
  account: AccountInfo;
  usage: AccountUsage | undefined;
  outside: boolean;
  health: ReturnType<typeof accountHealth>;
  signIn: boolean;
  requiresTransportOptIn?: boolean;
  /** Credentials live in the app it connects to, not here. */
  managed?: boolean;
  web: boolean;
  busy: boolean;
  onAction: (f: () => Promise<unknown>) => void;
  onBench: (a: AccountInfo) => void;
  onDialog: (d: Dialog) => void;
  /** Handlers of the drag grip; absent when the card cannot be reordered. */
  grip?: GripProps;
  /** Another card already shows this account's quota: only show its own requests. */
  hideUsage?: boolean;
}) {
  const a = props.account;
  const u = props.usage;
  const confirm = useConfirm();
  const toast = useToast();
  // Official-CLI subscription logins only show usage; Loadout sign-ins and browser sessions
  // can route.
  const optIn = props.signIn || props.web || props.requiresTransportOptIn;
  const usageOnly = a.account_type === "subscription" && !optIn;
  const routable = !usageOnly && (!optIn || a.transport_opt_in);
  const loginExpired = a.login_expires_at_ms != null && a.login_expires_at_ms <= Date.now();
  const name = `${a.provider}/${a.alias}`;
  const subtitle = [a.display_name, u?.account_label].filter(Boolean).join(" · ");
  return (
    <div className={`account-card ${a.enabled ? "" : "off"} ${props.outside ? "outside" : ""}`}>
      <div className="account-head">
        <DragGrip grip={props.grip} label={t("acc.dragHint")} />
        <Switch
          checked={a.enabled}
          disabled={props.busy}
          label={a.enabled ? t("common.disable") : t("common.enable")}
          onChange={(on) => props.onAction(() => api.setAccountEnabled(a.provider, a.alias, on))}
        />
        <div className="account-name">
          {/* The provider is already in the section header above. */}
          <div className="ellipsis" title={name}>
            {a.alias}
          </div>
          {subtitle && <div className="small muted ellipsis">{subtitle}</div>}
        </div>
        <div className="account-actions">
          {routable && !props.web && (
            <IconButton label={t("acc.bench")} disabled={props.busy || !a.enabled} onClick={() => props.onBench(a)}>
              <Zap size={16} />
            </IconButton>
          )}
          {!usageOnly && !props.web && (
            <IconButton
              label={t("acc.check")}
              disabled={props.busy}
              onClick={() =>
                props.onAction(async () => {
                  const [text, tone] = checkMessage(name, await api.checkAccount(a.provider, a.alias));
                  toast(text, tone);
                })
              }
            >
              <Stethoscope size={16} />
            </IconButton>
          )}
          {!usageOnly && (
            <IconButton
              label={t("acc.sync")}
              disabled={props.busy}
              onClick={() => props.onAction(() => api.syncModels(a.provider, a.alias))}
            >
              <RefreshCw size={16} />
            </IconButton>
          )}
          {props.signIn && (
            <IconButton
              label={t("sub.relogin")}
              disabled={props.busy}
              onClick={() => props.onDialog({ kind: "login", alias: a.alias })}
            >
              <LogIn size={16} />
            </IconButton>
          )}
          {!usageOnly && !optIn && !props.managed && (
            <IconButton
              label={a.has_key ? t("acc.changeKey") : t("acc.setKey")}
              disabled={props.busy}
              onClick={() => props.onDialog({ kind: "key", account: a })}
            >
              <KeyRound size={16} />
            </IconButton>
          )}
          <IconButton
            label={t("acc.plan")}
            disabled={props.busy}
            onClick={() => props.onDialog({ kind: "plan", account: a, usage: u })}
          >
            <Receipt size={16} />
          </IconButton>
          <IconButton
            label={t("common.remove")}
            danger
            disabled={props.busy}
            onClick={() =>
              void confirm({
                title: t("acc.removeTitle", { name }),
                message: t("acc.confirmRemove", { name }),
                confirmLabel: t("common.remove"),
                danger: true,
              }).then((ok) => ok && props.onAction(() => api.removeAccount(a.provider, a.alias)))
            }
          >
            <Trash size={16} />
          </IconButton>
        </div>
      </div>
      {/* Full width under the header so long states never cover the buttons. */}
      {(props.health || props.outside || (a.unavailable_models ?? []).length > 0) && (
        <div className="account-status">
          <HealthBadge health={props.health} />
          {(a.unavailable_models ?? []).length > 0 && (
            <Badge tone="warn" title={(a.unavailable_models ?? []).join(", ")}>
              <span className="ellipsis">
                {t("acc.notInPlan", { n: (a.unavailable_models ?? []).length, list: (a.unavailable_models ?? []).join(", ") })}
              </span>
            </Badge>
          )}
          {props.outside && (
            <Badge tone="warn" title={t("sp.outside")}>
              {t("sp.outside")}
            </Badge>
          )}
        </div>
      )}
      <div className="account-tags">
        {(props.managed || a.account_type === "subscription") && (
          <Badge tone="accent">{props.managed ? t("acc.managedBadge") : tv("accountType", a.account_type)}</Badge>
        )}
        {!a.enabled && <Badge>{t("common.disabled")}</Badge>}
        {usageOnly && <Badge title={t("acc.usageOnlyHint")}>{t("acc.usageOnly")}</Badge>}
        {optIn &&
          (a.transport_opt_in ? (
            <Badge tone="warn" dot>
              {t("sub.routingOn")}
            </Badge>
          ) : (
            <Badge title={t("sub.routingOffHint")}>{t("sub.routingOff")}</Badge>
          ))}
        {/* Opt-in transports never join loadout/realtime|fast|default (ADR-012, ADR-017). */}
        {optIn && a.transport_opt_in && (
          <Badge title={t("sub.notInBuiltinHint", { name: `${name}/<model>` })}>{t("sub.notInBuiltin")}</Badge>
        )}
        {!usageOnly && (
          <span className="small muted">
            {t(routable ? "acc.modelsRouted" : "acc.modelsSelected", { on: a.enabled_models, all: a.models })}
          </span>
        )}
        {!usageOnly && !optIn && !a.has_key && <Badge tone="warn">{t("acc.noKey")}</Badge>}
        {props.signIn && loginExpired && <Badge tone="bad">{t("sub.loginExpired")}</Badge>}
        {u?.plan?.name && <Badge tone="muted">{u.plan.name}</Badge>}
      </div>
      {optIn && (
        <div className="row wrap" style={{ gap: 8 }}>
          {a.transport_opt_in ? (
            <Button
              disabled={props.busy}
              onClick={() =>
                void confirm({
                  title: t("sub.turnOffTitle", { name }),
                  message: t("sub.turnOffBody", { name }),
                  confirmLabel: t("sub.turnOff"),
                }).then((ok) => ok && props.onAction(() => api.setTransport(a.provider, a.alias, false, null)))
              }
            >
              {t("sub.turnOff")}
            </Button>
          ) : (
            <Button
              icon={<ShieldAlert size={16} />}
              disabled={props.busy || loginExpired}
              onClick={() => props.onDialog({ kind: "transport", account: a, web: props.web })}
            >
              {t("sub.turnOn")}
            </Button>
          )}
          {a.login_expires_at_ms != null && !loginExpired && (
            <span className="small muted">{t("sub.loginValid", { date: date(a.login_expires_at_ms) })}</span>
          )}
        </div>
      )}
      {u &&
        (props.hideUsage ? (
          <div className="account-foot">
            <span>{t("acc.today", { n: u.spend_today.requests, cost: usd(u.spend_today.cost_usd) })}</span>
            <span>{t("acc.month", { n: u.spend_month.requests, cost: usd(u.spend_month.cost_usd) })}</span>
          </div>
        ) : (
          <UsageDetail usage={u} />
        ))}
    </div>
  );
}

function UsageDetail(props: { usage: AccountUsage }) {
  const u = props.usage;
  const price = u.plan?.price_usd_monthly;
  return (
    <>
      <div className="account-body">
        {u.error && (
          <div className="error small">{/API key not available/.test(u.error) ? t("acc.usageNoReply") : u.error}</div>
        )}
        {u.windows.map((w) => (
          <QuotaBar
            key={w.scope}
            label={w.label}
            remaining={w.remaining_percent}
            detail={
              w.used != null && w.limit != null && w.unit !== "percent"
                ? `${num(w.used)} / ${num(w.limit)} ${w.unit}`
                : undefined
            }
            resets={resetIn(w.resets_at_ms)}
          />
        ))}
        {u.balances.map((b) => (
          <div key={b.currency} className="row">
            <span className="quota-label">{t("acc.balance")}</span>
            <span className="spacer" />
            <span className="balance-value">
              {b.currency} {b.amount.toFixed(2)}
            </span>
            {(b.granted != null || b.topup != null) && (
              <span className="small muted">
                {b.topup != null && `${t("acc.topup", { n: b.topup.toFixed(2) })} `}
                {b.granted != null && t("acc.granted", { n: b.granted.toFixed(2) })}
              </span>
            )}
          </div>
        ))}
      </div>
      <div className="account-foot">
        <span>{t("acc.today", { n: u.spend_today.requests, cost: usd(u.spend_today.cost_usd) })}</span>
        <span>{t("acc.month", { n: u.spend_month.requests, cost: usd(u.spend_month.cost_usd) })}</span>
        {price != null && <span>{t("acc.perMonth", { price: usd(price) })}</span>}
        {u.plan?.renews_at_ms != null && <span>{t("acc.renews", { date: date(u.plan.renews_at_ms) })}</span>}
        {price != null && price > 0 && u.spend_month.cost_usd > 0 && (
          <span title={t("acc.valueHint")}>{t("acc.value", { x: (u.spend_month.cost_usd / price).toFixed(2) })}</span>
        )}
        {u.supported && u.fetched_at_ms != null && (
          <span>
            {t("acc.usageFrom", {
              source: tv("dataSource", u.source ?? ""),
              confidence: tv("confidence", u.confidence ?? ""),
              when: ago(u.fetched_at_ms),
            })}
          </span>
        )}
        {u.supported && u.source && u.fetched_at_ms == null && <span>{t("acc.usageNotRead")}</span>}
      </div>
    </>
  );
}

const KIND_GROUPS: { id: ProviderKind["group"]; label: MessageKey }[] = [
  { id: "platform", label: "add.groupPlatform" },
  { id: "relay", label: "add.groupRelay" },
  { id: "local", label: "add.groupLocal" },
  { id: "custom", label: "add.groupCustom" },
];

/** Pick a platform from the tiles, then fill in only what that platform needs. */
function AddAccountDialog(props: { providers: ProviderInfo[]; onClose: () => void; onDone: () => void; onSignIn: () => void; onScan: () => void }) {
  const kinds = usePoll(api.providerKinds, 0);
  const action = useAction();
  const [kind, setKind] = useState("");
  const [query, setQuery] = useState("");
  const [slug, setSlug] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [alias, setAlias] = useState("main");
  const [key, setKey] = useState("");
  const [result, setResult] = useState<string>();

  const all = kinds.data ?? [];
  const selected: ProviderKind | undefined = all.find((k) => k.kind === kind);
  const effectiveSlug = selected?.fixed_slug ?? slug;
  const exists = props.providers.some((p) => p.slug === effectiveSlug);
  const needsUrl = selected && !selected.fixed_slug && !exists;
  const q = query.trim().toLowerCase();
  const tiles = all.filter((k) => !q || `${k.kind} ${k.display_name}`.toLowerCase().includes(q));

  const submit = () =>
    void action.run(async () => {
      if (!selected) throw new Error(t("acc.errKind"));
      if (!effectiveSlug) throw new Error(t("acc.errSlug"));
      if (!exists) {
        await api.addProvider({
          slug: effectiveSlug,
          kind: selected.kind,
          base_url: baseUrl.trim() || null,
          settings: {},
          display_name: null,
        });
      }
      const added = await api.addAccount({
        provider: effectiveSlug,
        alias: alias.trim(),
        api_key: key.trim() || null,
        display_name: null,
      });
      setKey("");
      props.onDone();
      if (added.sync_error) {
        setResult(t("acc.syncFailed", { error: added.sync_error }));
      } else {
        props.onClose();
      }
    });

  if (!selected) {
    return (
      <Modal title={t("acc.addTitle")} onClose={props.onClose} wide>
        <label className="search" style={{ marginBottom: 14 }}>
          <Search size={16} aria-hidden />
          <input placeholder={t("add.search")} aria-label={t("add.search")} value={query} autoFocus onChange={(e) => setQuery(e.target.value)} />
        </label>
        {tiles.some((k) => k.group === "subscription") && (
          <div className="tile-group">
            <div className="small muted">{t("add.groupSubscription")}</div>
            <div className="tiles">
              {tiles
                .filter((k) => k.group === "subscription")
                .map((k) => (
                  <button
                    type="button"
                    className="tile"
                    key={k.kind}
                    title={k.hint}
                    onClick={() => (k.auth === "oauth" ? props.onSignIn() : props.onScan())}
                  >
                    <span className="mark">{k.display_name.slice(0, 1)}</span>
                    <span className="row-main">
                      <span className="strong ellipsis">{k.display_name}</span>
                      <span className="small muted ellipsis">{k.hint}</span>
                    </span>
                  </button>
                ))}
            </div>
          </div>
        )}
        {KIND_GROUPS.map((g) => {
          const list = tiles.filter((k) => k.group === g.id);
          if (!list.length) return null;
          return (
            <div className="tile-group" key={g.id}>
              <div className="small muted">{t(g.label)}</div>
              <div className="tiles">
                {list.map((k) => {
                  const used = props.providers.some((p) => p.slug === k.fixed_slug);
                  return (
                    <button type="button" className={`tile ${k.fixed_slug ? "" : "dashed"}`} key={k.kind} onClick={() => setKind(k.kind)}>
                      <span className="mark">{k.display_name.slice(0, 1)}</span>
                      <span className="row-main">
                        <span className="strong ellipsis">{k.display_name}</span>
                        <span className="small muted ellipsis">
                          {used ? t("add.addAnother") : (k.default_base_url?.replace(/^https?:\/\//, "").replace(/\/.*$/, "") ?? t("add.anyUrl"))}
                        </span>
                      </span>
                    </button>
                  );
                })}
              </div>
            </div>
          );
        })}
        {!tiles.length && <Empty>{t("acc.noMatch")}</Empty>}
      </Modal>
    );
  }

  return (
    <Modal
      title={t("add.formTitle", { name: selected.display_name })}
      onClose={props.onClose}
      footer={
        <>
          <Button onClick={() => (result ? props.onClose() : setKind(""))}>{result ? t("common.close") : t("keys.back")}</Button>
          {!result && (
            <Button kind="primary" disabled={action.busy} onClick={submit}>
              {action.busy ? t("acc.adding") : t("common.add")}
            </Button>
          )}
        </>
      }
    >
      {!selected.fixed_slug && (
        <Field label={t("acc.slug")} hint={t("acc.slugHint")}>
          <input value={slug} autoFocus onChange={(e) => setSlug(e.target.value.toLowerCase())} placeholder="my-llm" />
        </Field>
      )}
      {needsUrl && (
        <Field label={t("acc.baseUrl")} hint={t("acc.baseUrlHint")}>
          <input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder={selected.default_base_url ?? "https://"} />
        </Field>
      )}
      <Field label={t("acc.alias")} hint={t("acc.aliasHint")}>
        <input value={alias} onChange={(e) => setAlias(e.target.value.toLowerCase())} />
      </Field>
      <Field label={selected.auth === "optional_api_key" ? t("acc.keyOptional") : t("acc.key")}>
        <input type="password" value={key} autoFocus={!!selected.fixed_slug} onChange={(e) => setKey(e.target.value)} autoComplete="off" />
      </Field>
      <ErrorText error={action.error} />
      {result && <Notice tone="warn">{result}</Notice>}
    </Modal>
  );
}

function ScanDialog(props: { onClose: () => void; onDone: () => void }) {
  const tools = usePoll(api.scan, 0);
  const action = useAction();
  const toast = useToast();
  return (
    <Modal title={t("acc.scanTitle")} onClose={props.onClose} wide>
      <ErrorText error={action.error ?? tools.error?.message} />
      {tools.loading && <Empty>{t("acc.scanning")}</Empty>}
      <div className="table-wrap">
        <table>
          <thead>
            <tr>
              <th>{t("acc.tool")}</th>
              <th>{t("acc.installed")}</th>
              <th>{t("acc.signedIn")}</th>
              <th>{t("acc.use")}</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {(tools.data ?? []).map((tool) => (
              <tr key={tool.kind}>
                <td>
                  <div className="strong">{tool.display_name}</div>
                  {tool.detail && <div className="small muted">{tool.detail}</div>}
                </td>
                <td>
                  {tool.installed ? <Badge tone="ok">{t("common.yes")}</Badge> : <Badge>{t("common.no")}</Badge>}
                </td>
                <td>
                  {tool.logged_in ? (
                    tool.expired ? (
                      <Badge tone="warn">{t("acc.expired")}</Badge>
                    ) : (
                      <Badge tone="ok">{tool.auth ?? t("common.yes")}</Badge>
                    )
                  ) : (
                    <Badge>{t("common.no")}</Badge>
                  )}
                </td>
                <td className="small">{tool.inference ? t("acc.requestsAndUsage") : t("acc.usageOnly")}</td>
                <td className="actions-cell">
                  {tool.imported ? (
                    <Badge tone="info">{t("acc.connected")}</Badge>
                  ) : tool.inference ? (
                    <Button
                      disabled={!tool.logged_in || action.busy}
                      onClick={() =>
                        void action.run(async () => {
                          await api.connectTool(tool.kind);
                          toast(t("acc.connecting", { name: tool.display_name }), "info");
                          props.onDone();
                          props.onClose();
                        })
                      }
                    >
                      {t("acc.connect")}
                    </Button>
                  ) : (
                    <span className="small muted">{t("acc.readOnly")}</span>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Modal>
  );
}

function KeyDialog(props: { account: AccountInfo; onClose: () => void; onDone: () => void }) {
  const action = useAction();
  const [key, setKey] = useState("");
  const a = props.account;
  return (
    <Modal
      title={t("acc.keyTitle", { name: `${a.provider}/${a.alias}` })}
      onClose={props.onClose}
      footer={
        <>
          <Button onClick={props.onClose}>{t("common.cancel")}</Button>
          <Button
            kind="primary"
            disabled={action.busy || !key.trim()}
            onClick={() =>
              void action.run(async () => {
                await api.setAccountKey(a.provider, a.alias, key.trim());
                setKey("");
                props.onDone();
                props.onClose();
              })
            }
          >
            {t("common.save")}
          </Button>
        </>
      }
    >
      <Field label={t("acc.newKey")}>
        <input type="password" value={key} onChange={(e) => setKey(e.target.value)} autoComplete="off" />
      </Field>
      <ErrorText error={action.error} />
    </Modal>
  );
}

function PlanDialog(props: { account: AccountInfo; usage?: AccountUsage; onClose: () => void; onDone: () => void }) {
  const action = useAction();
  const plan = props.usage?.plan;
  const [name, setName] = useState(plan?.name ?? "");
  const [price, setPrice] = useState(plan?.price_usd_monthly?.toString() ?? "");
  const [renews, setRenews] = useState(
    plan?.renews_at_ms ? new Date(plan.renews_at_ms).toISOString().slice(0, 10) : "",
  );
  const a = props.account;
  return (
    <Modal
      title={t("acc.planTitle", { name: `${a.provider}/${a.alias}` })}
      onClose={props.onClose}
      footer={
        <>
          <Button onClick={props.onClose}>{t("common.cancel")}</Button>
          <Button
            kind="primary"
            disabled={action.busy}
            onClick={() =>
              void action.run(async () => {
                const p = price.trim() ? Number(price) : null;
                if (p != null && (!Number.isFinite(p) || p < 0)) throw new Error(t("acc.errPrice"));
                await api.setPlan({
                  provider: a.provider,
                  alias: a.alias,
                  name: name.trim() || null,
                  price_usd_monthly: p,
                  renews_at_ms: renews ? Date.parse(`${renews}T00:00:00Z`) : null,
                });
                props.onDone();
                props.onClose();
              })
            }
          >
            {t("common.save")}
          </Button>
        </>
      }
    >
      <Field label={t("acc.planNameField")}>
        <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Pro" />
      </Field>
      <Field label={t("acc.price")}>
        <input value={price} onChange={(e) => setPrice(e.target.value)} inputMode="decimal" />
      </Field>
      <Field label={t("acc.nextRenewal")}>
        <input type="date" value={renews} onChange={(e) => setRenews(e.target.value)} />
      </Field>
      <ErrorText error={action.error} />
    </Modal>
  );
}

/** Device-code sign-in for a Loadout-managed subscription login (ADR-012). */
function LoginDialog(props: { alias?: string; onClose: () => void; onDone: () => void }) {
  const action = useAction();
  const [alias, setAlias] = useState(props.alias ?? "main");
  const [start, setStart] = useState<OAuthStart>();
  const [done, setDone] = useState<AccountInfo>();
  const [failed, setFailed] = useState<string>();
  const { onDone } = props;

  useEffect(() => {
    if (!start || done || failed != null) return;
    const id = window.setInterval(() => {
      api
        .oauthStatus(start.login_id)
        .then((s) => {
          if (s.state === "done" && s.account) {
            setDone(s.account);
            onDone();
          } else if (s.state === "failed") {
            setFailed(s.message ?? "");
          }
        })
        .catch((e: unknown) => setFailed(e instanceof Error ? e.message : String(e)));
    }, 2000);
    return () => window.clearInterval(id);
  }, [start, done, failed, onDone]);

  const begin = () =>
    void action.run(async () => {
      setFailed(undefined);
      const s = await api.oauthStart(SUBSCRIPTION_SLUG, alias.trim());
      setStart(s);
      await api.openSignInPage(s.verification_url).catch(() => undefined);
    });

  return (
    <Modal
      title={t("sub.loginTitle")}
      onClose={props.onClose}
      footer={
        <>
          <Button onClick={props.onClose}>{done ? t("common.close") : t("common.cancel")}</Button>
          {!start && (
            <Button kind="primary" icon={<LogIn size={16} />} disabled={action.busy || !alias.trim()} onClick={begin}>
              {t("sub.start")}
            </Button>
          )}
        </>
      }
    >
      <Notice>{t("sub.loginIntro")}</Notice>
      <Field label={t("acc.alias")} hint={t("acc.aliasHint")}>
        <input
          value={alias}
          disabled={start != null || props.alias != null}
          onChange={(e) => setAlias(e.target.value.toLowerCase())}
        />
      </Field>
      {start && !done && failed == null && (
        <>
          <Field label={t("sub.code")}>
            <div className="signin-code mono">{start.user_code}</div>
          </Field>
          <CopyField value={start.verification_url} />
          <div className="row" style={{ marginTop: 8 }}>
            <Button icon={<ExternalLink size={16} />} onClick={() => void api.openSignInPage(start.verification_url)}>
              {t("sub.openPage")}
            </Button>
            <span className="small muted">{t("sub.waiting")}</span>
          </div>
        </>
      )}
      {done && (
        <Notice>
          {t("sub.done", { name: done.display_name ?? `${done.provider}/${done.alias}`, models: done.models })}{" "}
          {t("sub.routingOffHint")}
        </Notice>
      )}
      {failed != null && <Notice tone="warn">{t("sub.failed", { error: failed })}</Notice>}
      <ErrorText error={action.error} />
    </Modal>
  );
}

/** The two confirmation steps before routing through a lower-trust transport (ADR-012 §1,
 * ADR-014 §8). */
function TransportDialog(props: { account: AccountInfo; web?: boolean; onClose: () => void; onDone: () => void }) {
  const action = useAction();
  const a = props.account;
  const name = `${a.provider}/${a.alias}`;
  const [understood, setUnderstood] = useState(false);
  const [typed, setTyped] = useState("");
  const ready = understood && typed.trim() === a.alias;
  return (
    <Modal
      title={t("sub.riskTitle", { name })}
      onClose={props.onClose}
      footer={
        <>
          <Button onClick={props.onClose}>{t("common.cancel")}</Button>
          <Button
            kind="danger"
            disabled={!ready || action.busy}
            onClick={() =>
              void action.run(async () => {
                await api.setTransport(a.provider, a.alias, true, typed.trim());
                props.onDone();
                props.onClose();
              })
            }
          >
            {t("sub.enable")}
          </Button>
        </>
      }
    >
      <Notice tone="warn">
        <ul className="risk-list">
          <li>{t("sub.risk1")}</li>
          <li>{t("sub.risk2")}</li>
          <li>{t("sub.risk3")}</li>
          <li>{t("sub.risk4")}</li>
        </ul>
      </Notice>
      <label className="row check-row">
        <input type="checkbox" checked={understood} onChange={(e) => setUnderstood(e.target.checked)} />
        <span>{t("sub.understand")}</span>
      </label>
      <Field label={t("sub.typeAlias", { alias: a.alias })}>
        <input value={typed} disabled={!understood} onChange={(e) => setTyped(e.target.value)} autoComplete="off" />
      </Field>
      <ErrorText error={action.error} />
    </Modal>
  );
}
