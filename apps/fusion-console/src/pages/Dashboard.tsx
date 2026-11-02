// SPDX-License-Identifier: Apache-2.0
//
// The whole gateway on one picture: what is upstream, what is downstream, what moves between them
// right now. Getting started needs no checklist: the empty ends of the picture are the next step.

import { useEffect, useMemo, useRef, useState } from "react";
import { ChevronRight } from "lucide-react";
import { api, type DaemonStatus, type LiveDone, type LiveRequest, type RequestSummary, type StatsEntry } from "../api";
import { Badge, CopyField, HelpLink, Segmented, stateTone } from "../components";
import { Flow, type FlowMetric } from "../Flow";
import { ago, localDay, num, tokens, usd } from "../format";
import { usePoll, useRolling } from "../hooks";
import { t, tv } from "../i18n";
import { endpointApi } from "./Endpoints";

const RANGES = [
  { value: 1, label: "dash.rangeToday" as const },
  { value: 7, label: "dash.range7" as const },
  { value: 30, label: "dash.range30" as const },
];

function startOf(days: number) {
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  return d.getTime() - (days - 1) * 86400000;
}

/** Daily bars; today in the accent colour. */
function DayBars(props: { days: StatsEntry[] }) {
  const today = localDay(Date.now());
  const last = Array.from({ length: 7 }, (_, i) => localDay(new Date().setDate(new Date().getDate() - (6 - i))));
  const values = last.map((day) => {
    const e = props.days.find((d) => d.key === day);
    return { day, value: e ? e.input_tokens + e.output_tokens : 0 };
  });
  const max = Math.max(1, ...values.map((v) => v.value));
  return (
    <div className="day-bars" aria-label={t("dash.last7")}>
      {values.map((v) => (
        <a
          key={v.day}
          href={`#/stats/${v.day}`}
          className={`day-bar ${v.day === today ? "today" : ""}`}
          style={{ height: `${Math.max(6, (v.value / max) * 100)}%` }}
          title={`${v.day}: ${tokens(v.value)}`}
          aria-label={`${v.day}: ${tokens(v.value)}`}
        />
      ))}
    </div>
  );
}

/** Follows the gateway closely while something moves and lazily while nothing does. */
function useLive(onDone: (rows: LiveDone[]) => void) {
  const [active, setActive] = useState<LiveRequest[]>([]);
  const cursor = useRef<number | null>(null);
  const notify = useRef(onDone);
  notify.current = onDone;
  useEffect(() => {
    let stopped = false;
    let timer = 0;
    const tick = async () => {
      let delay = 1000;
      if (document.visibilityState === "visible") {
        try {
          const r = await api.live(cursor.current);
          if (stopped) return;
          cursor.current = r.cursor;
          setActive(r.active);
          if (r.done.length > 0) notify.current(r.done);
          if (r.active.length > 0) delay = 400;
        } catch {
          // The shell's status line reports a gateway that went away.
        }
      }
      if (!stopped) timer = window.setTimeout(tick, delay);
    };
    void tick();
    return () => {
      stopped = true;
      window.clearTimeout(timer);
    };
  }, []);
  return active;
}

function Elapsed(props: { from: number }) {
  const [, tick] = useState(0);
  useEffect(() => {
    const id = window.setInterval(() => tick((n) => n + 1), 500);
    return () => window.clearInterval(id);
  }, []);
  return <>{((Date.now() - props.from) / 1000).toFixed(1)} s</>;
}

function statusText(r: RequestSummary) {
  return r.status === "ok" ? tv("status", "ok") : (r.error_category ?? r.status);
}

export function Dashboard(props: { status: DaemonStatus }) {
  const [days, setDays] = useState(7);
  const [metric, setMetric] = useState<FlowMetric>("tokens");
  const since = startOf(days);
  const flow = usePoll(() => api.flow(since), 30000, [since]);
  const daily = usePoll(() => api.stats(startOf(7), "day"), 60000);
  const accounts = usePoll(api.accounts, 30000);
  const clients = usePoll(api.clients, 30000);
  const routes = usePoll(api.routeStatus, 8000);
  const endpoints = usePoll(endpointApi.list, 30000);
  const recent = usePoll(() => api.recent({ limit: 6 }), 15000);

  const [done, setDone] = useState<{ rows: LiveDone[]; tick: number }>({ rows: [], tick: 0 });
  const active = useLive((rows) => {
    setDone((d) => ({ rows, tick: d.tick + 1 }));
    void flow.reload();
    void recent.reload();
    void daily.reload();
  });

  const links = flow.data?.links ?? [];
  const totals = useMemo(() => {
    const out = { requests: 0, errors: 0, input: 0, output: 0, cost: 0 };
    for (const l of links) {
      out.requests += l.requests;
      out.errors += l.errors;
      out.input += l.input_tokens;
      out.output += l.output_tokens;
      out.cost += l.cost_usd ?? 0;
    }
    return out;
  }, [links]);

  const base = `http://${props.status.gateway.addr}`;
  const attention = (routes.data ?? []).filter((r) => !["HEALTHY", "UNKNOWN", "DISABLED"].includes(r.state));
  const totalTokens = totals.input + totals.output;
  const rolled = useRolling(metric === "tokens" ? totalTokens : totals.requests);
  const liveAccounts = useMemo(() => (accounts.data ?? []).filter((a) => a.enabled), [accounts.data]);
  const liveClients = useMemo(
    () => (clients.data ?? []).filter((c) => c.enabled).map((c) => c.name),
    [clients.data],
  );

  return (
    <section className="page dash">
      <header className="dash-head">
        <div className="dash-total">
          <span className="hero-number">{metric === "tokens" ? tokens(rolled) : num(Math.round(rolled))}</span>
          <span className="hero-unit">{metric === "tokens" ? t("dash.metricTokens") : t("dash.metricRequests")}</span>
          <span className="hero-sub">
            <span>{metric === "tokens" ? `${num(totals.requests)} ${t("dash.metricRequests")}` : `${tokens(totalTokens)} ${t("dash.metricTokens")}`}</span>
            <span>{usd(totals.cost)}</span>
            {totals.errors > 0 && (
              <a className="bad-text" href="#/logs">
                {t("flow.errors", { n: num(totals.errors) })}
              </a>
            )}
          </span>
        </div>
        <DayBars days={daily.data ?? []} />
        <HelpLink section="dashboard" />
        <div className="spacer" />
        {active.length > 0 && (
          <span className="live-chip">
            <i aria-hidden /> {t("dash.liveNow", { n: active.length })}
          </span>
        )}
        <Segmented
          label={t("dash.metric")}
          value={metric}
          onChange={setMetric}
          options={[
            { value: "tokens", label: t("dash.metricTokens") },
            { value: "requests", label: t("dash.metricRequests") },
          ]}
        />
        <Segmented
          label={t("dash.range")}
          value={days}
          onChange={setDays}
          options={RANGES.map((r) => ({ value: r.value, label: t(r.label) }))}
        />
      </header>

      <Flow
        key={days}
        links={links}
        metric={metric}
        active={active}
        done={done}
        accounts={liveAccounts.map((a) => ({ provider: a.provider, alias: a.alias }))}
        clients={liveClients}
      />

      <div className="connect-strip">
        <div className="connect-item">
          <span className="connect-label">{t("dash.apiAddress")}</span>
          <CopyField value={`${base}/v1`} />
        </div>
        {(endpoints.data ?? [])
          .filter((e) => e.enabled)
          .map((e) => (
            <div className="connect-item" key={e.name}>
              <span className="connect-label">{e.provider}</span>
              <CopyField value={e.openai_base_url} />
            </div>
          ))}
        <a className="connect-keys" href="#/clients">
          {t("dash.manageKeys")} <ChevronRight size={14} aria-hidden />
        </a>
      </div>

      <div className={attention.length > 0 ? "dash-lists two" : "dash-lists"}>
        <section>
          <header className="section-head">
            <h2>{t("dash.recent")}</h2>
            <a href="#/logs">
              {t("dash.allLogs")} <ChevronRight size={14} aria-hidden />
            </a>
          </header>
          {active.map((r) => (
            <div className="list-row recent-row running" key={r.id}>
              <span className="recent-time num">
                <Elapsed from={r.started_ms} />
              </span>
              <span className="ellipsis recent-client">{r.client || t("flow.unrecorded")}</span>
              <span className="ellipsis recent-route">
                {r.provider}/{r.account}/{r.model}
              </span>
              <span className="running-label">{r.answering ? t("dash.answering") : t("dash.waiting")}</span>
            </div>
          ))}
          {(recent.data ?? []).length === 0 && active.length === 0 && <div className="calm muted">{t("dash.noRequests")}</div>}
          {(recent.data ?? []).map((r, i) => (
            <div className="list-row recent-row" key={r.id ?? `${r.ts_ms}-${i}`}>
              <span className="recent-time num">{ago(r.ts_ms)}</span>
              <span className="ellipsis recent-client">{r.client ?? t("flow.unrecorded")}</span>
              <span className="ellipsis recent-route" title={r.route ?? r.requested_model}>
                {r.route ?? r.requested_model}
              </span>
              <span className="recent-tokens num">
                {r.input_tokens != null ? `${tokens(r.input_tokens)} / ${tokens(r.output_tokens ?? 0)}` : ""}
              </span>
              <Badge tone={r.status === "ok" ? "ok" : "bad"} dot>
                {statusText(r)}
              </Badge>
            </div>
          ))}
        </section>
        {attention.length > 0 && (
          <section>
            <header className="section-head">
              <h2>{t("dash.attention")}</h2>
              <a href="#/routes">
                {t("nav.routes")} <ChevronRight size={14} aria-hidden />
              </a>
            </header>
            {attention.slice(0, 7).map((r) => (
              <div className="list-row" key={r.name}>
                <span className="small ellipsis" style={{ flex: 1 }} title={r.name}>
                  {r.name}
                </span>
                <Badge tone={stateTone(r.state)} dot>
                  {tv("state", r.state)}
                </Badge>
              </div>
            ))}
          </section>
        )}
      </div>
    </section>
  );
}
