// SPDX-License-Identifier: Apache-2.0

import { useState } from "react";
import { api, type ClientCreated, type ClientInfo, type ClientPolicy } from "../api";
import { Badge, Button, Card, Empty, ErrorText, Field, Modal, Page, copy } from "../components";
import { num, time, usd } from "../format";
import { useAction, usePoll } from "../hooks";
import { t, tr } from "../i18n";

function describe(p: ClientPolicy): string {
  const parts: string[] = [];
  if (p.default_model) parts.push(t("clients.pDefault", { model: p.default_model }));
  if (p.allowed_models?.length) parts.push(t("clients.pOnly", { models: p.allowed_models.join(", ") }));
  if (p.daily_token_limit != null) parts.push(t("clients.pTokens", { n: num(p.daily_token_limit) }));
  if (p.daily_cost_limit_usd != null) parts.push(t("clients.pCost", { cost: usd(p.daily_cost_limit_usd) }));
  if (p.requests_per_minute != null) parts.push(t("clients.pRpm", { n: p.requests_per_minute }));
  return parts.length ? parts.join(" · ") : t("clients.noRestrictions");
}

export function Clients(props: { gateway: string }) {
  const clients = usePoll(api.clients, 10000);
  const action = useAction();
  const [creating, setCreating] = useState(false);
  const [created, setCreated] = useState<ClientCreated>();
  const [policyOf, setPolicyOf] = useState<ClientInfo>();
  const base = `http://${props.gateway}/v1`;

  return (
    <Page
      title={t("clients.title")}
      actions={
        <Button kind="primary" onClick={() => setCreating(true)}>
          {t("clients.new")}
        </Button>
      }
    >
      <div className="notice small">{tr("clients.intro", { base: <span className="mono">{base}</span> })}</div>
      <ErrorText error={action.error ?? clients.error?.message} />
      <Card>
        {(clients.data ?? []).length === 0 && !clients.loading && <Empty>{t("clients.empty")}</Empty>}
        {(clients.data ?? []).length > 0 && (
          <table>
            <thead>
              <tr>
                <th>{t("common.name")}</th>
                <th>{t("clients.key")}</th>
                <th>{t("clients.created")}</th>
                <th>{t("clients.status")}</th>
                <th>{t("clients.today")}</th>
                <th>{t("clients.policy")}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {(clients.data ?? []).map((c) => (
                <tr key={c.id}>
                  <td>{c.name}</td>
                  <td className="mono">{c.key_prefix}…</td>
                  <td className="small">{time(c.created_at_ms)}</td>
                  <td>
                    {c.revoked_at_ms ? (
                      <Badge tone="bad">{t("clients.revoked", { time: time(c.revoked_at_ms) })}</Badge>
                    ) : (
                      <Badge tone="ok">{t("clients.active")}</Badge>
                    )}
                  </td>
                  <td className="small">
                    {t("clients.usageToday", { tokens: num(c.tokens_today), cost: usd(c.cost_today_usd) })}
                  </td>
                  <td className="small">{describe(c.policy)}</td>
                  <td className="num">
                    {!c.revoked_at_ms && (
                      <Button disabled={action.busy} onClick={() => setPolicyOf(c)}>
                        {t("clients.policy")}
                      </Button>
                    )}{" "}
                    {!c.revoked_at_ms && (
                      <Button
                        kind="danger"
                        disabled={action.busy}
                        onClick={() => {
                          if (confirm(t("clients.confirmRevoke", { name: c.name }))) {
                            void action.run(async () => {
                              await api.revokeClient(c.name);
                              await clients.reload();
                            });
                          }
                        }}
                      >
                        {t("clients.revoke")}
                      </Button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>
      {creating && (
        <CreateDialog
          onClose={() => setCreating(false)}
          onCreated={(c) => {
            setCreating(false);
            setCreated(c);
            void clients.reload();
          }}
        />
      )}
      {created && <KeyShown created={created} base={base} onClose={() => setCreated(undefined)} />}
      {policyOf && (
        <PolicyDialog client={policyOf} onClose={() => setPolicyOf(undefined)} onDone={() => void clients.reload()} />
      )}
    </Page>
  );
}

function CreateDialog(props: { onClose: () => void; onCreated: (c: ClientCreated) => void }) {
  const action = useAction();
  const [name, setName] = useState("");
  return (
    <Modal title={t("clients.new")} onClose={props.onClose}>
      <Field label={t("common.name")} hint={t("clients.nameHint")}>
        <input value={name} onChange={(e) => setName(e.target.value)} autoFocus />
      </Field>
      <ErrorText error={action.error} />
      <div className="row">
        <div className="spacer" />
        <Button onClick={props.onClose}>{t("common.cancel")}</Button>
        <Button
          kind="primary"
          disabled={action.busy || !name.trim()}
          onClick={() =>
            void action.run(async () => {
              props.onCreated(await api.createClient(name.trim()));
            })
          }
        >
          {t("common.create")}
        </Button>
      </div>
    </Modal>
  );
}

function KeyShown(props: { created: ClientCreated; base: string; onClose: () => void }) {
  const key = props.created.key;
  const openai = `OPENAI_BASE_URL=${props.base}\nOPENAI_API_KEY=${key}`;
  const anthropic = `ANTHROPIC_BASE_URL=${props.base.replace(/\/v1$/, "")}\nANTHROPIC_API_KEY=${key}`;
  return (
    <Modal title={t("clients.keyFor", { name: props.created.client.name })} onClose={props.onClose} wide>
      <p>
        <strong>{t("clients.copyNow")}</strong> {t("clients.hashOnly")}
      </p>
      <div className="key-box">{key}</div>
      <div className="row" style={{ margin: "10px 0 16px" }}>
        <Button kind="primary" onClick={() => copy(key)}>
          {t("clients.copyKey")}
        </Button>
      </div>
      <div className="small muted">{t("clients.openaiApps")}</div>
      <pre className="snippet">{openai}</pre>
      <div className="small muted">{t("clients.anthropicApps")}</div>
      <pre className="snippet">{anthropic}</pre>
      <div className="small muted">{t("clients.modelNames")}</div>
      <div className="row" style={{ marginTop: 12 }}>
        <div className="spacer" />
        <Button onClick={props.onClose}>{t("common.done")}</Button>
      </div>
    </Modal>
  );
}

function PolicyDialog(props: { client: ClientInfo; onClose: () => void; onDone: () => void }) {
  const p = props.client.policy;
  const action = useAction();
  const [defaultModel, setDefaultModel] = useState(p.default_model ?? "");
  const [allowed, setAllowed] = useState((p.allowed_models ?? []).join("\n"));
  const [tokens, setTokens] = useState(p.daily_token_limit?.toString() ?? "");
  const [cost, setCost] = useState(p.daily_cost_limit_usd?.toString() ?? "");
  const [rpm, setRpm] = useState(p.requests_per_minute?.toString() ?? "");
  const number = (v: string, integer: boolean) => {
    if (!v.trim()) return null;
    const n = Number(v);
    if (!Number.isFinite(n) || n <= 0 || (integer && !Number.isInteger(n))) {
      throw new Error(t("clients.errLimits"));
    }
    return n;
  };
  return (
    <Modal title={t("clients.policyTitle", { name: props.client.name })} onClose={props.onClose}>
      <Field label={t("clients.defaultModel")} hint={t("clients.defaultModelHint")}>
        <input value={defaultModel} onChange={(e) => setDefaultModel(e.target.value)} placeholder="loadout/default" />
      </Field>
      <Field label={t("clients.allowed")} hint={t("clients.allowedHint")}>
        <textarea rows={3} value={allowed} onChange={(e) => setAllowed(e.target.value)} />
      </Field>
      <div className="grid cols-3" style={{ gap: 10 }}>
        <Field label={t("clients.tokensPerDay")}>
          <input value={tokens} onChange={(e) => setTokens(e.target.value)} inputMode="numeric" />
        </Field>
        <Field label={t("clients.costPerDay")}>
          <input value={cost} onChange={(e) => setCost(e.target.value)} inputMode="decimal" />
        </Field>
        <Field label={t("clients.rpm")}>
          <input value={rpm} onChange={(e) => setRpm(e.target.value)} inputMode="numeric" />
        </Field>
      </div>
      <p className="small muted">{t("clients.limitsNote")}</p>
      <ErrorText error={action.error} />
      <div className="row">
        <div className="spacer" />
        <Button onClick={props.onClose}>{t("common.cancel")}</Button>
        <Button
          kind="primary"
          disabled={action.busy}
          onClick={() =>
            void action.run(async () => {
              await api.setClientPolicy(props.client.name, {
                default_model: defaultModel.trim() || null,
                allowed_models: allowed
                  .split(/[\n,]/)
                  .map((s) => s.trim())
                  .filter(Boolean),
                daily_token_limit: number(tokens, true),
                daily_cost_limit_usd: number(cost, false),
                requests_per_minute: number(rpm, true),
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
