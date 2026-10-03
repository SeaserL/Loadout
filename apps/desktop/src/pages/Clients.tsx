// SPDX-License-Identifier: Apache-2.0

import { useState } from "react";
import { Ban, Plus, SlidersHorizontal } from "lucide-react";
import { api, type ClientCreated, type ClientInfo, type ClientPolicy } from "../api";
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
  useConfirm,
} from "../components";
import { num, time, usd } from "../format";
import { useAction, usePoll } from "../hooks";
import { t } from "../i18n";

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
  const confirm = useConfirm();
  const [creating, setCreating] = useState(false);
  const [created, setCreated] = useState<ClientCreated>();
  const [policyOf, setPolicyOf] = useState<ClientInfo>();
  const base = `http://${props.gateway}/v1`;

  return (
    <Page
      title={t("clients.title")}
      actions={
        <Button kind="primary" icon={<Plus size={16} />} onClick={() => setCreating(true)}>
          {t("clients.new")}
        </Button>
      }
    >
      <Card title={t("clients.connect")} subtitle={t("clients.intro2")}>
        <div className="grid cols-2" style={{ gap: 12 }}>
          <Field label={t("clients.baseOpenai")}>
            <CopyField value={base} />
          </Field>
          <Field label={t("clients.baseAnthropic")}>
            <CopyField value={base.replace(/\/v1$/, "")} />
          </Field>
        </div>
      </Card>
      <ErrorText error={action.error ?? clients.error?.message} />
      <Card title={t("clients.keys")}>
        {(clients.data ?? []).length === 0 && !clients.loading && (
          <Empty
            action={
              <Button kind="primary" icon={<Plus size={16} />} onClick={() => setCreating(true)}>
                {t("clients.new")}
              </Button>
            }
          >
            {t("clients.empty")}
          </Empty>
        )}
        {(clients.data ?? []).length > 0 && (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>{t("common.name")}</th>
                  <th>{t("clients.key")}</th>
                  <th>{t("clients.status")}</th>
                  <th>{t("clients.today")}</th>
                  <th>{t("clients.policy")}</th>
                  <th>{t("clients.created")}</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {(clients.data ?? []).map((c) => (
                  <tr key={c.id} className={c.revoked_at_ms ? "dim" : ""}>
                    <td className="strong">{c.name}</td>
                    <td className="mono">{c.key_prefix}…</td>
                    <td>
                      {c.revoked_at_ms ? (
                        <Badge tone="bad" title={time(c.revoked_at_ms)}>
                          {t("clients.revokedShort")}
                        </Badge>
                      ) : (
                        <Badge tone="ok" dot>
                          {t("clients.active")}
                        </Badge>
                      )}
                    </td>
                    <td className="small" style={{ whiteSpace: "nowrap" }}>
                      {t("clients.usageToday", { tokens: num(c.tokens_today), cost: usd(c.cost_today_usd) })}
                    </td>
                    <td className="small">{describe(c.policy)}</td>
                    <td className="small muted" style={{ whiteSpace: "nowrap" }}>
                      {time(c.created_at_ms)}
                    </td>
                    <td className="actions-cell">
                      {!c.revoked_at_ms && (
                        <div className="row">
                          <IconButton label={t("clients.policy")} disabled={action.busy} onClick={() => setPolicyOf(c)}>
                            <SlidersHorizontal size={16} />
                          </IconButton>
                          <IconButton
                            label={t("clients.revoke")}
                            danger
                            disabled={action.busy}
                            onClick={() =>
                              void confirm({
                                title: t("clients.revokeTitle", { name: c.name }),
                                message: t("clients.confirmRevoke", { name: c.name }),
                                confirmLabel: t("clients.revoke"),
                                danger: true,
                              }).then(
                                (ok) =>
                                  ok &&
                                  void action.run(async () => {
                                    await api.revokeClient(c.name);
                                    await clients.reload();
                                  }),
                              )
                            }
                          >
                            <Ban size={16} />
                          </IconButton>
                        </div>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
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
  const submit = () =>
    void action.run(async () => {
      props.onCreated(await api.createClient(name.trim()));
    });
  return (
    <Modal
      title={t("clients.new")}
      onClose={props.onClose}
      footer={
        <>
          <Button onClick={props.onClose}>{t("common.cancel")}</Button>
          <Button kind="primary" disabled={action.busy || !name.trim()} onClick={submit}>
            {t("common.create")}
          </Button>
        </>
      }
    >
      <Field label={t("common.name")} hint={t("clients.nameHint")}>
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && name.trim() && !action.busy) submit();
          }}
        />
      </Field>
      <ErrorText error={action.error} />
    </Modal>
  );
}

function KeyShown(props: { created: ClientCreated; base: string; onClose: () => void }) {
  const key = props.created.key;
  const openai = `OPENAI_BASE_URL=${props.base}\nOPENAI_API_KEY=${key}`;
  const anthropic = `ANTHROPIC_BASE_URL=${props.base.replace(/\/v1$/, "")}\nANTHROPIC_API_KEY=${key}`;
  return (
    <Modal
      title={t("clients.keyFor", { name: props.created.client.name })}
      onClose={props.onClose}
      wide
      footer={
        <Button kind="primary" onClick={props.onClose}>
          {t("common.done")}
        </Button>
      }
    >
      <Notice tone="warn">
        <strong>{t("clients.copyNow")}</strong> {t("clients.hashOnly")}
      </Notice>
      <CopyField value={key} label={t("clients.copyKey")} secret />
      <div style={{ height: 16 }} />
      <div className="small muted">{t("clients.openaiApps")}</div>
      <pre className="snippet">{openai}</pre>
      <div className="small muted">{t("clients.anthropicApps")}</div>
      <pre className="snippet">{anthropic}</pre>
      <div className="small muted">{t("clients.modelNames")}</div>
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
    <Modal
      title={t("clients.policyTitle", { name: props.client.name })}
      onClose={props.onClose}
      footer={
        <>
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
        </>
      }
    >
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
      <p className="small muted" style={{ margin: 0 }}>
        {t("clients.limitsNote")}
      </p>
      <ErrorText error={action.error} />
    </Modal>
  );
}
