// SPDX-License-Identifier: Apache-2.0

import { useState } from "react";
import { api, type ClientCreated } from "../api";
import { Badge, Button, Card, Empty, ErrorText, Field, Modal, Page, copy } from "../components";
import { time } from "../format";
import { useAction, usePoll } from "../hooks";

export function Clients(props: { gateway: string }) {
  const clients = usePoll(api.clients, 10000);
  const action = useAction();
  const [creating, setCreating] = useState(false);
  const [created, setCreated] = useState<ClientCreated>();
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
                  <td className="num">
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
