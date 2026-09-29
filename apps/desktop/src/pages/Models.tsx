// SPDX-License-Identifier: Apache-2.0

import { useMemo, useState } from "react";
import { api, type ModelInfo } from "../api";
import { Badge, Button, Card, Empty, ErrorText, Field, Modal, Page, copy, tierTone } from "../components";
import { num, tierLabel } from "../format";
import { useAction, usePoll } from "../hooks";

const TIERS = [
  { value: "auto", label: "Measured" },
  { value: "ultra-fast", label: "Ultra fast (pinned)" },
  { value: "fast", label: "Fast (pinned)" },
  { value: "normal", label: "Normal (pinned)" },
];

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
  const [filter, setFilter] = useState("");
  const [showDisabled, setShowDisabled] = useState(false);
  const [price, setPrice] = useState<ModelInfo>();
  const [adding, setAdding] = useState(false);

  const tierOf = useMemo(() => {
    const m = new Map<string, string>();
    for (const r of routes.data ?? []) m.set(r.name, r.tier);
    return m;
  }, [routes.data]);

  const list = (models.data ?? []).filter(
    (m) => (showDisabled || m.enabled) && m.name.toLowerCase().includes(filter.toLowerCase()),
  );
  const run = (f: () => Promise<unknown>) =>
    void action.run(async () => {
      await f();
      await Promise.all([models.reload(), routes.reload()]);
    });

  return (
    <Page
      title="Models"
      actions={
        <>
          <input placeholder="Filter" value={filter} onChange={(e) => setFilter(e.target.value)} />
          <label className="row small">
            <input type="checkbox" checked={showDisabled} onChange={(e) => setShowDisabled(e.target.checked)} />
            show disabled
          </label>
          <Button onClick={() => setAdding(true)}>Add model manually</Button>
        </>
      }
    >
      <ErrorText error={action.error ?? models.error?.message} />
      <Card>
        {list.length === 0 && !models.loading && (
          <Empty>No models. Sync an account's models under Accounts, or add one manually.</Empty>
        )}
        {list.length > 0 && (
          <table>
            <thead>
              <tr>
                <th>Name (use this as "model")</th>
                <th>Protocols</th>
                <th className="num">Context</th>
                <th className="num">Price in / out ($/M)</th>
                <th>Tier</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {list.map((m) => {
                const tier = tierOf.get(m.name) ?? "UNMEASURED";
                return (
                  <tr key={m.name} style={{ opacity: m.enabled ? 1 : 0.55 }}>
                    <td>
                      <button className="btn link mono" title="Copy" onClick={() => copy(m.name)}>
                        {m.name}
                      </button>
                      {m.source === "manual" && <span className="small muted"> manual</span>}
                    </td>
                    <td className="small">{m.protocols.join(", ")}</td>
                    <td className="num">{num(m.context_window)}</td>
                    <td className="num">
                      <button className="btn link" onClick={() => setPrice(m)}>
                        {m.input_price ?? "-"} / {m.output_price ?? "-"}
                      </button>
                    </td>
                    <td>
                      <div className="row">
                        <Badge tone={tierTone(tier)}>{tierLabel(tier)}</Badge>
                        <select
                          value={tierValue(m)}
                          disabled={action.busy || !m.enabled}
                          onChange={(e) =>
                            run(() => api.setModelTier(m.name, e.target.value === "auto" ? null : e.target.value))
                          }
                        >
                          {TIERS.map((t) => (
                            <option key={t.value} value={t.value}>
                              {t.label}
                            </option>
                          ))}
                        </select>
                      </div>
                    </td>
                    <td className="num">
                      <div className="row" style={{ justifyContent: "flex-end" }}>
                        <Button disabled={action.busy} onClick={() => run(() => api.setModelEnabled(m.name, !m.enabled))}>
                          {m.enabled ? "Disable" : "Enable"}
                        </Button>
                        {m.source === "manual" && (
                          <Button kind="danger" disabled={action.busy} onClick={() => run(() => api.removeModel(m.name))}>
                            Remove
                          </Button>
                        )}
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </Card>
      {price && <PriceDialog model={price} onClose={() => setPrice(undefined)} onDone={() => void models.reload()} />}
      {adding && (
        <AddModelDialog
          accounts={(accounts.data ?? []).filter((a) => a.account_type !== "subscription").map((a) => `${a.provider}/${a.alias}`)}
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
    if (!Number.isFinite(n) || n < 0) throw new Error("Prices must be non-negative numbers.");
    return n;
  };
  return (
    <Modal title={`Prices of ${props.model.name}`} onClose={props.onClose}>
      <p className="small muted">USD per million tokens, used for cost statistics.</p>
      <Field label="Input">
        <input value={input} onChange={(e) => setInput(e.target.value)} inputMode="decimal" />
      </Field>
      <Field label="Output">
        <input value={output} onChange={(e) => setOutput(e.target.value)} inputMode="decimal" />
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
              await api.setModelPrice(props.model.name, parse(input), parse(output));
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

function AddModelDialog(props: { accounts: string[]; onClose: () => void; onDone: () => void }) {
  const action = useAction();
  const [account, setAccount] = useState(props.accounts[0] ?? "");
  const [model, setModel] = useState("");
  const [protocols, setProtocols] = useState<string[]>(["chat"]);
  const [context, setContext] = useState("");
  const toggle = (p: string) =>
    setProtocols((cur) => (cur.includes(p) ? cur.filter((x) => x !== p) : [...cur, p]));
  return (
    <Modal title="Add a model manually" onClose={props.onClose}>
      <Field label="Account">
        <select value={account} onChange={(e) => setAccount(e.target.value)}>
          {props.accounts.map((a) => (
            <option key={a}>{a}</option>
          ))}
        </select>
      </Field>
      <Field label="Upstream model id">
        <input value={model} onChange={(e) => setModel(e.target.value)} placeholder="model-id" />
      </Field>
      <Field label="Protocols the upstream serves it with">
        <div className="row">
          {["chat", "responses", "messages"].map((p) => (
            <label key={p} className="row small">
              <input type="checkbox" checked={protocols.includes(p)} onChange={() => toggle(p)} />
              {p}
            </label>
          ))}
        </div>
      </Field>
      <Field label="Context window (tokens, optional)">
        <input value={context} onChange={(e) => setContext(e.target.value)} inputMode="numeric" />
      </Field>
      <ErrorText error={action.error} />
      <div className="row">
        <div className="spacer" />
        <Button onClick={props.onClose}>Cancel</Button>
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
          Add
        </Button>
      </div>
    </Modal>
  );
}
