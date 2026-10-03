// SPDX-License-Identifier: Apache-2.0

import { useMemo, useState } from "react";
import { Copy, Plus, Search, Trash } from "lucide-react";
import { api, type ModelInfo } from "../api";
import {
  Badge,
  Button,
  Card,
  Empty,
  ErrorText,
  Field,
  IconButton,
  Modal,
  Page,
  Switch,
  tierTone,
  useConfirm,
  useCopy,
} from "../components";
import { num, tierLabel } from "../format";
import { useAction, usePoll } from "../hooks";
import { t } from "../i18n";

function tierOptions() {
  return [
    { value: "auto", label: t("models.tierMeasured") },
    { value: "ultra-fast", label: t("models.tierPinned", { tier: tierLabel("ULTRA_FAST") }) },
    { value: "fast", label: t("models.tierPinned", { tier: tierLabel("FAST") }) },
    { value: "normal", label: t("models.tierPinned", { tier: tierLabel("NORMAL") }) },
  ];
}

function tierValue(m: ModelInfo): string {
  switch (m.tier_override) {
    case "ULTRA_FAST":
      return "ultra-fast";
    case "FAST":
      return "fast";
    case "NORMAL":
      return "normal";
    default:
      return "auto";
  }
}

export function Models() {
  const models = usePoll(api.models, 10000);
  const routes = usePoll(api.routeStatus, 10000);
  const accounts = usePoll(api.accounts, 0);
  const action = useAction();
  const confirm = useConfirm();
  const doCopy = useCopy();
  const [filter, setFilter] = useState("");
  const [showDisabled, setShowDisabled] = useState(false);
  const [price, setPrice] = useState<ModelInfo>();
  const [adding, setAdding] = useState(false);

  const tierOf = useMemo(() => {
    const m = new Map<string, string>();
    for (const r of routes.data ?? []) m.set(r.name, r.tier);
    return m;
  }, [routes.data]);

  const all = models.data ?? [];
  const enabledCount = all.filter((m) => m.enabled).length;
  const list = all.filter(
    (m) => (showDisabled || m.enabled) && m.name.toLowerCase().includes(filter.toLowerCase()),
  );
  const run = (f: () => Promise<unknown>) =>
    void action.run(async () => {
      await f();
      await Promise.all([models.reload(), routes.reload()]);
    });
  const options = tierOptions();

  return (
    <Page
      title={t("models.title")}
      actions={
        <Button kind="primary" icon={<Plus size={16} />} onClick={() => setAdding(true)}>
          {t("models.addManual")}
        </Button>
      }
    >
      <div className="row wrap" style={{ marginBottom: 16, gap: 14 }}>
        <label className="search" style={{ flex: 1, maxWidth: 420 }}>
          <Search size={16} aria-hidden />
          <input
            placeholder={t("models.search")}
            aria-label={t("models.search")}
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
        </label>
        <span className="check-row">
          <Switch checked={showDisabled} onChange={setShowDisabled} label={t("common.showDisabled")} />
          {t("common.showDisabled")}
        </span>
        <span className="spacer" />
        <span className="small muted">{t("models.count", { on: enabledCount, all: all.length })}</span>
      </div>
      <ErrorText error={action.error ?? models.error?.message} />
      <Card>
        {list.length === 0 && !models.loading && <Empty>{t("models.empty")}</Empty>}
        {list.length > 0 && (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th style={{ width: 48 }}>{t("models.on")}</th>
                  <th>{t("models.name")}</th>
                  <th>{t("models.protocols")}</th>
                  <th className="num">{t("models.context")}</th>
                  <th className="num" style={{ whiteSpace: "normal", width: 110 }}>
                    {t("models.price")}
                  </th>
                  <th>{t("models.tier")}</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {list.map((m) => {
                  const tier = tierOf.get(m.name) ?? "UNMEASURED";
                  return (
                    <tr key={m.name} className={m.enabled ? "" : "dim"}>
                      <td>
                        <Switch
                          checked={m.enabled}
                          disabled={action.busy}
                          label={m.enabled ? t("common.disable") : t("common.enable")}
                          onChange={(on) => run(() => api.setModelEnabled(m.name, on))}
                        />
                      </td>
                      <td>
                        <div className="row" style={{ gap: 4 }}>
                          <span className="mono">{m.name}</span>
                          <IconButton label={t("models.copy")} onClick={() => doCopy(m.name)}>
                            <Copy size={14} />
                          </IconButton>
                          {m.source === "manual" && <Badge>{t("models.manual")}</Badge>}
                        </div>
                      </td>
                      <td className="small">{m.protocols.join(", ")}</td>
                      <td className="num">{num(m.context_window)}</td>
                      <td className="num">
                        <button className="btn link" onClick={() => setPrice(m)} title={t("models.priceTitle", { name: m.name })}>
                          {m.input_price ?? "-"} / {m.output_price ?? "-"}
                        </button>
                      </td>
                      <td>
                        <div className="row">
                          <Badge tone={tierTone(tier)}>{tierLabel(tier)}</Badge>
                          <select
                            value={tierValue(m)}
                            disabled={action.busy || !m.enabled}
                            aria-label={t("models.tier")}
                            style={{ width: 132 }}
                            onChange={(e) =>
                              run(() => api.setModelTier(m.name, e.target.value === "auto" ? null : e.target.value))
                            }
                          >
                            {options.map((o) => (
                              <option key={o.value} value={o.value}>
                                {o.label}
                              </option>
                            ))}
                          </select>
                        </div>
                      </td>
                      <td className="actions-cell">
                        {m.source === "manual" && (
                          <IconButton
                            label={t("common.remove")}
                            danger
                            disabled={action.busy}
                            onClick={() =>
                              void confirm({
                                title: t("models.removeTitle", { name: m.name }),
                                message: t("models.confirmRemove"),
                                confirmLabel: t("common.remove"),
                                danger: true,
                              }).then((ok) => ok && run(() => api.removeModel(m.name)))
                            }
                          >
                            <Trash size={16} />
                          </IconButton>
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </Card>
      {price && <PriceDialog model={price} onClose={() => setPrice(undefined)} onDone={() => void models.reload()} />}
      {adding && (
        <AddModelDialog
          accounts={(accounts.data ?? [])
            .filter((a) => a.account_type !== "subscription")
            .map((a) => `${a.provider}/${a.alias}`)}
          onClose={() => setAdding(false)}
          onDone={() => void models.reload()}
        />
      )}
    </Page>
  );
}

function PriceDialog(props: { model: ModelInfo; onClose: () => void; onDone: () => void }) {
  const action = useAction();
  const [input, setInput] = useState(props.model.input_price?.toString() ?? "");
  const [output, setOutput] = useState(props.model.output_price?.toString() ?? "");
  const parse = (v: string) => {
    if (!v.trim()) return null;
    const n = Number(v);
    if (!Number.isFinite(n) || n < 0) throw new Error(t("models.errPrice"));
    return n;
  };
  return (
    <Modal
      title={t("models.priceTitle", { name: props.model.name })}
      onClose={props.onClose}
      footer={
        <>
          <Button onClick={props.onClose}>{t("common.cancel")}</Button>
          <Button
            kind="primary"
            disabled={action.busy}
            onClick={() =>
              void action.run(async () => {
                await api.setModelPrice(props.model.name, parse(input), parse(output));
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
        {t("models.priceIntro")}
      </p>
      <div className="grid cols-2" style={{ gap: 12 }}>
        <Field label={t("models.input")}>
          <input value={input} onChange={(e) => setInput(e.target.value)} inputMode="decimal" />
        </Field>
        <Field label={t("models.output")}>
          <input value={output} onChange={(e) => setOutput(e.target.value)} inputMode="decimal" />
        </Field>
      </div>
      <ErrorText error={action.error} />
    </Modal>
  );
}

function AddModelDialog(props: { accounts: string[]; onClose: () => void; onDone: () => void }) {
  const action = useAction();
  const [account, setAccount] = useState(props.accounts[0] ?? "");
  const [model, setModel] = useState("");
  const [protocols, setProtocols] = useState<string[]>(["chat"]);
  const [context, setContext] = useState("");
  const toggle = (p: string) =>
    setProtocols((cur) => (cur.includes(p) ? cur.filter((x) => x !== p) : [...cur, p]));
  return (
    <Modal
      title={t("models.addTitle")}
      onClose={props.onClose}
      footer={
        <>
          <Button onClick={props.onClose}>{t("common.cancel")}</Button>
          <Button
            kind="primary"
            disabled={action.busy || !account || !model.trim()}
            onClick={() =>
              void action.run(async () => {
                const [provider = "", alias = ""] = account.split("/");
                await api.addModel({
                  provider,
                  account: alias,
                  model: model.trim(),
                  protocols,
                  context_window: context.trim() ? Number(context) : null,
                });
                props.onDone();
                props.onClose();
              })
            }
          >
            {t("common.add")}
          </Button>
        </>
      }
    >
      <Field label={t("models.account")}>
        <select value={account} onChange={(e) => setAccount(e.target.value)}>
          {props.accounts.map((a) => (
            <option key={a}>{a}</option>
          ))}
        </select>
      </Field>
      <Field label={t("models.upstreamId")}>
        <input value={model} onChange={(e) => setModel(e.target.value)} placeholder="model-id" />
      </Field>
      <Field label={t("models.serves")}>
        <div className="row" style={{ gap: 16 }}>
          {["chat", "responses", "messages"].map((p) => (
            <label key={p} className="check-row">
              <input type="checkbox" checked={protocols.includes(p)} onChange={() => toggle(p)} />
              {p}
            </label>
          ))}
        </div>
      </Field>
      <Field label={t("models.contextOptional")}>
        <input value={context} onChange={(e) => setContext(e.target.value)} inputMode="numeric" />
      </Field>
      <ErrorText error={action.error} />
    </Modal>
  );
}
