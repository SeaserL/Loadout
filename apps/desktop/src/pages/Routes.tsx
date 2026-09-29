// SPDX-License-Identifier: Apache-2.0

import { useMemo, useState } from "react";
import { api, type BenchResult, type PoolInfo, type TargetStatus } from "../api";
import { Badge, Button, Card, Empty, ErrorText, Field, Modal, Page, stateTone, tierTone } from "../components";
import { ago, ms, tierLabel, time } from "../format";
import { useAction, usePoll } from "../hooks";

export function Routes() {
  const [tab, setTab] = useState<"virtual" | "health">("virtual");
  const virtuals = usePoll(api.virtuals, 5000);
  const routes = usePoll(api.routeStatus, 5000);
  const pools = usePoll(api.pools, 10000);
  const action = useAction();
  const [bench, setBench] = useState<BenchResult[]>();
  const [editing, setEditing] = useState<PoolInfo | "new">();

  const byName = useMemo(() => {
    const m = new Map<string, TargetStatus>();
    for (const r of routes.data ?? []) if (!m.has(r.name)) m.set(r.name, r);
    return m;
  }, [routes.data]);

  const measure = (names: string[]) =>
    void action.run(async () => {
      setBench(await api.bench(names));
      await Promise.all([virtuals.reload(), routes.reload()]);
    });

  return (
    <Page
      title="Routes"
      actions={
        <>
          <Button disabled={action.busy} onClick={() => measure([])}>
            {action.busy ? "Measuring…" : "Measure unmeasured routes"}
          </Button>
          <Button kind="primary" onClick={() => setEditing("new")}>
            New pool
          </Button>
        </>
      }
    >
      <div className="notice small">
        Clients choose a route by the <span className="mono">model</span> name. <span className="mono">loadout/realtime</span>{" "}
        uses only ultra-fast routes, <span className="mono">loadout/fast</span> ultra-fast and fast ones,{" "}
        <span className="mono">loadout/default</span> everything (fastest first). The first available route in a list
        serves the request; the next ones take over when it fails or runs out of quota.
      </div>
      <div className="tabs">
        <button className={tab === "virtual" ? "active" : ""} onClick={() => setTab("virtual")}>
          Virtual models & pools
        </button>
        <button className={tab === "health" ? "active" : ""} onClick={() => setTab("health")}>
          Route health & latency
        </button>
      </div>
      <ErrorText error={action.error ?? virtuals.error?.message ?? routes.error?.message} />

      {tab === "virtual" && (
        <div className="grid cols-2">
          {(virtuals.data ?? []).map((v) => {
            const pool = pools.data?.find((p) => `loadout/${p.name}` === v.name);
            return (
              <Card
                key={v.name}
                title={<span className="mono">{v.name}</span>}
                actions={
                  <>
                    <Badge tone="info">{v.kind === "pool" ? `pool · ${v.ordering}` : "built-in"}</Badge>
                    <Button disabled={action.busy || v.routes.length === 0} onClick={() => measure([v.name])}>
                      Measure
                    </Button>
                    {pool && <Button onClick={() => setEditing(pool)}>Edit</Button>}
                  </>
                }
              >
                {v.routes.length === 0 ? (
                  <Empty>
                    {v.name === "loadout/realtime" || v.name === "loadout/fast"
                      ? "No measured route is fast enough yet. Measure routes or send some requests."
                      : "No routes."}
                  </Empty>
                ) : (
                  <ol className="routes">
                    {v.routes.map((r) => {
                      const s = byName.get(r);
                      return (
                        <li key={r}>
                          <div className="row">
                            <span className="mono">{r}</span>
                            <div className="spacer" />
                            {s && <Badge tone={tierTone(s.tier)}>{tierLabel(s.tier)}</Badge>}
                            {s && s.state !== "HEALTHY" && s.state !== "UNKNOWN" && (
                              <Badge tone={stateTone(s.state)}>{s.state}</Badge>
                            )}
                            <span className="small muted" style={{ minWidth: 90, textAlign: "right" }}>
                              {s?.ttft_p50_ms != null ? `${ms(s.ttft_p50_ms)}` : ""}
                              {s?.tps != null ? ` · ${s.tps.toFixed(0)} tok/s` : ""}
                            </span>
                          </div>
                        </li>
                      );
                    })}
                  </ol>
                )}
              </Card>
            );
          })}
        </div>
      )}

      {tab === "health" && (
        <Card>
          {(routes.data ?? []).length === 0 && <Empty>No routes.</Empty>}
          {(routes.data ?? []).length > 0 && (
            <table>
              <thead>
                <tr>
                  <th>Route</th>
                  <th>Protocol</th>
                  <th>State</th>
                  <th>Tier</th>
                  <th className="num">TTFT p50 / p90</th>
                  <th className="num">tok/s</th>
                  <th className="num">Samples</th>
                  <th className="num">OK / fail</th>
                  <th>Last measured</th>
                  <th>Last error</th>
                </tr>
              </thead>
              <tbody>
                {(routes.data ?? []).map((r) => (
                  <tr key={`${r.name}:${r.protocol}`}>
                    <td className="mono">{r.name}</td>
                    <td className="small">{r.protocol}</td>
                    <td>
                      <Badge tone={stateTone(r.state)} title={r.unavailable_until_ms ? `until ${time(r.unavailable_until_ms)}` : undefined}>
                        {r.state}
                      </Badge>
                    </td>
                    <td>
                      <Badge tone={tierTone(r.tier)}>
                        {tierLabel(r.tier)}
                        {r.tier_pinned ? " (pinned)" : ""}
                      </Badge>
                    </td>
                    <td className="num">
                      {ms(r.ttft_p50_ms)} / {ms(r.ttft_p90_ms)}
                    </td>
                    <td className="num">{r.tps != null ? r.tps.toFixed(0) : "-"}</td>
                    <td className="num">{r.samples}</td>
                    <td className="num">
                      {r.successes} / {r.failures}
                    </td>
                    <td className="small">{ago(r.last_sample_ms)}</td>
                    <td className="small muted" style={{ maxWidth: 260 }}>
                      {r.last_error}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Card>
      )}

      {bench && <BenchDialog results={bench} onClose={() => setBench(undefined)} />}
      {editing && (
        <PoolDialog
          pool={editing === "new" ? undefined : editing}
          routes={[...byName.keys()].sort()}
          onClose={() => setEditing(undefined)}
          onDone={() => void Promise.all([pools.reload(), virtuals.reload()])}
        />
      )}
    </Page>
  );
}

function BenchDialog(props: { results: BenchResult[]; onClose: () => void }) {
  return (
    <Modal title="Latency measurement" onClose={props.onClose} wide>
      {props.results.length === 0 ? (
        <Empty>Nothing to measure: every route already has latency data.</Empty>
      ) : (
        <table>
          <thead>
            <tr>
              <th>Route</th>
              <th>Result</th>
              <th className="num">TTFT</th>
              <th className="num">Total</th>
              <th>Tier now</th>
            </tr>
          </thead>
          <tbody>
            {props.results.map((r) => (
              <tr key={r.name}>
                <td className="mono">{r.name}</td>
                <td>
                  {r.ok ? (
                    <Badge tone="ok">ok</Badge>
                  ) : (
                    <>
                      <Badge tone="bad">failed</Badge>
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
      )}
      <p className="small muted">
        A tier needs at least 3 measurements; real requests count too. Each measurement is a tiny request
        (“Reply with exactly: OK”, at most 16 output tokens).
      </p>
    </Modal>
  );
}

function PoolDialog(props: { pool?: PoolInfo; routes: string[]; onClose: () => void; onDone: () => void }) {
  const action = useAction();
  const [name, setName] = useState(props.pool?.name ?? "");
  const [ordering, setOrdering] = useState<string>(props.pool?.ordering ?? "manual");
  const [members, setMembers] = useState<string[]>(props.pool?.members ?? []);
  const [pick, setPick] = useState("");
  const available = props.routes.filter((r) => !members.includes(r));
  const move = (i: number, d: number) =>
    setMembers((m) => {
      const next = [...m];
      const j = i + d;
      if (j < 0 || j >= next.length) return m;
      [next[i], next[j]] = [next[j] as string, next[i] as string];
      return next;
    });

  return (
    <Modal title={props.pool ? `Edit loadout/${props.pool.name}` : "New pool"} onClose={props.onClose} wide>
      <Field label="Name" hint="Clients use it as loadout/<name>.">
        <input
          value={name}
          disabled={!!props.pool}
          onChange={(e) => setName(e.target.value.toLowerCase())}
          placeholder="my-models"
        />
      </Field>
      <Field label="Order">
        <select value={ordering} onChange={(e) => setOrdering(e.target.value)}>
          <option value="manual">As listed below</option>
          <option value="latency">Fastest first (tier, then speed)</option>
        </select>
      </Field>
      <Field label="Models">
        <ol className="routes">
          {members.map((m, i) => (
            <li key={m}>
              <div className="row">
                <span className="mono">{m}</span>
                <div className="spacer" />
                <Button onClick={() => move(i, -1)} disabled={i === 0}>
                  ↑
                </Button>
                <Button onClick={() => move(i, 1)} disabled={i === members.length - 1}>
                  ↓
                </Button>
                <Button kind="danger" onClick={() => setMembers((cur) => cur.filter((x) => x !== m))}>
                  Remove
                </Button>
              </div>
            </li>
          ))}
        </ol>
        <div className="row" style={{ marginTop: 8 }}>
          <select value={pick} onChange={(e) => setPick(e.target.value)} style={{ flex: 1 }}>
            <option value="">Add a model…</option>
            {available.map((r) => (
              <option key={r}>{r}</option>
            ))}
          </select>
          <Button
            disabled={!pick}
            onClick={() => {
              setMembers((cur) => [...cur, pick]);
              setPick("");
            }}
          >
            Add
          </Button>
        </div>
      </Field>
      <ErrorText error={action.error} />
      <div className="row">
        {props.pool && (
          <Button
            kind="danger"
            disabled={action.busy}
            onClick={() =>
              void action.run(async () => {
                await api.removePool(props.pool?.name ?? "");
                props.onDone();
                props.onClose();
              })
            }
          >
            Delete pool
          </Button>
        )}
        <div className="spacer" />
        <Button onClick={props.onClose}>Cancel</Button>
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
          Save
        </Button>
      </div>
    </Modal>
  );
}
