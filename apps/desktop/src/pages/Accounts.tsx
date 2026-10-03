// SPDX-License-Identifier: Apache-2.0

import { useMemo, useState } from "react";
import { KeyRound, Plus, Receipt, RefreshCw, ScanSearch, Search, Trash, Zap } from "lucide-react";
import {
  api,
  type AccountInfo,
  type AccountUsage,
  type ModelInfo,
  type ProviderInfo,
  type ProviderKind,
} from "../api";
import {
  Badge,
  Button,
  Card,
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
import { t, tv } from "../i18n";

type Dialog =
  | { kind: "add" }
  | { kind: "scan" }
  | { kind: "key"; account: AccountInfo }
  | { kind: "plan"; account: AccountInfo; usage?: AccountUsage };

export function Accounts() {
  const providers = usePoll(api.providers, 10000);
  const accounts = usePoll(api.accounts, 10000);
  const usage = usePoll(() => api.usage(false), 10000);
  const kinds = usePoll(api.providerKinds, 60000);
  const models = usePoll(api.models, 30000);
  const action = useAction();
  const toast = useToast();
  const [dialog, setDialog] = useState<Dialog>();
  const [only, setOnly] = useState<string>();
  const [query, setQuery] = useState("");

  const reloadAll = async () => {
    await Promise.all([providers.reload(), accounts.reload(), usage.reload(), models.reload()]);
  };

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
  const shown = (providers.data ?? []).filter(
    (p) => (!only || p.slug === only) && (!q || p.slug.includes(q) || (byProvider.get(p.slug) ?? []).some(matches)),
  );

  const onAction = (f: () => Promise<unknown>) =>
    void action.run(async () => {
      await f();
      await reloadAll();
    });

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

  return (
    <Page
      title={t("acc.title")}
      actions={
        <>
          <Button
            icon={<RefreshCw size={16} />}
            disabled={action.busy}
            onClick={() => void action.run(async () => api.usage(true).then(() => usage.reload()))}
          >
            {t("acc.refresh")}
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
          <div className="provider-grid">
            {(providers.data ?? []).map((p) => (
              <button
                key={p.slug}
                type="button"
                className={`provider-tile ${only === p.slug ? "active" : ""}`}
                aria-pressed={only === p.slug}
                title={only === p.slug ? t("acc.showAll") : t("acc.showOnly", { name: p.display_name })}
                onClick={() => setOnly(only === p.slug ? undefined : p.slug)}
              >
                <span className="tile-name ellipsis">{p.display_name}</span>
                <span className="mono small muted">{p.slug}</span>
                <span className="tile-count">{t("acc.accountCount", { n: p.accounts })}</span>
              </button>
            ))}
            {/* ExecPlan 010 adds the "Codex subscription" sign-in tile here (ADR-012). */}
          </div>
          <div className="row" style={{ marginBottom: 16 }}>
            <label className="search" style={{ flex: 1, maxWidth: 420 }}>
              <Search size={16} aria-hidden />
              <input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t("acc.search")}
                aria-label={t("acc.search")}
              />
            </label>
            {only && (
              <Button onClick={() => setOnly(undefined)}>{t("acc.showAll")}</Button>
            )}
          </div>
        </>
      )}

      {shown.map((p) => (
        <ProviderSection
          key={p.slug}
          provider={p}
          usageSources={kinds.data?.find((k) => k.kind === p.kind)?.usage_sources ?? []}
          accounts={(byProvider.get(p.slug) ?? []).filter(matches)}
          usageOf={usageOf}
          busy={action.busy}
          onAction={onAction}
          onBench={bench}
          onDialog={setDialog}
        />
      ))}
      {shown.length === 0 && (providers.data ?? []).length > 0 && <Empty>{t("acc.noMatch")}</Empty>}

      {dialog?.kind === "add" && (
        <AddAccountDialog
          providers={providers.data ?? []}
          onClose={() => setDialog(undefined)}
          onDone={() => void reloadAll()}
        />
      )}
      {dialog?.kind === "scan" && <ScanDialog onClose={() => setDialog(undefined)} onDone={() => void reloadAll()} />}
      {dialog?.kind === "key" && (
        <KeyDialog account={dialog.account} onClose={() => setDialog(undefined)} onDone={() => void reloadAll()} />
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

function ProviderSection(props: {
  provider: ProviderInfo;
  usageSources: string[];
  accounts: AccountInfo[];
  usageOf: (a: AccountInfo) => AccountUsage | undefined;
  busy: boolean;
  onAction: (f: () => Promise<unknown>) => void;
  onBench: (a: AccountInfo) => void;
  onDialog: (d: Dialog) => void;
}) {
  const p = props.provider;
  const confirm = useConfirm();
  const source = typeof p.settings?.usage_source === "string" ? p.settings.usage_source : "auto";
  return (
    <section className="provider-section">
      <div className="provider-head">
        <h2>{p.display_name}</h2>
        <span className="mono small muted">{p.slug}</span>
        {p.base_url && (
          <span className="mono small muted ellipsis" title={p.base_url}>
            {p.base_url}
          </span>
        )}
        <div className="spacer" />
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
      {props.accounts.length === 0 ? (
        <Card>
          <Empty>{t("acc.noAccounts")}</Empty>
        </Card>
      ) : (
        <div className="account-grid">
          {props.accounts.map((a) => (
            <AccountCard
              key={a.alias}
              account={a}
              usage={props.usageOf(a)}
              busy={props.busy}
              onAction={props.onAction}
              onBench={props.onBench}
              onDialog={props.onDialog}
            />
          ))}
        </div>
      )}
    </section>
  );
}

function AccountCard(props: {
  account: AccountInfo;
  usage: AccountUsage | undefined;
  busy: boolean;
  onAction: (f: () => Promise<unknown>) => void;
  onBench: (a: AccountInfo) => void;
  onDialog: (d: Dialog) => void;
}) {
  const a = props.account;
  const u = props.usage;
  const confirm = useConfirm();
  const subscription = a.account_type === "subscription";
  const name = `${a.provider}/${a.alias}`;
  const subtitle = [a.display_name, u?.account_label].filter(Boolean).join(" · ");
  return (
    <div className={`account-card ${a.enabled ? "" : "off"}`}>
      <div className="account-head">
        <Switch
          checked={a.enabled}
          disabled={props.busy}
          label={a.enabled ? t("common.disable") : t("common.enable")}
          onChange={(on) => props.onAction(() => api.setAccountEnabled(a.provider, a.alias, on))}
        />
        <div className="account-name">
          <div className="ellipsis">{name}</div>
          {subtitle && <div className="small muted ellipsis">{subtitle}</div>}
        </div>
        <div className="account-actions">
          {!subscription && (
            <IconButton label={t("acc.bench")} disabled={props.busy || !a.enabled} onClick={() => props.onBench(a)}>
              <Zap size={16} />
            </IconButton>
          )}
          {!subscription && (
            <IconButton
              label={t("acc.sync")}
              disabled={props.busy}
              onClick={() => props.onAction(() => api.syncModels(a.provider, a.alias))}
            >
              <RefreshCw size={16} />
            </IconButton>
          )}
          {!subscription && (
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
      <div className="account-tags">
        <Badge tone={subscription ? "accent" : "info"}>{tv("accountType", a.account_type)}</Badge>
        {!a.enabled && <Badge>{t("common.disabled")}</Badge>}
        {subscription ? (
          <Badge title={t("acc.usageOnlyHint")}>{t("acc.usageOnly")}</Badge>
        ) : (
          <span className="small muted">{t("acc.modelsRouted", { on: a.enabled_models, all: a.models })}</span>
        )}
        {!subscription && !a.has_key && <Badge tone="warn">{t("acc.noKey")}</Badge>}
        {u?.plan?.name && <Badge tone="muted">{u.plan.name}</Badge>}
      </div>
      {u && <UsageDetail usage={u} />}
    </div>
  );
}

function UsageDetail(props: { usage: AccountUsage }) {
  const u = props.usage;
  const price = u.plan?.price_usd_monthly;
  const hasData = u.windows.length > 0 || u.balances.length > 0;
  return (
    <>
      <div className="account-body">
        {u.error && <div className="error small">{u.error}</div>}
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
        {!hasData && !u.error && (
          <div className="small muted">{u.supported ? t("dash.noData") : t("acc.usageEstimated")}</div>
        )}
      </div>
      <div className="account-foot">
        <span>{t("acc.today", { n: u.spend_today.requests, cost: usd(u.spend_today.cost_usd) })}</span>
        <span>{t("acc.month", { n: u.spend_month.requests, cost: usd(u.spend_month.cost_usd) })}</span>
        {price != null && <span>{t("acc.perMonth", { price: usd(price) })}</span>}
        {u.plan?.renews_at_ms != null && <span>{t("acc.renews", { date: date(u.plan.renews_at_ms) })}</span>}
        {price != null && price > 0 && u.spend_month.cost_usd > 0 && (
          <span title={t("acc.valueHint")}>{t("acc.value", { x: (u.spend_month.cost_usd / price).toFixed(2) })}</span>
        )}
        {u.supported && (
          <span>
            {t("acc.usageFrom", {
              source: u.source ?? "",
              confidence: u.confidence ?? "",
              when: ago(u.fetched_at_ms),
            })}
          </span>
        )}
      </div>
    </>
  );
}

function AddAccountDialog(props: { providers: ProviderInfo[]; onClose: () => void; onDone: () => void }) {
  const kinds = usePoll(api.providerKinds, 0);
  const action = useAction();
  const [kind, setKind] = useState("");
  const [slug, setSlug] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [alias, setAlias] = useState("main");
  const [key, setKey] = useState("");
  const [result, setResult] = useState<string>();

  const inferenceKinds = (kinds.data ?? []).filter((k) => k.auth !== "external_managed");
  const selected: ProviderKind | undefined = inferenceKinds.find((k) => k.kind === kind);
  const effectiveSlug = selected?.fixed_slug ?? slug;
  const exists = props.providers.some((p) => p.slug === effectiveSlug);
  const needsUrl = selected && !selected.default_base_url && !exists;

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

  return (
    <Modal
      title={t("acc.addTitle")}
      onClose={props.onClose}
      footer={
        <>
          <Button onClick={props.onClose}>{result ? t("common.close") : t("common.cancel")}</Button>
          {!result && (
            <Button kind="primary" disabled={action.busy} onClick={submit}>
              {action.busy ? t("acc.adding") : t("common.add")}
            </Button>
          )}
        </>
      }
    >
      <Field label={t("acc.kind")}>
        <select value={kind} onChange={(e) => setKind(e.target.value)}>
          <option value="">{t("acc.choose")}</option>
          {inferenceKinds.map((k) => (
            <option key={k.kind} value={k.kind}>
              {k.display_name}
            </option>
          ))}
        </select>
      </Field>
      {selected && !selected.fixed_slug && (
        <Field label={t("acc.slug")} hint={t("acc.slugHint")}>
          <input value={slug} onChange={(e) => setSlug(e.target.value.toLowerCase())} placeholder="my-llm" />
        </Field>
      )}
      {needsUrl && (
        <Field label={t("acc.baseUrl")} hint={t("acc.baseUrlHint")}>
          <input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="https://" />
        </Field>
      )}
      <Field label={t("acc.alias")} hint={t("acc.aliasHint")}>
        <input value={alias} onChange={(e) => setAlias(e.target.value.toLowerCase())} />
      </Field>
      <Field label={selected?.auth === "optional_api_key" ? t("acc.keyOptional") : t("acc.key")} hint={t("acc.keyHint")}>
        <input type="password" value={key} onChange={(e) => setKey(e.target.value)} autoComplete="off" />
      </Field>
      <ErrorText error={action.error} />
      {result && <Notice tone="warn">{result}</Notice>}
    </Modal>
  );
}

function ScanDialog(props: { onClose: () => void; onDone: () => void }) {
  const tools = usePoll(api.scan, 0);
  const action = useAction();
  return (
    <Modal title={t("acc.scanTitle")} onClose={props.onClose} wide>
      <Notice>{t("acc.scanIntro")}</Notice>
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
                    <Badge tone="info">{t("acc.imported")}</Badge>
                  ) : (
                    <Button
                      disabled={!tool.logged_in || action.busy}
                      onClick={() =>
                        void action.run(async () => {
                          await api.importTool(tool.kind, null);
                          await tools.reload();
                          props.onDone();
                        })
                      }
                    >
                      {t("acc.import")}
                    </Button>
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
      <Field label={t("acc.newKey")} hint={t("acc.newKeyHint")}>
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
      <p className="small muted" style={{ marginTop: 0 }}>
        {t("acc.planIntro")}
      </p>
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
