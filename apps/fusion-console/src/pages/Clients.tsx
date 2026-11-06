// SPDX-License-Identifier: Apache-2.0

import { useEffect, useMemo, useRef, useState } from "react";
import { Ban, Plus, SlidersHorizontal, Wand2, X } from "lucide-react";
import { api, native, type ClientCreated, type ClientInfo, type ClientPolicy, type ModelInfo, type ToolConnected, type VirtualInfo } from "../api";
import {
  Badge,
  Button,
  CopyField,
  Empty,
  ErrorText,
  Field,
  IconButton,
  Modal,
  Notice,
  Page,
  Segmented,
  Switch,
  useConfirm,
} from "../components";
import { num, time, usd } from "../format";
import { useAction, usePoll } from "../hooks";
import { type MessageKey, t } from "../i18n";

function some(names: string[], full: boolean): string {
  if (full || names.length <= 3) return names.join(", ");
  return t("clients.someOf", { first: names.slice(0, 2).join(", "), n: names.length });
}

function describe(p: ClientPolicy, full = false): string {
  const parts: string[] = [];
  if (p.allowed_models?.length) parts.push(t("clients.pOnly", { models: some(p.allowed_models, full) }));
  if (p.allowed_accounts?.length) parts.push(t("clients.pAccounts", { accounts: some(p.allowed_accounts, full) }));
  if (p.daily_cost_limit_usd != null) parts.push(t("clients.pCost", { cost: usd(p.daily_cost_limit_usd) }));
  if (p.weekly_cost_limit_usd != null) parts.push(t("clients.pWeekCost", { cost: usd(p.weekly_cost_limit_usd) }));
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
  const [connecting, setConnecting] = useState(false);
  const [connected, setConnected] = useState<ToolConnected>();
  // Keys made on one PC share a machine prefix; the column only helps when it tells them apart.
  const showPrefix = new Set((clients.data ?? []).map((c) => c.key_prefix)).size > 1;
  const base = `http://${props.gateway}/v1`;

  return (
    <Page
      title={t("clients.title")}
      help="clients"
      actions={
        <>
          <Button icon={<Wand2 size={16} />} onClick={() => setConnecting(true)}>
            {t("clients.connectTool")}
          </Button>
          <Button kind="primary" icon={<Plus size={16} />} onClick={() => setCreating(true)}>
            {t("clients.new")}
          </Button>
        </>
      }
    >
      <div className="connect-strip top">
        <div className="connect-item">
          <span className="connect-label">{t("dash.apiAddress")}</span>
          <CopyField value={base} />
        </div>
      </div>
      <ErrorText error={action.error ?? clients.error?.message} />
      <section>
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
                  {showPrefix && <th>{t("clients.key")}</th>}
                  <th>{t("clients.status")}</th>
                  <th>{t("clients.today")}</th>
                  <th>{t("clients.policy")}</th>
                  <th>MCP</th>
                  <th>{t("clients.created")}</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {(clients.data ?? []).map((c) => (
                  <tr key={c.id} className={c.revoked_at_ms ? "dim" : ""}>
                    <td className="strong">{c.name}</td>
                    {showPrefix && <td className="mono">{c.key_prefix}…</td>}
                    <td>
                      {c.revoked_at_ms ? (
                        <Badge tone="bad" title={time(c.revoked_at_ms)} dot>
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
                    <td className="small" title={describe(c.policy, true)}>
                      {describe(c.policy)}
                    </td>
                    <td>
                      <Switch
                        checked={c.mcp}
                        disabled={action.busy || !!c.revoked_at_ms}
                        label={t("clients.mcpFor", { name: c.name })}
                        onChange={(on) =>
                          void action.run(async () => {
                            await api.setClientMcp(c.name, on);
                            await clients.reload();
                          })
                        }
                      />
                    </td>
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
      </section>
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
      {connecting && (
        <ConnectDialog
          onClose={() => setConnecting(false)}
          onDone={(result) => {
            setConnecting(false);
            setConnected(result);
            void clients.reload();
          }}
        />
      )}
      {connected && <Connected result={connected} onClose={() => setConnected(undefined)} />}
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
      <Field label={t("common.name")}>
        <input
          value={name}
          placeholder="opencode"
          autoFocus
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

/** Makes a key for one tool and writes it into that tool's own config (backed up first). */
function ConnectDialog(props: { onClose: () => void; onDone: (result: ToolConnected) => void }) {
  const action = useAction();
  const tools = usePoll(api.clientTools, 0);
  const models = usePoll(api.models, 0);
  const virtuals = usePoll(api.virtuals, 0);
  const options = useMemo(
    () => (models.data && virtuals.data ? modelOptions(models.data, virtuals.data) : []),
    [models.data, virtuals.data],
  );
  const [tool, setTool] = useState<string>();
  const [model, setModel] = useState("");
  const [mcp, setMcp] = useState(false);
  const chosen = tool ?? tools.data?.find((x) => x.installed)?.id ?? tools.data?.[0]?.id;
  const target = tools.data?.find((x) => x.id === chosen);
  return (
    <Modal
      title={t("clients.connectTool")}
      onClose={props.onClose}
      footer={
        <>
          <Button onClick={props.onClose}>{t("common.cancel")}</Button>
          <Button
            kind="primary"
            disabled={action.busy || !target?.installed || !model}
            onClick={() => void action.run(async () => props.onDone(await api.connectClientTool(target!.id, model, mcp)))}
          >
            {t("clients.connectWrite")}
          </Button>
        </>
      }
    >
      <Field label={t("clients.connectWhich")}>
        <Segmented
          label={t("clients.connectWhich")}
          value={chosen ?? ""}
          options={(tools.data ?? []).map((x) => ({
            value: x.id,
            label: x.name,
            ...(x.installed ? {} : { title: t("clients.toolMissing") }),
          }))}
          onChange={(id) => setTool(id)}
        />
      </Field>
      {target && <div className="small muted mono">{target.config_path}</div>}
      {target && !target.installed && <Notice tone="warn">{t("clients.toolMissing")}</Notice>}
      <Field label={t("clients.connectModel")}>
        <select value={model} onChange={(e) => setModel(e.target.value)}>
          <option value="">{t("features.pickModel")}</option>
          {options.map((o) => (
            <option key={o.name} value={o.name}>
              {o.name}
            </option>
          ))}
        </select>
      </Field>
      <Field label="MCP">
        <Switch checked={mcp} label={t("clients.connectMcp")} onChange={setMcp} />
      </Field>
      <ErrorText error={action.error ?? tools.error?.message} />
    </Modal>
  );
}

function Connected(props: { result: ToolConnected; onClose: () => void }) {
  const r = props.result;
  return (
    <Modal
      title={t("clients.connectedTitle", { name: r.client.name })}
      onClose={props.onClose}
      footer={
        <Button kind="primary" onClick={props.onClose}>
          {t("common.done")}
        </Button>
      }
    >
      <dl className="kv left">
        <dt>{t("clients.connectedFile")}</dt>
        <dd className="mono small">{r.written}</dd>
        {r.backup && (
          <>
            <dt>{t("clients.connectedBackup")}</dt>
            <dd className="mono small">{r.backup}</dd>
          </>
        )}
        {r.usage && (
          <>
            <dt>{t("clients.connectedUsage")}</dt>
            <dd className="mono">{r.usage}</dd>
          </>
        )}
      </dl>
    </Modal>
  );
}

function KeyShown(props: { created: ClientCreated; base: string; onClose: () => void }) {
  const key = props.created.key;
  const openai = `OPENAI_BASE_URL=${props.base}\nOPENAI_API_KEY=${key}`;
  const anthropic = `ANTHROPIC_BASE_URL=${props.base}\nANTHROPIC_API_KEY=${key}`;
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
      <Notice tone="warn">{t("clients.copyNow")}</Notice>
      <CopyField value={key} label={t("clients.copyKey")} secret />
      <div style={{ height: 16 }} />
      <div className="small muted">{t("clients.openaiApps")}</div>
      <pre className="snippet">{openai}</pre>
      <div className="small muted">{t("clients.anthropicApps")}</div>
      <pre className="snippet">{anthropic}</pre>
    </Modal>
  );
}

type ModelOption = { name: string; kind: "group" | "virtual" | "any" | "exact" };

/** Everything a client can ask for, the broadest names first. */
function modelOptions(models: ModelInfo[], virtuals: VirtualInfo[]): ModelOption[] {
  const out = new Map<string, ModelOption>();
  for (const v of virtuals) {
    if (v.enabled !== false) out.set(v.name, { name: v.name, kind: v.kind === "pool" ? "group" : "virtual" });
  }
  const on = models.filter((x) => x.enabled);
  for (const m of on) {
    // Only offered where it means something: several accounts of this provider have the model.
    // Upstream ids with a slash cannot be told apart from provider/account/model in the short form.
    const any = `${m.provider}/${m.upstream_model}`;
    const shared = on.filter((x) => `${x.provider}/${x.upstream_model}` === any).length > 1;
    if (shared && !m.upstream_model.includes("/")) out.set(any, { name: any, kind: "any" });
  }
  for (const m of on) out.set(m.name, { name: m.name, kind: "exact" });
  return [...out.values()];
}

/** Every name the gateway answers to, listed or not. */
function knownNames(models: ModelInfo[], virtuals: VirtualInfo[]): Set<string> {
  return new Set([
    ...virtuals.map((v) => v.name),
    ...models.flatMap((m) => [m.name, `${m.provider}/${m.upstream_model}`]),
  ]);
}

const KIND_LABEL: Record<ModelOption["kind"], MessageKey | null> = {
  group: "clients.kindGroup",
  virtual: "clients.kindVirtual",
  any: "clients.kindAny",
  exact: null,
};

function Marked(props: { text: string; query: string }) {
  const at = props.query ? props.text.toLowerCase().indexOf(props.query) : -1;
  if (at < 0) return <>{props.text}</>;
  const end = at + props.query.length;
  return (
    <>
      {props.text.slice(0, at)}
      <mark>{props.text.slice(at, end)}</mark>
      {props.text.slice(end)}
    </>
  );
}

/**
 * Picks from the names that exist: typing narrows the list; Tab, Enter or a click adds the
 * highlighted one. A name that no longer exists stays visible, marked, until it is removed.
 */
function ModelPicker(props: {
  label: string;
  value: string[];
  options: ModelOption[] | undefined;
  /** Every name that exists, including switched-off models already on the list. */
  known: Set<string> | undefined;
  onChange: (value: string[]) => void;
  draft: string;
  onDraft: (text: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [index, setIndex] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const q = props.draft.trim().toLowerCase();
  const matches = useMemo(() => {
    const free = (props.options ?? []).filter((o) => !props.value.includes(o.name));
    if (!q) return free;
    // Groups stay on top: they are what a client usually wants. Within that, whole-name starts,
    // then a word inside the name, then anywhere.
    const rank = (name: string) => {
      const n = name.toLowerCase();
      if (n.startsWith(q)) return 0;
      if (n.includes(`/${q}`) || n.includes(`-${q}`)) return 1;
      return n.includes(q) ? 2 : 3;
    };
    const broad = (o: ModelOption) => (o.kind === "group" || o.kind === "virtual" ? 0 : 1);
    return free
      .map((o, i) => ({ o, i, r: rank(o.name) }))
      .filter((x) => x.r < 3)
      .sort((a, b) => broad(a.o) - broad(b.o) || a.r - b.r || a.i - b.i)
      .map((x) => x.o);
  }, [props.options, props.value, q]);
  const at = Math.min(index, Math.max(0, matches.length - 1));
  useEffect(() => {
    list.current?.querySelector(".model-option.on")?.scrollIntoView({ block: "nearest" });
  }, [at]);
  const add = (name: string) => {
    props.onChange([...props.value, name]);
    props.onDraft("");
    setIndex(0);
    input.current?.focus();
  };
  const showList = open && !!props.options && (matches.length > 0 || q !== "");
  return (
    <div className="field">
      <span className="label">{props.label}</span>
      <div className="model-picker">
        <div className="model-picker-box" onMouseDown={(e) => {
          if (e.target === e.currentTarget) {
            e.preventDefault();
            input.current?.focus();
          }
        }}>
          {props.value.map((name) => {
            const gone = !!props.known && !props.known.has(name);
            return (
              <span key={name} className={`model-chip ${gone ? "gone" : ""}`} title={gone ? t("clients.modelGone") : name}>
                <span className="ellipsis">{name}</span>
                <button type="button" aria-label={t("common.remove")} onClick={() => props.onChange(props.value.filter((x) => x !== name))}>
                  <X size={12} aria-hidden />
                </button>
              </span>
            );
          })}
          <input
            ref={input}
            role="combobox"
            aria-label={props.label}
            aria-expanded={showList}
            aria-controls="model-options"
            aria-autocomplete="list"
            aria-activedescendant={showList && matches[at] ? `model-option-${at}` : undefined}
            autoFocus
            value={props.draft}
            placeholder={props.value.length === 0 ? t("clients.allowedAny") : ""}
            onFocus={() => setOpen(true)}
            onBlur={() => setOpen(false)}
            onChange={(e) => {
              props.onDraft(e.target.value);
              setIndex(0);
              setOpen(true);
            }}
            onKeyDown={(e) => {
              const pick = matches[at];
              if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                e.preventDefault();
                if (matches.length === 0) return;
                setOpen(true);
                setIndex((at + (e.key === "ArrowDown" ? 1 : matches.length - 1)) % matches.length);
              } else if (e.key === "Enter" || (e.key === "Tab" && q !== "")) {
                // Tab with nothing typed still moves on, as it does everywhere else.
                if (e.key === "Enter" || pick) e.preventDefault();
                if (pick && showList) add(pick.name);
              } else if (e.key === "Backspace" && props.draft === "" && props.value.length > 0) {
                props.onChange(props.value.slice(0, -1));
              } else if (e.key === "Escape" && showList) {
                e.stopPropagation();
                setOpen(false);
              }
            }}
          />
        </div>
        {showList && (
          <div
            ref={list}
            className="model-options"
            id="model-options"
            role="listbox"
            aria-label={props.label}
            // Keeps focus in the input, so picking or scrolling does not close the list.
            onMouseDown={(e) => e.preventDefault()}
          >
            {matches.length === 0 && <div className="model-none">{t("clients.noSuchModel", { name: props.draft.trim() })}</div>}
            {matches.map((o, i) => {
              const kind = KIND_LABEL[o.kind];
              return (
                <button
                  key={o.name}
                  id={`model-option-${i}`}
                  type="button"
                  role="option"
                  aria-selected={i === at}
                  tabIndex={-1}
                  className={`model-option ${i === at ? "on" : ""}`}
                  onMouseEnter={() => setIndex(i)}
                  onClick={() => add(o.name)}
                >
                  <span className="ellipsis">
                    <Marked text={o.name} query={q} />
                  </span>
                  {kind && <span className="small muted nowrap">{t(kind)}</span>}
                  {i === at && q !== "" && <kbd>Tab</kbd>}
                </button>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

function PolicyDialog(props: { client: ClientInfo; onClose: () => void; onDone: () => void }) {
  const p = props.client.policy;
  const action = useAction();
  const accounts = usePoll(api.accounts, 0);
  const models = usePoll(api.models, 0);
  const virtuals = usePoll(api.virtuals, 0);
  const [allowed, setAllowed] = useState<string[]>(p.allowed_models ?? []);
  const [draft, setDraft] = useState("");
  const options = useMemo(
    () => (models.data && virtuals.data ? modelOptions(models.data, virtuals.data) : undefined),
    [models.data, virtuals.data],
  );
  const known = useMemo(
    () => (models.data && virtuals.data ? knownNames(models.data, virtuals.data) : undefined),
    [models.data, virtuals.data],
  );
  const [scope, setScope] = useState<Set<string>>(new Set(p.allowed_accounts ?? []));
  const [cost, setCost] = useState(p.daily_cost_limit_usd?.toString() ?? "");
  const [weekly, setWeekly] = useState(p.weekly_cost_limit_usd?.toString() ?? "");
  const [monthly, setMonthly] = useState<string>();
  const [rpm, setRpm] = useState(p.requests_per_minute?.toString() ?? "");
  // The key's own limits count per day and week; the native budget holds one window, used for the month.
  const budget = usePoll(
    () => native<{ monthlyLimitUsd?: number | null }>(`/api/usage/budget?apiKeyId=${encodeURIComponent(props.client.id)}`),
    0,
  );
  const monthlySaved = budget.data?.monthlyLimitUsd ? String(budget.data.monthlyLimitUsd) : "";
  const monthlyShown = monthly ?? monthlySaved;
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
      wide
      footer={
        <>
          <Button onClick={props.onClose}>{t("common.cancel")}</Button>
          <Button
            kind="primary"
            disabled={action.busy}
            onClick={() =>
              void action.run(async () => {
                // A name left half typed must not read as "no restriction".
                if (draft.trim()) throw new Error(t("clients.errDraft", { name: draft.trim() }));
                const gone = allowed.filter((name) => known && !known.has(name));
                if (gone.length > 0) throw new Error(t("clients.errGone", { list: gone.join(", ") }));
                await api.setClientPolicy(props.client.name, {
                  allowed_models: allowed,
                  allowed_accounts: [...scope],
                  daily_cost_limit_usd: number(cost, false),
                  weekly_cost_limit_usd: number(weekly, false),
                  requests_per_minute: number(rpm, true),
                });
                if (monthlyShown !== monthlySaved) {
                  await native("/api/usage/budget", "POST", {
                    apiKeyId: props.client.id,
                    dailyLimitUsd: 0,
                    weeklyLimitUsd: 0,
                    monthlyLimitUsd: number(monthlyShown, false) ?? 0,
                    resetInterval: "monthly",
                  });
                }
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
      <ModelPicker
        label={t("clients.allowed")}
        value={allowed}
        options={options}
        known={known}
        onChange={(value) => {
          setAllowed(value);
          action.setError(undefined);
        }}
        draft={draft}
        onDraft={(text) => {
          setDraft(text);
          action.setError(undefined);
        }}
      />
      <Field label={t("clients.scope")}>
        <div className="scope-grid">
          {(accounts.data ?? []).map((a) => {
            const name = `${a.provider}/${a.alias}`;
            return (
              <label key={name} className="check-row small">
                <input
                  type="checkbox"
                  checked={scope.has(name)}
                  onChange={() => {
                    const next = new Set(scope);
                    if (next.has(name)) next.delete(name);
                    else next.add(name);
                    setScope(next);
                  }}
                />
                <span className="mono ellipsis">{name}</span>
              </label>
            );
          })}
        </div>
      </Field>
      <div className="grid cols-4" style={{ gap: 10 }}>
        <Field label={t("clients.costPerDay")}>
          <input value={cost} placeholder={t("res.noLimit")} onChange={(e) => setCost(e.target.value)} inputMode="decimal" />
        </Field>
        <Field label={t("clients.costPerWeek")}>
          <input value={weekly} placeholder={t("res.noLimit")} onChange={(e) => setWeekly(e.target.value)} inputMode="decimal" />
        </Field>
        <Field label={t("clients.costPerMonth")}>
          <input value={monthlyShown} placeholder={t("res.noLimit")} onChange={(e) => setMonthly(e.target.value)} inputMode="decimal" />
        </Field>
        <Field label={t("clients.rpm")}>
          <input value={rpm} placeholder={t("res.noLimit")} onChange={(e) => setRpm(e.target.value)} inputMode="numeric" />
        </Field>
      </div>
      <ErrorText error={action.error} />
    </Modal>
  );
}
