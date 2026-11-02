// SPDX-License-Identifier: Apache-2.0
//
// How a request gets served: pick a route group and see it end to end, clients on the left, the
// gateway in the middle and the ordered targets on the right, with the state of each.

import { useMemo, useState } from "react";
import { Pencil, Plus, Timer, Trash } from "lucide-react";
import { api, type BenchResult, type PoolInfo, type TargetStatus, type VirtualInfo } from "../api";
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
  stateTone,
  tierTone,
  useConfirm,
} from "../components";
import { ago, ms, num, tierLabel, time } from "../format";
import { useAction, usePoll } from "../hooks";
import { type MessageKey, t, tv } from "../i18n";
import { usePointerReorder } from "../reorder";
import { Resilience } from "./Resources";
import { GripVertical, X } from "lucide-react";

/** Loadout's two orders plus OmniRoute's strategies, each said in plain words. */
export const ORDERINGS: { value: string; label: MessageKey; hint: MessageKey }[] = [
  { value: "manual", label: "routes.ordManual", hint: "routes.orderManualHint" },
  { value: "latency", label: "routes.ordLatency", hint: "routes.orderLatencyHint" },
  { value: "round-robin", label: "routes.orderRotate", hint: "routes.orderRotateHint" },
  { value: "reset-aware", label: "routes.orderReset", hint: "routes.orderResetHint" },
  { value: "cost-optimized", label: "routes.orderCost", hint: "routes.orderCostHint" },
  { value: "least-used", label: "routes.orderLeast", hint: "routes.orderLeastHint" },
  { value: "random", label: "routes.orderRandom", hint: "routes.orderRandomHint" },
];
export function orderingLabel(ordering: string | null): string {
  const found = ORDERINGS.find((o) => o.value === ordering);
  return found ? t(found.label) : (ordering ?? "");
}
function orderingHint(ordering: string | null): string {
  const found = ORDERINGS.find((o) => o.value === ordering);
  return found ? t(found.hint) : "";
}

function shortTarget(name: string) {
  const [provider, account, ...rest] = name.split("/");
  return { provider, account, model: rest.join("/") };
}

function chainSummary(routes: string[]) {
  const targets = routes.map(shortTarget);
  if (targets.length > 1 && targets.every((x) => x.model === targets[0].model)) {
    return `${targets[0].model}：${targets.map((x) => `${x.provider}/${x.account}`).join(" → ")}`;
  }
  return targets.map((x) => x.model).join(" → ");
}

export function Routes(props: { gateway: string }) {
  const [tab, setTab] = useState<"groups" | "health" | "retry">("groups");
  const virtuals = usePoll(api.virtuals, 5000);
  const routes = usePoll(api.routeStatus, 5000);
  const pools = usePoll(api.pools, 10000);
  const recent = usePoll(() => api.recent({ limit: 30, since_ms: Date.now() - 3600000 }), 4000);
  const flow = usePoll(() => api.flow(Date.now() - 7 * 86400000), 20000);
  const clients = usePoll(api.clients, 30000);
  const action = useAction();
  const [bench, setBench] = useState<BenchResult[]>();
  const [editing, setEditing] = useState<PoolInfo | "new">();
  const [picked, setPicked] = useState<string>();

  const byName = useMemo(() => {
    const m = new Map<string, TargetStatus>();
    for (const r of routes.data ?? []) if (!m.has(r.name)) m.set(r.name, r);
    return m;
  }, [routes.data]);

  const all = virtuals.data ?? [];
  const groups = all.filter((v) => v.kind === "pool");
  const builtin = all.filter((v) => v.kind === "builtin");
  const selected =
    all.find((v) => v.name === picked) ?? groups.find((v) => v.enabled) ?? builtin.find((v) => v.enabled) ?? all[0];

  const measure = (names: string[]) =>
    void action.run(async () => {
      setBench(await api.bench(names));
      await Promise.all([virtuals.reload(), routes.reload()]);
    });
  const toggleVirtual = (v: VirtualInfo, on: boolean) =>
    void action.run(async () => {
      await api.setVirtual(v.name, on);
      await virtuals.reload();
    });

  return (
    <section className="page">
      <div className="page-header">
        <h1>{t("routes.title")}</h1>
        <HelpLink section={tab === "retry" ? "retry" : "routes"} />
        <div className="spacer" />
        <div className="actions">
          <Segmented
            label={t("routes.title")}
            value={tab}
            onChange={setTab}
            options={[
              { value: "groups", label: t("routes.tabVirtual") },
              { value: "health", label: t("routes.tabHealth") },
              { value: "retry", label: t("res.resilience") },
            ]}
          />
          <Button icon={<Timer size={16} />} disabled={action.busy} onClick={() => measure([])}>
            {action.busy ? t("routes.measuring") : t("routes.measureAll")}
          </Button>
          <Button kind="primary" icon={<Plus size={16} />} onClick={() => setEditing("new")}>
            {t("routes.newPool")}
          </Button>
        </div>
      </div>
      <ErrorText error={action.error ?? virtuals.error?.message ?? routes.error?.message} />

      <div className="tab-panel" key={tab}>
      {tab === "groups" && (
        <>
          {selected && (
            <RouteMap
              group={selected}
              gateway={props.gateway}
              byName={byName}
              clients={[
                ...new Set(
                  (flow.data?.links ?? [])
                    .filter((l) => selected.routes.includes(`${l.provider}/${l.account}/${l.model}`))
                    .filter((l) => (clients.data ?? []).some((c) => c.enabled && c.name === l.client))
                    .map((l) => l.client),
                ),
              ].slice(0, 5)}
              last={(recent.data ?? []).find((r) => r.route && selected.routes.includes(r.route))}
              busy={action.busy}
              onMeasure={() => measure([selected.name])}
            />
          )}

          <div className="pane-title">{t("routes.groupsTitle")}</div>
          <section className="rows">
            {groups.length === 0 && <Empty>{t("routes.noGroups")}</Empty>}
            {groups.map((v) => {
              const pool = pools.data?.find((p) => `loadout/${p.name}` === v.name);
              return (
                <div className={`route-group ${selected?.name === v.name ? "selected" : ""}`} key={v.name}>
                  <button type="button" className="route-group-main" onClick={() => setPicked(v.name)}>
                    <span className="row-main">
                      <span className="strong">{v.name}</span>
                      <span className="small muted ellipsis">
                        {chainSummary(v.routes) || t("routes.none")}
                      </span>
                    </span>
                  </button>
                  <span className="pill" title={orderingHint(v.ordering)}>
                    {orderingLabel(v.ordering)}
                  </span>
                  {pool && (
                    <IconButton label={t("common.edit")} onClick={() => setEditing(pool)}>
                      <Pencil size={15} />
                    </IconButton>
                  )}
                </div>
              );
            })}
          </section>

          <div className="pane-title">{t("routes.builtinTitle")}</div>
          <section className="rows">
            {builtin.map((v) => (
              <div className={`route-group ${selected?.name === v.name ? "selected" : ""}`} key={v.name}>
                <button type="button" className="route-group-main" onClick={() => setPicked(v.name)}>
                  <span className="row-main">
                    <span className="strong">{v.name}</span>
                    <span className="small muted ellipsis">
                      {v.routes.length ? t("routes.builtinCount", { n: v.routes.length, first: v.routes[0] ?? "" }) : t("routes.noFastYet")}
                    </span>
                  </span>
                </button>
                <Switch
                  checked={!!v.enabled}
                  disabled={action.busy}
                  label={v.enabled ? t("common.disable") : t("common.enable")}
                  onChange={(on) => toggleVirtual(v, on)}
                />
              </div>
            ))}
          </section>
        </>
      )}

      {tab === "health" && (
        <section>
          {(routes.data ?? []).length === 0 && <Empty>{t("routes.none")}</Empty>}
          {(routes.data ?? []).length > 0 && (
            <div className="table-wrap">
              <table>
                <thead>
                  <tr>
                    <th>{t("routes.route")}</th>
                    <th>{t("routes.state")}</th>
                    <th>{t("models.tier")}</th>
                    <th className="num">{t("routes.ttft")}</th>
                    <th className="num">tok/s</th>
                    <th className="num">{t("routes.okFail")}</th>
                    <th>{t("routes.lastMeasured")}</th>
                    <th>{t("routes.lastError")}</th>
                  </tr>
                </thead>
                <tbody>
                  {(routes.data ?? []).map((r) => (
                    <tr key={`${r.name}:${r.protocol}`}>
                      <td className="break" style={{ minWidth: 180 }}>
                        {r.name}
                      </td>
                      <td>
                        <Badge
                          tone={stateTone(r.state)}
                          dot
                          title={r.unavailable_until_ms ? t("routes.until", { time: time(r.unavailable_until_ms) }) : undefined}
                        >
                          {tv("state", r.state)}
                        </Badge>
                      </td>
                      <td>
                        <Badge tone={tierTone(r.tier)}>
                          {tierLabel(r.tier)}
                          {r.tier_pinned ? ` (${t("tier.pinned")})` : ""}
                        </Badge>
                      </td>
                      <td className="num">
                        {ms(r.ttft_p50_ms)} / {ms(r.ttft_p90_ms)}
                      </td>
                      <td className="num">{r.tps != null ? r.tps.toFixed(0) : "-"}</td>
                      <td className="num">
                        {r.successes} / {r.failures}
                      </td>
                      <td className="small" style={{ whiteSpace: "nowrap" }}>
                        {ago(r.last_sample_ms)}
                      </td>
                      <td className="small muted" style={{ maxWidth: 260 }}>
                        {r.last_error}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </section>
      )}

      {tab === "retry" && <Resilience />}
      </div>

      {bench && <BenchDialog results={bench} onClose={() => setBench(undefined)} />}
      {editing && (
        <PoolDialog
          pool={editing === "new" ? undefined : editing}
          routes={[...byName.keys()].sort()}
          onClose={() => setEditing(undefined)}
          onDone={() => void Promise.all([pools.reload(), virtuals.reload()])}
        />
      )}
    </section>
  );
}

/** Clients, gateway, ordered targets. Lines are drawn from the real member order. */
function RouteMap(props: {
  group: VirtualInfo;
  gateway: string;
  byName: Map<string, TargetStatus>;
  clients: string[];
  last: { route: string | null; client: string | null; requested_model: string; status: string; ts_ms: number; decision?: string | null } | undefined;
  busy: boolean;
  onMeasure: () => void;
}) {
  const g = props.group;
  const targets = g.routes.slice(0, 8);
  const hidden = g.routes.length - targets.length;
  const clients = props.clients.length ? props.clients : [t("routes.anyClient")];
  return (
    <section className="card route-map">
      <header className="route-map-head">
        <div>
          <div className="row" style={{ gap: 10 }}>
            <h2>{g.name}</h2>
            {!g.enabled && <Badge>{t("common.disabled")}</Badge>}
          </div>
        </div>
        <Button icon={<Timer size={15} />} disabled={props.busy || g.routes.length === 0} onClick={props.onMeasure}>
          {t("routes.measure")}
        </Button>
      </header>
      <div className="route-map-body">
        <div className="map-col map-clients">
          {clients.map((c) => (
            <div className="map-client" key={c}>
              <span className="ellipsis">{c}</span>
            </div>
          ))}
        </div>
        <div className="map-link left" aria-hidden />
        <div className="map-hub">
          <div className="strong">Loadout</div>
          <div className="small muted">{props.gateway}</div>
          <span className="pill dark" title={orderingHint(g.ordering)}>{orderingLabel(g.ordering)}</span>
        </div>
        <div className="map-link right" aria-hidden />
        <ol className="map-col map-targets">
          {targets.map((name, i) => {
            const s = props.byName.get(name);
            const { provider, account, model } = shortTarget(name);
            const blocked = s && !["HEALTHY", "UNKNOWN"].includes(s.state);
            const active = props.last?.route === name && Date.now() - (props.last?.ts_ms ?? 0) < 60000;
            return (
              <li key={name} className={`map-target ${blocked ? "blocked" : ""} ${active ? "active" : ""}`}>
                <span className={`state-ring ${active ? "on" : ""}`} />
                <span className="strong ellipsis map-target-name">{provider}</span>
                <span className="small muted">{account}</span>
                <span className="code-pill mono ellipsis" title={model}>
                  {model}
                </span>
                <span className="spacer" />
                {blocked ? (
                  <Badge tone={stateTone(s.state)}>{tv("state", s.state)}</Badge>
                ) : (
                  <span className="small muted num">
                    {s?.ttft_p50_ms != null ? ms(s.ttft_p50_ms) : i === 0 ? t("routes.first") : t("routes.standby")}
                  </span>
                )}
                {s && s.tier !== "UNMEASURED" && <Badge tone={tierTone(s.tier)}>{tierLabel(s.tier)}</Badge>}
              </li>
            );
          })}
          {hidden > 0 && <li className="small muted map-more">{t("routes.moreTargets", { n: num(hidden) })}</li>}
          {targets.length === 0 && <li className="small muted map-more">{t("routes.none")}</li>}
        </ol>
      </div>
      {props.last && (
        <footer className="route-map-foot small">
          <span className="muted">{t("routes.lastRequest")}</span>
          <span>{props.last.client ?? t("flow.unrecorded")}</span>
          <span>{props.last.route}</span>
          {props.last.decision && <span className="muted">{props.last.decision}</span>}
          <span className="spacer" />
          <Badge tone={props.last.status === "ok" ? "ok" : "bad"} dot>
            {tv("status", props.last.status)}
          </Badge>
        </footer>
      )}
    </section>
  );
}

function BenchDialog(props: { results: BenchResult[]; onClose: () => void }) {
  return (
    <Modal
      title={t("routes.benchTitle")}
      onClose={props.onClose}
      wide
      footer={<Button onClick={props.onClose}>{t("common.close")}</Button>}
    >
      {props.results.length === 0 ? (
        <Empty>{t("routes.benchNothing")}</Empty>
      ) : (
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>{t("routes.route")}</th>
                <th>{t("routes.result")}</th>
                <th className="num">TTFT</th>
                <th className="num">{t("routes.total")}</th>
                <th>{t("routes.tierNow")}</th>
              </tr>
            </thead>
            <tbody>
              {props.results.map((r) => (
                <tr key={r.name}>
                  <td className="mono break">{r.name}</td>
                  <td>
                    {r.ok ? (
                      <Badge tone="ok">{tv("status", "ok")}</Badge>
                    ) : (
                      <>
                        <Badge tone="bad">{t("routes.failed")}</Badge>
                        <div className="small muted">{r.error}</div>
                      </>
                    )}
                  </td>
                  <td className="num">{ms(r.ttft_ms)}</td>
                  <td className="num">{ms(r.duration_ms)}</td>
                  <td>
                    <Badge tone={tierTone(r.tier)}>{tierLabel(r.tier)}</Badge>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </Modal>
  );
}

function PoolDialog(props: { pool?: PoolInfo; routes: string[]; onClose: () => void; onDone: () => void }) {
  const action = useAction();
  const confirm = useConfirm();
  const [name, setName] = useState(props.pool?.name ?? "");
  const [ordering, setOrdering] = useState<string>(props.pool?.ordering ?? "manual");
  const [members, setMembers] = useState<string[]>(props.pool?.members ?? []);
  const [pick, setPick] = useState("");
  const available = props.routes.filter((r) => !members.includes(r));
  const order = usePointerReorder({
    scope: "pool-dialog",
    scopeAttr: "pool-dialog",
    itemAttr: "pool-member",
    ids: members,
    onReorder: setMembers,
  });

  return (
    <Modal
      title={props.pool ? t("routes.editPool", { name: props.pool.name }) : t("routes.newPool")}
      onClose={props.onClose}
      wide
      footer={
        <>
          {props.pool && (
            <Button
              kind="danger"
              icon={<Trash size={15} />}
              disabled={action.busy}
              onClick={() =>
                void confirm({
                  title: t("routes.deletePoolTitle", { name: props.pool?.name ?? "" }),
                  message: t("routes.confirmDeletePool"),
                  confirmLabel: t("routes.deletePool"),
                  danger: true,
                }).then(
                  (ok) =>
                    ok &&
                    void action.run(async () => {
                      await api.removePool(props.pool?.name ?? "");
                      props.onDone();
                      props.onClose();
                    }),
                )
              }
            >
              {t("routes.deletePool")}
            </Button>
          )}
          <div className="spacer" />
          <Button onClick={props.onClose}>{t("common.cancel")}</Button>
          <Button
            kind="primary"
            disabled={action.busy || !name || members.length === 0}
            onClick={() =>
              void action.run(async () => {
                await api.savePool(name, ordering, members);
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
        <Field label={t("common.name")}>
          <input
            value={name}
            disabled={!!props.pool}
            onChange={(e) => setName(e.target.value.toLowerCase())}
            placeholder="my-models"
          />
        </Field>
        <Field label={t("routes.order")} hint={orderingHint(ordering)}>
          <select value={ordering} onChange={(e) => setOrdering(e.target.value)}>
            {ORDERINGS.map((o) => (
              <option key={o.value} value={o.value}>
                {t(o.label)}
              </option>
            ))}
          </select>
        </Field>
      </div>
      <div className="field">
        <span className="label">{t("routes.models")}</span>
        {members.length === 0 && <div className="small muted">{t("routes.noMembers")}</div>}
        <ol className="pool-members" data-pool-dialog="pool-dialog">
          {members.map((m) => {
            const grip = order.grip(m);
            return (
              <li key={m} data-pool-member={m} className={order.stateClass(m)}>
                {grip && (
                  <span className="drag-grip" role="button" tabIndex={0} aria-label={t("acc.dragHint")} {...grip}>
                    <GripVertical size={14} aria-hidden />
                  </span>
                )}
                <span className="mono ellipsis" style={{ flex: 1 }}>
                  {m}
                </span>
                <IconButton label={t("common.remove")} onClick={() => setMembers((cur) => cur.filter((x) => x !== m))}>
                  <X size={14} />
                </IconButton>
              </li>
            );
          })}
        </ol>
        <div className="row" style={{ marginTop: 8 }}>
          <select value={pick} onChange={(e) => setPick(e.target.value)} style={{ flex: 1 }} aria-label={t("routes.addModel")}>
            <option value="">{t("routes.addModel")}</option>
            {available.map((r) => (
              <option key={r}>{r}</option>
            ))}
          </select>
          <Button
            icon={<Plus size={16} />}
            disabled={!pick}
            onClick={() => {
              setMembers((cur) => [...cur, pick]);
              setPick("");
            }}
          >
            {t("common.add")}
          </Button>
        </div>
      </div>
      <ErrorText error={action.error} />
    </Modal>
  );
}
