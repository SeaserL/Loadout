// SPDX-License-Identifier: Apache-2.0
//
// Models and their groups. The left column is what clients can ask for (a group = a pool, an
// ordered failover list); the right column is everything the accounts offer. Dragging a model onto
// a group adds it; dragging inside a group sets its order.

import { useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, ChevronRight, Copy, FolderPlus, GripVertical, Plus, RefreshCw, Search, Trash, X } from "lucide-react";
import { api, type ModelInfo, type PoolInfo } from "../api";
import {
  Badge,
  Button,
  Empty,
  ErrorText,
  Field,
  HelpLink,
  IconButton,
  Modal,
  Segmented,
  Switch,
  tierTone,
  useConfirm,
  useCopy,
  useToast,
} from "../components";
import { num, tierLabel } from "../format";
import { useAction, usePoll } from "../hooks";
import { t } from "../i18n";
import { usePointerReorder } from "../reorder";
import { Prices } from "./Prices";
import { FreeCatalog, PriceTable } from "./Resources";
import { ORDERINGS, orderingLabel } from "./Routes";

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

/** Follows the pointer while a model is dragged out of the catalog. */
function useModelDrag(onDrop: (model: string, pool: string) => void) {
  const [dragging, setDragging] = useState<string>();
  const [over, setOver] = useState<string>();
  const ghost = useRef<HTMLDivElement>(null);
  const drop = useRef(onDrop);
  drop.current = onDrop;
  useEffect(() => {
    if (!dragging) return;
    const poolAt = (x: number, y: number) =>
      document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-pool]")?.getAttribute("data-pool") ?? undefined;
    const move = (e: PointerEvent) => {
      if (ghost.current) ghost.current.style.transform = `translate(${e.clientX + 12}px, ${e.clientY + 8}px)`;
      setOver(poolAt(e.clientX, e.clientY));
    };
    const up = (e: PointerEvent) => {
      const pool = poolAt(e.clientX, e.clientY);
      if (pool) drop.current(dragging, pool);
      setDragging(undefined);
      setOver(undefined);
    };
    const cancel = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setDragging(undefined);
        setOver(undefined);
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("keydown", cancel);
    document.body.classList.add("reordering");
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("keydown", cancel);
      document.body.classList.remove("reordering");
    };
  }, [dragging]);
  return { dragging, over, ghost, start: setDragging };
}

export function Models() {
  const [tab, setTab] = useState<"groups" | "prices" | "table" | "free">("groups");
  return (
    <section className="page">
      <div className="page-header">
        <h1>{t("models.title")}</h1>
        <HelpLink section={tab === "free" ? "free" : tab === "groups" ? "models" : "prices"} />
        <div className="spacer" />
        <Segmented
          label={t("models.title")}
          value={tab}
          onChange={setTab}
          options={[
            { value: "groups", label: t("models.tabGroups") },
            { value: "prices", label: t("nav.prices") },
            { value: "table", label: t("res.pricing") },
            { value: "free", label: t("res.free") },
          ]}
        />
      </div>
      <div className="tab-panel" key={tab}>
        {tab === "groups" && <GroupsView />}
        {tab === "prices" && <Prices />}
        {tab === "table" && <PriceTable />}
        {tab === "free" && <FreeCatalog />}
      </div>
    </section>
  );
}

function GroupsView() {
  const models = usePoll(api.models, 15000);
  const routes = usePoll(api.routeStatus, 15000);
  const pools = usePoll(api.pools, 15000);
  const accounts = usePoll(api.accounts, 0);
  const action = useAction();
  const toast = useToast();
  const doCopy = useCopy();
  const [filter, setFilter] = useState("");
  const [showDisabled, setShowDisabled] = useState(false);
  const [closed, setClosed] = useState<Set<string>>(new Set());
  const [creating, setCreating] = useState<string | true>();
  const [price, setPrice] = useState<ModelInfo>();
  const [adding, setAdding] = useState(false);
  const [picking, setPicking] = useState<string>();

  const tierOf = useMemo(() => new Map((routes.data ?? []).map((r) => [r.name, r.tier])), [routes.data]);
  const all = models.data ?? [];
  const q = filter.trim().toLowerCase();
  const list = all.filter((m) => (showDisabled || m.enabled) && (!q || m.name.toLowerCase().includes(q)));
  const groups = useMemo(() => {
    const out = new Map<string, ModelInfo[]>();
    for (const m of list) {
      const key = `${m.provider}/${m.account}`;
      out.set(key, [...(out.get(key) ?? []), m]);
    }
    return [...out.entries()];
  }, [list]);

  const reload = async () => {
    await Promise.all([models.reload(), routes.reload(), pools.reload()]);
  };
  const run = (f: () => Promise<unknown>) =>
    void action.run(async () => {
      await f();
      await reload();
    });
  const addToPool = (model: string, name: string) => {
    const pool = pools.data?.find((p) => p.name === name);
    if (!pool) return;
    if (pool.members.includes(model)) {
      toast(t("models.alreadyInGroup", { name }), "info");
      return;
    }
    run(async () => {
      await api.savePool(name, pool.ordering, [...pool.members, model]);
      toast(t("models.addedToGroup", { name }), "ok");
    });
  };
  const drag = useModelDrag(addToPool);

  return (
    <>
      <ErrorText error={action.error ?? models.error?.message} />
      <div className="groups-layout">
        <aside className="groups-pane">
          <div className="pane-head">
            <h2>{t("models.groups")}</h2>
            <Button icon={<FolderPlus size={15} />} onClick={() => setCreating(true)}>
              {t("models.newGroup")}
            </Button>
          </div>
          {(pools.data ?? []).length === 0 && !pools.loading && (
            <div className={`group-empty ${drag.dragging ? "armed" : ""}`}>{t("models.noGroups")}</div>
          )}
          {(pools.data ?? []).map((pool) => (
            <PoolCard
              key={pool.name}
              pool={pool}
              over={drag.over === pool.name}
              armed={!!drag.dragging}
              tierOf={tierOf}
              busy={action.busy}
              run={run}
            />
          ))}
        </aside>

        <div className="catalog-pane">
          <div className="pane-tools">
            <label className="search" style={{ flex: 1 }}>
              <Search size={16} aria-hidden />
              <input
                placeholder={t("models.search")}
                aria-label={t("models.search")}
                value={filter}
                onChange={(e) => setFilter(e.target.value)}
              />
            </label>
            <span className="check-row small">
              <Switch checked={showDisabled} onChange={setShowDisabled} label={t("common.showDisabled")} />
              {t("common.showDisabled")}
            </span>
            <Button icon={<Plus size={15} />} onClick={() => setAdding(true)}>
              {t("models.addManual")}
            </Button>
          </div>
          {groups.length === 0 && !models.loading && <Empty>{t("models.empty")}</Empty>}
          {groups.map(([key, rows]) => {
            const open = !closed.has(key);
            const [provider, account] = key.split("/");
            return (
              <section className="model-group" key={key}>
                <header>
                  <button
                    type="button"
                    className="model-group-toggle"
                    aria-expanded={open}
                    onClick={() => {
                      const next = new Set(closed);
                      if (open) next.add(key);
                      else next.delete(key);
                      setClosed(next);
                    }}
                  >
                    {open ? <ChevronDown size={15} /> : <ChevronRight size={15} />}
                    <span className="strong">{provider}</span>
                    <span className="muted">{account}</span>
                    <span className="chip-count">{rows.length}</span>
                  </button>
                  <IconButton
                    label={t("acc.sync")}
                    disabled={action.busy}
                    onClick={() =>
                      run(async () => {
                        const r = await api.syncModels(provider as string, account as string);
                        toast(t("models.synced", { n: r.total }), "ok");
                      })
                    }
                  >
                    <RefreshCw size={15} />
                  </IconButton>
                </header>
                {open &&
                  rows.map((m) => {
                    const tier = tierOf.get(m.name) ?? "UNMEASURED";
                    return (
                      <div className={`model-row ${m.enabled ? "" : "dim"} ${drag.dragging === m.name ? "dragging" : ""}`} key={m.name}>
                        <span
                          className="drag-grip"
                          role="button"
                          tabIndex={0}
                          aria-label={t("models.dragToGroup", { name: m.upstream_model })}
                          title={t("models.dragToGroup", { name: m.upstream_model })}
                          onPointerDown={(e) => {
                            if (e.button !== 0) return;
                            e.preventDefault();
                            drag.start(m.name);
                          }}
                          onKeyDown={(e) => {
                            if (e.key === "Enter" || e.key === " ") {
                              e.preventDefault();
                              setPicking(m.name);
                            }
                          }}
                        >
                          <GripVertical size={14} aria-hidden />
                        </span>
                        <Switch
                          checked={m.enabled}
                          disabled={action.busy}
                          label={m.enabled ? t("common.disable") : t("common.enable")}
                          onChange={(on) => run(() => api.setModelEnabled(m.name, on))}
                        />
                        <span className="mono model-name ellipsis" title={m.name}>
                          {m.upstream_model}
                        </span>
                        {m.source === "manual" && <Badge>{t("models.manual")}</Badge>}
                        <span className="spacer" />
                        <button className="btn link small price-link" onClick={() => setPrice(m)} title={t("models.priceTitle", { name: m.name })}>
                          {m.input_price != null || m.output_price != null ? `$${m.input_price ?? "-"} / $${m.output_price ?? "-"}` : t("models.noPrice")}
                        </button>
                        <span className="small muted num model-context">{m.context_window ? num(m.context_window) : ""}</span>
                        <select
                          className={`tier-select tone-${tierTone(tier)}`}
                          value={tierValue(m)}
                          disabled={action.busy || !m.enabled}
                          aria-label={t("models.tier")}
                          onChange={(e) => run(() => api.setModelTier(m.name, e.target.value === "auto" ? null : e.target.value))}
                        >
                          <option value="auto">{tierLabel(tier)}</option>
                          <option value="ultra-fast">{t("models.tierPinned", { tier: tierLabel("ULTRA_FAST") })}</option>
                          <option value="fast">{t("models.tierPinned", { tier: tierLabel("FAST") })}</option>
                          <option value="normal">{t("models.tierPinned", { tier: tierLabel("NORMAL") })}</option>
                        </select>
                        <IconButton label={t("models.copy")} onClick={() => doCopy(m.name)}>
                          <Copy size={14} />
                        </IconButton>
                        {m.source === "manual" && (
                          <IconButton label={t("common.remove")} danger disabled={action.busy} onClick={() => run(() => api.removeModel(m.name))}>
                            <Trash size={14} />
                          </IconButton>
                        )}
                      </div>
                    );
                  })}
              </section>
            );
          })}
        </div>
      </div>

      {drag.dragging && (
        <div className="drag-ghost mono" ref={drag.ghost} aria-hidden>
          {drag.dragging.split("/").slice(2).join("/")}
        </div>
      )}
      {creating && (
        <GroupDialog
          first={typeof creating === "string" ? creating : undefined}
          onClose={() => setCreating(undefined)}
          onDone={() => void reload()}
        />
      )}
      {picking && (
        <Modal title={t("models.pickGroup")} onClose={() => setPicking(undefined)}>
          {(pools.data ?? []).map((p) => (
            <button
              key={p.name}
              type="button"
              className="pick-row"
              onClick={() => {
                addToPool(picking, p.name);
                setPicking(undefined);
              }}
            >
              <span className="mono">loadout/{p.name}</span>
              <span className="small muted">{t("models.memberCount", { n: p.members.length })}</span>
            </button>
          ))}
          <Button
            icon={<FolderPlus size={15} />}
            onClick={() => {
              setCreating(picking);
              setPicking(undefined);
            }}
          >
            {t("models.newGroup")}
          </Button>
        </Modal>
      )}
      {price && <PriceDialog model={price} onClose={() => setPrice(undefined)} onDone={() => void models.reload()} />}
      {adding && (
        <AddModelDialog
          accounts={(accounts.data ?? []).map((a) => `${a.provider}/${a.alias}`)}
          onClose={() => setAdding(false)}
          onDone={() => void models.reload()}
        />
      )}
    </>
  );
}

function PoolCard(props: {
  pool: PoolInfo;
  over: boolean;
  armed: boolean;
  tierOf: Map<string, string>;
  busy: boolean;
  run: (f: () => Promise<unknown>) => void;
}) {
  const p = props.pool;
  const confirm = useConfirm();
  const doCopy = useCopy();
  const manual = p.ordering === "manual";
  const members = usePointerReorder({
    scope: p.name,
    scopeAttr: "members",
    itemAttr: "member",
    ids: p.members,
    onReorder: (next) => props.run(() => api.savePool(p.name, p.ordering, next)),
  });
  return (
    <section className={`pool-card ${props.over ? "drop-target" : ""} ${props.armed ? "armed" : ""}`} data-pool={p.name}>
      <header>
        <button type="button" className="pool-name mono" title={t("models.copy")} onClick={() => doCopy(`loadout/${p.name}`)}>
          loadout/{p.name}
        </button>
        <select
          className="ordering-select"
          value={p.ordering}
          disabled={props.busy}
          aria-label={t("routes.order")}
          title={orderingLabel(p.ordering)}
          onChange={(e) => props.run(() => api.savePool(p.name, e.target.value, p.members))}
        >
          {ORDERINGS.map((o) => (
            <option key={o.value} value={o.value} title={t(o.hint)}>
              {t(o.label)}
            </option>
          ))}
        </select>
        <IconButton
          label={t("routes.deletePool")}
          danger
          disabled={props.busy}
          onClick={() =>
            void confirm({
              title: t("routes.deletePoolTitle", { name: p.name }),
              message: t("routes.confirmDeletePool"),
              confirmLabel: t("routes.deletePool"),
              danger: true,
            }).then((ok) => ok && props.run(() => api.removePool(p.name)))
          }
        >
          <Trash size={14} />
        </IconButton>
      </header>
      <ol className="pool-members" data-members={p.name}>
        {p.members.map((name, i) => {
          const tier = props.tierOf.get(name);
          const [provider, account, ...rest] = name.split("/");
          const grip = manual ? members.grip(name) : undefined;
          return (
            <li key={name} data-member={name} className={members.stateClass(name)}>
              {grip ? (
                <span className="drag-grip" role="button" tabIndex={0} aria-label={t("acc.dragHint")} title={t("acc.dragHint")} {...grip}>
                  <GripVertical size={14} aria-hidden />
                </span>
              ) : (
                <span className="member-index small muted num">{i + 1}</span>
              )}
              <span className="member-name ellipsis" title={name}>
                <span className="mono">{rest.join("/")}</span>
                <span className="small muted">
                  {provider}/{account}
                </span>
              </span>
              {tier && tier !== "UNMEASURED" && <Badge tone={tierTone(tier)}>{tierLabel(tier)}</Badge>}
              <IconButton
                label={t("common.remove")}
                disabled={props.busy || p.members.length === 1}
                onClick={() => props.run(() => api.savePool(p.name, p.ordering, p.members.filter((m) => m !== name)))}
              >
                <X size={13} />
              </IconButton>
            </li>
          );
        })}
      </ol>
    </section>
  );
}

function GroupDialog(props: { first?: string; onClose: () => void; onDone: () => void }) {
  const action = useAction();
  const models = usePoll(api.models, 0);
  const [name, setName] = useState("");
  const [ordering, setOrdering] = useState("manual");
  const [first, setFirst] = useState(props.first ?? "");
  return (
    <Modal
      title={t("models.newGroup")}
      onClose={props.onClose}
      footer={
        <>
          <Button onClick={props.onClose}>{t("common.cancel")}</Button>
          <Button
            kind="primary"
            disabled={action.busy || !name || !first}
            onClick={() =>
              void action.run(async () => {
                await api.savePool(name, ordering, [first]);
                props.onDone();
                props.onClose();
              })
            }
          >
            {t("common.create")}
          </Button>
        </>
      }
    >
      <Field label={t("common.name")}>
        <input value={name} autoFocus onChange={(e) => setName(e.target.value.toLowerCase())} placeholder="daily-coding" />
      </Field>
      <Field label={t("routes.order")}>
        <select value={ordering} onChange={(e) => setOrdering(e.target.value)}>
          {ORDERINGS.map((o) => (
            <option key={o.value} value={o.value}>
              {t(o.label)}
            </option>
          ))}
        </select>
      </Field>
      <Field label={t("models.firstMember")}>
        <select value={first} onChange={(e) => setFirst(e.target.value)}>
          <option value="">{t("acc.choose")}</option>
          {(models.data ?? [])
            .filter((m) => m.enabled)
            .map((m) => (
              <option key={m.name} value={m.name}>
                {m.name}
              </option>
            ))}
        </select>
      </Field>
      <ErrorText error={action.error} />
    </Modal>
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
  const [context, setContext] = useState("");
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
                  protocols: ["chat"],
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
      <Field label={t("models.contextOptional")}>
        <input value={context} onChange={(e) => setContext(e.target.value)} inputMode="numeric" />
      </Field>
      <ErrorText error={action.error} />
    </Modal>
  );
}
