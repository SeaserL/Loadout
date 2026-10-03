// SPDX-License-Identifier: Apache-2.0

import { useMemo, useState } from "react";
import {
  api,
  type AccountInfo,
  type AccountUsage,
  type ProviderInfo,
  type ProviderKind,
} from "../api";
import { Badge, Button, Card, Empty, ErrorText, Field, Meter, Modal, Page } from "../components";
import { ago, date, num, until, usd } from "../format";
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
  const action = useAction();
  const [dialog, setDialog] = useState<Dialog>();

  const reloadAll = async () => {
    await Promise.all([providers.reload(), accounts.reload(), usage.reload()]);
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

  return (
    <Page
      title={t("acc.title")}
      actions={
        <>
          <Button
            disabled={action.busy}
            onClick={() => void action.run(async () => api.usage(true).then(() => usage.reload()))}
          >
            {t("acc.refresh")}
          </Button>
          <Button onClick={() => setDialog({ kind: "scan" })}>{t("acc.scan")}</Button>
          <Button kind="primary" onClick={() => setDialog({ kind: "add" })}>
            {t("acc.add")}
          </Button>
        </>
      }
    >
      <ErrorText error={action.error ?? providers.error?.message ?? accounts.error?.message} />
      {(providers.data ?? []).length === 0 && !providers.loading && (
        <Card>
          <Empty>{t("acc.empty")}</Empty>
        </Card>
      )}
      {(providers.data ?? []).map((p) => (
        <ProviderCard
          key={p.slug}
          provider={p}
          usageSources={kinds.data?.find((k) => k.kind === p.kind)?.usage_sources ?? []}
          accounts={byProvider.get(p.slug) ?? []}
          usageOf={usageOf}
          busy={action.busy}
          onAction={(f) =>
            void action.run(async () => {
              await f();
              await reloadAll();
            })
          }
          onDialog={setDialog}
        />
      ))}

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

function ProviderCard(props: {
  provider: ProviderInfo;
  usageSources: string[];
  accounts: AccountInfo[];
  usageOf: (a: AccountInfo) => AccountUsage | undefined;
  busy: boolean;
  onAction: (f: () => Promise<unknown>) => void;
  onDialog: (d: Dialog) => void;
}) {
  const p = props.provider;
  const source = typeof p.settings?.usage_source === "string" ? p.settings.usage_source : "auto";
  return (
    <Card
      title={
        <span>
          {p.display_name} <span className="muted small mono">{p.slug}</span>
        </span>
      }
      actions={
        <>
          {props.usageSources.length > 0 && (
            <label className="row small" title={t("acc.usageSourceHint")}>
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
          <Button
          kind="danger"
          disabled={props.busy}
          onClick={() => {
            if (confirm(t("acc.confirmRemoveProvider", { slug: p.slug }))) {
              props.onAction(() => api.removeProvider(p.slug));
            }
          }}
        >
          {t("acc.removeProvider")}
        </Button>
        </>
      }
    >
      {p.base_url && <div className="small muted mono">{p.base_url}</div>}
      {props.accounts.length === 0 && <Empty>{t("acc.noAccounts")}</Empty>}
      {props.accounts.map((a) => (
        <AccountRow key={a.alias} account={a} usage={props.usageOf(a)} {...props} />
      ))}
    </Card>
  );
}

function AccountRow(props: {
  account: AccountInfo;
  usage: AccountUsage | undefined;
  busy: boolean;
  onAction: (f: () => Promise<unknown>) => void;
  onDialog: (d: Dialog) => void;
}) {
  const a = props.account;
  const u = props.usage;
  const subscription = a.account_type === "subscription";
  const name = `${a.provider}/${a.alias}`;
  return (
    <div style={{ borderTop: "1px solid var(--border)", paddingTop: 12, marginTop: 12 }}>
      <div className="row wrap">
        <strong>{name}</strong>
        {a.display_name && <span className="muted">{a.display_name}</span>}
        <Badge tone={a.enabled ? "ok" : "muted"}>{a.enabled ? t("common.enabled") : t("common.disabled")}</Badge>
        <Badge tone="info">{tv("accountType", a.account_type)}</Badge>
        {subscription ? (
          <Badge title={t("acc.usageOnlyHint")}>{t("acc.usageOnly")}</Badge>
        ) : (
          <span className="small muted">{t("acc.modelsRouted", { on: a.enabled_models, all: a.models })}</span>
        )}
        {u?.account_label && <span className="small muted">{u.account_label}</span>}
        <div className="spacer" />
        {!subscription && (
          <Button disabled={props.busy} onClick={() => props.onAction(() => api.syncModels(a.provider, a.alias))}>
            {t("acc.sync")}
          </Button>
        )}
        {!subscription && (
          <Button disabled={props.busy} onClick={() => props.onDialog({ kind: "key", account: a })}>
            {a.has_key ? t("acc.changeKey") : t("acc.setKey")}
          </Button>
        )}
        <Button disabled={props.busy} onClick={() => props.onDialog({ kind: "plan", account: a, usage: u })}>
          {t("acc.plan")}
        </Button>
        <Button
          disabled={props.busy}
          onClick={() => props.onAction(() => api.setAccountEnabled(a.provider, a.alias, !a.enabled))}
        >
          {a.enabled ? t("common.disable") : t("common.enable")}
        </Button>
        <Button
          kind="danger"
          disabled={props.busy}
          onClick={() => {
            if (confirm(t("acc.confirmRemove", { name }))) {
              props.onAction(() => api.removeAccount(a.provider, a.alias));
            }
          }}
        >
          {t("common.remove")}
        </Button>
      </div>
      {u && <UsageDetail usage={u} />}
    </div>
  );
}

function UsageDetail(props: { usage: AccountUsage }) {
  const u = props.usage;
  const price = u.plan?.price_usd_monthly;
  return (
    <div style={{ marginTop: 10 }}>
      {u.error && <div className="error small">{u.error}</div>}
      <div className="grid cols-3" style={{ gap: 10 }}>
        {u.windows.map((w) => (
          <div key={w.scope}>
            <div className="small">
              {w.label}
              {w.used != null && w.limit != null && w.unit !== "percent" && (
                <span className="muted">
                  {" "}
                  · {num(w.used)} / {num(w.limit)} {w.unit}
                </span>
              )}
            </div>
            <Meter remaining={w.remaining_percent} />
            <div className="small muted">{t("common.resets", { when: until(w.resets_at_ms) })}</div>
          </div>
        ))}
        {u.balances.map((b) => (
          <div key={b.currency}>
            <div className="small">{t("acc.balance")}</div>
            <div style={{ fontSize: 18, fontWeight: 600 }}>
              {b.currency} {b.amount.toFixed(2)}
            </div>
            {(b.granted != null || b.topup != null) && (
              <div className="small muted">
                {b.topup != null && `${t("acc.topup", { n: b.topup.toFixed(2) })} `}
                {b.granted != null && t("acc.granted", { n: b.granted.toFixed(2) })}
              </div>
            )}
          </div>
        ))}
      </div>
      <div className="row wrap small muted" style={{ marginTop: 8, gap: 16 }}>
        {u.plan?.name && <span>{t("acc.planName", { name: u.plan.name })}</span>}
        {price != null && <span>{t("acc.perMonth", { price: usd(price) })}</span>}
        {u.plan?.renews_at_ms != null && <span>{t("acc.renews", { date: date(u.plan.renews_at_ms) })}</span>}
        <span>{t("acc.today", { n: u.spend_today.requests, cost: usd(u.spend_today.cost_usd) })}</span>
        <span>{t("acc.month", { n: u.spend_month.requests, cost: usd(u.spend_month.cost_usd) })}</span>
        {price != null && price > 0 && u.spend_month.cost_usd > 0 && (
          <span title={t("acc.valueHint")}>{t("acc.value", { x: (u.spend_month.cost_usd / price).toFixed(2) })}</span>
        )}
        <span>
          {u.supported
            ? t("acc.usageFrom", {
                source: u.source ?? "",
                confidence: u.confidence ?? "",
                when: ago(u.fetched_at_ms),
              })
            : t("acc.usageEstimated")}
        </span>
      </div>
    </div>
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
    <Modal title={t("acc.addTitle")} onClose={props.onClose}>
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
      {result && <div className="notice small">{result}</div>}
      <div className="row">
        <div className="spacer" />
        <Button onClick={props.onClose}>{result ? t("common.close") : t("common.cancel")}</Button>
        {!result && (
          <Button kind="primary" disabled={action.busy} onClick={submit}>
            {action.busy ? t("acc.adding") : t("common.add")}
          </Button>
        )}
      </div>
    </Modal>
  );
}

function ScanDialog(props: { onClose: () => void; onDone: () => void }) {
  const tools = usePoll(api.scan, 0);
  const action = useAction();
  return (
    <Modal title={t("acc.scanTitle")} onClose={props.onClose} wide>
      <p className="small muted">{t("acc.scanIntro")}</p>
      <ErrorText error={action.error ?? tools.error?.message} />
      {tools.loading && <Empty>{t("acc.scanning")}</Empty>}
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
                {tool.display_name}
                {tool.detail && <div className="small muted">{tool.detail}</div>}
              </td>
              <td>{tool.installed ? <Badge tone="ok">{t("common.yes")}</Badge> : <Badge>{t("common.no")}</Badge>}</td>
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
              <td className="num">
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
    </Modal>
  );
}

function KeyDialog(props: { account: AccountInfo; onClose: () => void; onDone: () => void }) {
  const action = useAction();
  const [key, setKey] = useState("");
  const a = props.account;
  return (
    <Modal title={t("acc.keyTitle", { name: `${a.provider}/${a.alias}` })} onClose={props.onClose}>
      <Field label={t("acc.newKey")} hint={t("acc.newKeyHint")}>
        <input type="password" value={key} onChange={(e) => setKey(e.target.value)} autoComplete="off" />
      </Field>
      <ErrorText error={action.error} />
      <div className="row">
        <div className="spacer" />
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
      </div>
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
    <Modal title={t("acc.planTitle", { name: `${a.provider}/${a.alias}` })} onClose={props.onClose}>
      <p className="small muted">{t("acc.planIntro")}</p>
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
      <div className="row">
        <div className="spacer" />
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
      </div>
    </Modal>
  );
}
