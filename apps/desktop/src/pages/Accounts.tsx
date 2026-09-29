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

type Dialog =
  | { kind: "add" }
  | { kind: "scan" }
  | { kind: "key"; account: AccountInfo }
  | { kind: "plan"; account: AccountInfo; usage?: AccountUsage };

export function Accounts() {
  const providers = usePoll(api.providers, 10000);
  const accounts = usePoll(api.accounts, 10000);
  const usage = usePoll(() => api.usage(false), 10000);
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
      title="Accounts & usage"
      actions={
        <>
          <Button
            disabled={action.busy}
            onClick={() => void action.run(async () => api.usage(true).then(() => usage.reload()))}
          >
            Refresh usage
          </Button>
          <Button onClick={() => setDialog({ kind: "scan" })}>Scan this PC</Button>
          <Button kind="primary" onClick={() => setDialog({ kind: "add" })}>
            Add account
          </Button>
        </>
      }
    >
      <ErrorText error={action.error ?? providers.error?.message ?? accounts.error?.message} />
      {(providers.data ?? []).length === 0 && !providers.loading && (
        <Card>
          <Empty>
            No providers yet. Add an API account, or scan this PC for Codex, Claude Code, Gemini CLI and
            Kimi Code logins.
          </Empty>
        </Card>
      )}
      {(providers.data ?? []).map((p) => (
        <ProviderCard
          key={p.slug}
          provider={p}
          accounts={byProvider.get(p.slug) ?? []}
          usageOf={usageOf}
          busy={action.busy}
          onAction={(f) => void action.run(async () => { await f(); await reloadAll(); })}
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
  accounts: AccountInfo[];
  usageOf: (a: AccountInfo) => AccountUsage | undefined;
  busy: boolean;
  onAction: (f: () => Promise<unknown>) => void;
  onDialog: (d: Dialog) => void;
}) {
  const p = props.provider;
  return (
    <Card
      title={
        <span>
          {p.display_name} <span className="muted small mono">{p.slug}</span>
        </span>
      }
      actions={
        <Button
          kind="danger"
          disabled={props.busy}
          onClick={() => {
            if (confirm(`Remove ${p.slug} with all its accounts, keys and models?`)) {
              props.onAction(() => api.removeProvider(p.slug));
            }
          }}
        >
          Remove provider
        </Button>
      }
    >
      {p.base_url && <div className="small muted mono">{p.base_url}</div>}
      {props.accounts.length === 0 && <Empty>No accounts.</Empty>}
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
  return (
    <div style={{ borderTop: "1px solid var(--border)", paddingTop: 12, marginTop: 12 }}>
      <div className="row wrap">
        <strong>
          {a.provider}/{a.alias}
        </strong>
        {a.display_name && <span className="muted">{a.display_name}</span>}
        <Badge tone={a.enabled ? "ok" : "muted"}>{a.enabled ? "enabled" : "disabled"}</Badge>
        <Badge tone="info">{a.account_type}</Badge>
        {subscription ? (
          <Badge title="Subscription logins are used for usage display only (ADR-009)">usage only</Badge>
        ) : (
          <span className="small muted">
            {a.enabled_models}/{a.models} models routed
          </span>
        )}
        {u?.account_label && <span className="small muted">{u.account_label}</span>}
        <div className="spacer" />
        {!subscription && (
          <Button disabled={props.busy} onClick={() => props.onAction(() => api.syncModels(a.provider, a.alias))}>
            Sync models
          </Button>
        )}
        {!subscription && (
          <Button disabled={props.busy} onClick={() => props.onDialog({ kind: "key", account: a })}>
            {a.has_key ? "Change key" : "Set key"}
          </Button>
        )}
        <Button disabled={props.busy} onClick={() => props.onDialog({ kind: "plan", account: a, usage: u })}>
          Plan
        </Button>
        <Button
          disabled={props.busy}
          onClick={() => props.onAction(() => api.setAccountEnabled(a.provider, a.alias, !a.enabled))}
        >
          {a.enabled ? "Disable" : "Enable"}
        </Button>
        <Button
          kind="danger"
          disabled={props.busy}
          onClick={() => {
            if (confirm(`Remove ${a.provider}/${a.alias}? Its key is deleted from Windows Credential Manager.`)) {
              props.onAction(() => api.removeAccount(a.provider, a.alias));
            }
          }}
        >
          Remove
        </Button>
      </div>
      {u && <UsageDetail usage={u} />}
    </div>
  );
}

function UsageDetail(props: { usage: AccountUsage }) {
  const u = props.usage;
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
            <div className="small muted">resets {until(w.resets_at_ms)}</div>
          </div>
        ))}
        {u.balances.map((b) => (
          <div key={b.currency}>
            <div className="small">Balance</div>
            <div style={{ fontSize: 18, fontWeight: 600 }}>
              {b.currency} {b.amount.toFixed(2)}
            </div>
            {(b.granted != null || b.topup != null) && (
              <div className="small muted">
                {b.topup != null && `top-up ${b.topup.toFixed(2)} `}
                {b.granted != null && `granted ${b.granted.toFixed(2)}`}
              </div>
            )}
          </div>
        ))}
      </div>
      <div className="row wrap small muted" style={{ marginTop: 8, gap: 16 }}>
        {u.plan?.name && <span>Plan: {u.plan.name}</span>}
        {u.plan?.price_usd_monthly != null && <span>{usd(u.plan.price_usd_monthly)}/month</span>}
        {u.plan?.renews_at_ms != null && <span>renews {date(u.plan.renews_at_ms)}</span>}
        <span>
          Loadout today: {u.spend_today.requests} req · {usd(u.spend_today.cost_usd)}
        </span>
        <span>
          this month: {u.spend_month.requests} req · {usd(u.spend_month.cost_usd)}
        </span>
        {u.plan?.price_usd_monthly != null && u.plan.price_usd_monthly > 0 && u.spend_month.cost_usd > 0 && (
          <span title="Equivalent API value of this month's requests ÷ subscription price">
            value captured {(u.spend_month.cost_usd / u.plan.price_usd_monthly).toFixed(2)}×
          </span>
        )}
        <span>
          {u.supported ? `usage ${u.source ?? ""} ${u.confidence ?? ""} · ${ago(u.fetched_at_ms)}` : "usage estimated from Loadout's own requests"}
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
      if (!selected) throw new Error("Choose a provider type.");
      if (!effectiveSlug) throw new Error("Enter a short name for this provider.");
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
        setResult(`Account added, but the model list could not be fetched: ${added.sync_error}`);
      } else {
        props.onClose();
      }
    });

  return (
    <Modal title="Add an API account" onClose={props.onClose}>
      <Field label="Provider type">
        <select value={kind} onChange={(e) => setKind(e.target.value)}>
          <option value="">Choose…</option>
          {inferenceKinds.map((k) => (
            <option key={k.kind} value={k.kind}>
              {k.display_name}
            </option>
          ))}
        </select>
      </Field>
      {selected && !selected.fixed_slug && (
        <Field label="Provider name" hint="Used in model names, e.g. my-llm/main/model-id. Lowercase letters, digits, dashes.">
          <input value={slug} onChange={(e) => setSlug(e.target.value.toLowerCase())} placeholder="my-llm" />
        </Field>
      )}
      {needsUrl && (
        <Field label="Base URL" hint="Including the version path, e.g. https://api.example.com/v1">
          <input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="https://" />
        </Field>
      )}
      <Field label="Account name" hint="Distinguishes several accounts of one provider, e.g. main, work.">
        <input value={alias} onChange={(e) => setAlias(e.target.value.toLowerCase())} />
      </Field>
      <Field
        label={selected?.auth === "optional_api_key" ? "API key (optional)" : "API key"}
        hint="Stored in Windows Credential Manager; never in Loadout's database or logs."
      >
        <input type="password" value={key} onChange={(e) => setKey(e.target.value)} autoComplete="off" />
      </Field>
      <ErrorText error={action.error} />
      {result && <div className="notice small">{result}</div>}
      <div className="row">
        <div className="spacer" />
        <Button onClick={props.onClose}>{result ? "Close" : "Cancel"}</Button>
        {!result && (
          <Button kind="primary" disabled={action.busy} onClick={submit}>
            {action.busy ? "Adding…" : "Add"}
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
    <Modal title="AI tools on this PC" onClose={props.onClose} wide>
      <p className="small muted">
        Loadout only reads whether these tools are installed and signed in. Importing a subscription login
        shows its quota; requests are never sent with subscription credentials.
      </p>
      <ErrorText error={action.error ?? tools.error?.message} />
      {tools.loading && <Empty>Scanning…</Empty>}
      <table>
        <thead>
          <tr>
            <th>Tool</th>
            <th>Installed</th>
            <th>Signed in</th>
            <th>Use</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {(tools.data ?? []).map((t) => (
            <tr key={t.kind}>
              <td>
                {t.display_name}
                {t.detail && <div className="small muted">{t.detail}</div>}
              </td>
              <td>{t.installed ? <Badge tone="ok">yes</Badge> : <Badge>no</Badge>}</td>
              <td>
                {t.logged_in ? (
                  t.expired ? (
                    <Badge tone="warn">expired</Badge>
                  ) : (
                    <Badge tone="ok">{t.auth ?? "yes"}</Badge>
                  )
                ) : (
                  <Badge>no</Badge>
                )}
              </td>
              <td className="small">{t.inference ? "requests + usage" : "usage only"}</td>
              <td className="num">
                {t.imported ? (
                  <Badge tone="info">imported</Badge>
                ) : (
                  <Button
                    disabled={!t.logged_in || action.busy}
                    onClick={() =>
                      void action.run(async () => {
                        await api.importTool(t.kind, null);
                        await tools.reload();
                        props.onDone();
                      })
                    }
                  >
                    Import
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
    <Modal title={`API key for ${a.provider}/${a.alias}`} onClose={props.onClose}>
      <Field label="New API key" hint="Replaces the stored key and clears any authentication error.">
        <input type="password" value={key} onChange={(e) => setKey(e.target.value)} autoComplete="off" />
      </Field>
      <ErrorText error={action.error} />
      <div className="row">
        <div className="spacer" />
        <Button onClick={props.onClose}>Cancel</Button>
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
          Save
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
    <Modal title={`Plan of ${a.provider}/${a.alias}`} onClose={props.onClose}>
      <p className="small muted">
        For renewal reminders and the subscription value shown on this page. The renewal date is separate
        from quota resets.
      </p>
      <Field label="Plan name">
        <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Pro" />
      </Field>
      <Field label="Price (USD per month)">
        <input value={price} onChange={(e) => setPrice(e.target.value)} inputMode="decimal" />
      </Field>
      <Field label="Next renewal">
        <input type="date" value={renews} onChange={(e) => setRenews(e.target.value)} />
      </Field>
      <ErrorText error={action.error} />
      <div className="row">
        <div className="spacer" />
        <Button onClick={props.onClose}>Cancel</Button>
        <Button
          kind="primary"
          disabled={action.busy}
          onClick={() =>
            void action.run(async () => {
              const p = price.trim() ? Number(price) : null;
              if (p != null && (!Number.isFinite(p) || p < 0)) throw new Error("Enter a valid price.");
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
          Save
        </Button>
      </div>
    </Modal>
  );
}
