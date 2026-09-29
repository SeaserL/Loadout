// SPDX-License-Identifier: Apache-2.0

import { useState } from "react";
import { api, type ClientCreated, type ClientInfo, type ClientPolicy } from "../api";
import { Badge, Button, Card, Empty, ErrorText, Field, Modal, Page, copy } from "../components";
import { num, time, usd } from "../format";

function describe(p: ClientPolicy): string {
  const parts: string[] = [];
  if (p.default_model) parts.push(`default ${p.default_model}`);
  if (p.allowed_models?.length) parts.push(`only ${p.allowed_models.join(", ")}`);
  if (p.daily_token_limit != null) parts.push(`${num(p.daily_token_limit)} tokens/day`);
  if (p.daily_cost_limit_usd != null) parts.push(`${usd(p.daily_cost_limit_usd)}/day`);
  if (p.requests_per_minute != null) parts.push(`${p.requests_per_minute}/min`);
  return parts.length ? parts.join(" · ") : "no restrictions";
}
import { useAction, usePoll } from "../hooks";

export function Clients(props: { gateway: string }) {
  const clients = usePoll(api.clients, 10000);
  const action = useAction();
  const [creating, setCreating] = useState(false);
  const [created, setCreated] = useState<ClientCreated>();
  const [policyOf, setPolicyOf] = useState<ClientInfo>();
  const base = `http://${props.gateway}/v1`;

  return (
    <Page
      title="Client keys"
      actions={
        <Button kind="primary" onClick={() => setCreating(true)}>
          New client key
        </Button>
      }
    >
      <div className="notice small">
        Every app that calls Loadout gets its own key. Point the app at <span className="mono">{base}</span> and
        use the key as its OpenAI or Anthropic API key.
      </div>
      <ErrorText error={action.error ?? clients.error?.message} />
      <Card>
        {(clients.data ?? []).length === 0 && !clients.loading && <Empty>No client keys yet.</Empty>}
        {(clients.data ?? []).length > 0 && (
          <table>
            <thead>
              <tr>
                <th>Name</th>
                <th>Key</th>
                <th>Created</th>
                <th>Status</th>
                <th>Today</th>
                <th>Policy</th>
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
                    {c.revoked_at_ms ? <Badge tone="bad">revoked {time(c.revoked_at_ms)}</Badge> : <Badge tone="ok">active</Badge>}
                  </td>
                  <td className="small">
                    {num(c.tokens_today)} tok · {usd(c.cost_today_usd)}
                  </td>
                  <td className="small">{describe(c.policy)}</td>
                  <td className="num">
                    {!c.revoked_at_ms && (
                      <Button disabled={action.busy} onClick={() => setPolicyOf(c)}>
                        Policy
                      </Button>
                    )}{" "}
                    {!c.revoked_at_ms && (
                      <Button
                        kind="danger"
                        disabled={action.busy}
                        onClick={() => {
                          if (confirm(`Revoke the key of ${c.name}? Apps using it stop working immediately.`)) {
                            void action.run(async () => {
                              await api.revokeClient(c.name);
                              await clients.reload();
                            });
                          }
                        }}
                      >
                        Revoke
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
    <Modal title="New client key" onClose={props.onClose}>
      <Field label="Name" hint="Which app uses this key, e.g. opencode, cursor, my-script.">
        <input value={name} onChange={(e) => setName(e.target.value)} autoFocus />
      </Field>
      <ErrorText error={action.error} />
      <div className="row">
        <div className="spacer" />
        <Button onClick={props.onClose}>Cancel</Button>
        <Button
          kind="primary"
          disabled={action.busy || !name.trim()}
          onClick={() =>
            void action.run(async () => {
              props.onCreated(await api.createClient(name.trim()));
            })
          }
        >
          Create
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
    <Modal title={`Key for ${props.created.client.name}`} onClose={props.onClose} wide>
      <p>
        <strong>Copy it now — it is shown only once.</strong> Loadout stores only a hash.
      </p>
      <div className="key-box">{key}</div>
      <div className="row" style={{ margin: "10px 0 16px" }}>
        <Button kind="primary" onClick={() => copy(key)}>
          Copy key
        </Button>
      </div>
      <div className="small muted">OpenAI-compatible apps</div>
      <pre className="snippet">{openai}</pre>
      <div className="small muted">Anthropic-compatible apps</div>
      <pre className="snippet">{anthropic}</pre>
      <div className="small muted">
        Model names: <span className="mono">loadout/default</span>, <span className="mono">loadout/fast</span>,{" "}
        <span className="mono">loadout/realtime</span>, a pool, or an exact <span className="mono">provider/account/model</span>{" "}
        from the Models page.
      </div>
      <div className="row" style={{ marginTop: 12 }}>
        <div className="spacer" />
        <Button onClick={props.onClose}>Done</Button>
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
      throw new Error("Limits must be positive numbers (leave empty for no limit).");
    }
    return n;
  };
  return (
    <Modal title={`Policy of ${props.client.name}`} onClose={props.onClose}>
      <Field
        label="Default model"
        hint="Used when the app sends a model name Loadout does not know (for apps that cannot change it). E.g. loadout/default."
      >
        <input value={defaultModel} onChange={(e) => setDefaultModel(e.target.value)} placeholder="loadout/default" />
      </Field>
      <Field label="Allowed models" hint="One per line; end with * for a prefix, e.g. loadout/* . Empty allows all.">
        <textarea rows={3} value={allowed} onChange={(e) => setAllowed(e.target.value)} />
      </Field>
      <div className="grid cols-3" style={{ gap: 10 }}>
        <Field label="Tokens per day">
          <input value={tokens} onChange={(e) => setTokens(e.target.value)} inputMode="numeric" />
        </Field>
        <Field label="Cost per day (USD)">
          <input value={cost} onChange={(e) => setCost(e.target.value)} inputMode="decimal" />
        </Field>
        <Field label="Requests per minute">
          <input value={rpm} onChange={(e) => setRpm(e.target.value)} inputMode="numeric" />
        </Field>
      </div>
      <p className="small muted">Daily limits reset at 00:00 UTC. Requests over a limit get HTTP 429.</p>
      <ErrorText error={action.error} />
      <div className="row">
        <div className="spacer" />
        <Button onClick={props.onClose}>Cancel</Button>
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
          Save
        </Button>
      </div>
    </Modal>
  );
}
