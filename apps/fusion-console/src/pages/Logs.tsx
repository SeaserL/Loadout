// SPDX-License-Identifier: Apache-2.0

import { Fragment, useState } from "react";
import { ChevronDown, ChevronRight, Search } from "lucide-react";
import { api } from "../api";
import { Badge, Empty, ErrorText, Page, Switch } from "../components";
import { ms, num, shortTime, time, usd } from "../format";
import { usePoll } from "../hooks";
import { type MessageKey, t, tv } from "../i18n";

const PERIODS: { label: MessageKey; hours: number }[] = [
  { label: "logs.anyTime", hours: 0 },
  { label: "logs.lastHour", hours: 1 },
  { label: "logs.last24h", hours: 24 },
  { label: "logs.last7d", hours: 168 },
];

export function Logs() {
  const [limit, setLimit] = useState(100);
  const [client, setClient] = useState("");
  const [search, setSearch] = useState("");
  const [errorsOnly, setErrorsOnly] = useState(false);
  const [hours, setHours] = useState(0);
  // Rows whose routing decision is expanded.
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const toggle = (key: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (!next.delete(key)) next.add(key);
      return next;
    });
  const clients = usePoll(api.clients, 0);
  const recent = usePoll(
    () =>
      api.recent({
        limit,
        client: client || null,
        search: search.trim() || null,
        status: errorsOnly ? "error" : null,
        since_ms: hours ? Date.now() - hours * 3600000 : null,
      }),
    4000,
    [limit, client, search, errorsOnly, hours],
  );
  return (
    <Page title={t("logs.title")} help="logs">
      <div className="toolbar">
        <label className="search grow">
          <Search size={16} aria-hidden />
          <input placeholder={t("logs.search")} aria-label={t("logs.search")} value={search} onChange={(e) => setSearch(e.target.value)} />
        </label>
        <select value={client} aria-label={t("logs.client")} onChange={(e) => setClient(e.target.value)}>
          <option value="">{t("logs.allClients")}</option>
          {(clients.data ?? []).map((c) => (
            <option key={c.id} value={c.name}>
              {c.name}
            </option>
          ))}
        </select>
        <select value={hours} aria-label={t("stats.period")} onChange={(e) => setHours(Number(e.target.value))}>
          {PERIODS.map((p) => (
            <option key={p.hours} value={p.hours}>
              {t(p.label)}
            </option>
          ))}
        </select>
        <select value={limit} aria-label={t("logs.rows")} onChange={(e) => setLimit(Number(e.target.value))}>
          {[50, 100, 500, 1000].map((n) => (
            <option key={n} value={n}>
              {t("logs.upTo", { n })}
            </option>
          ))}
        </select>
        <span className="check-row small">
          <Switch checked={errorsOnly} onChange={setErrorsOnly} label={t("logs.failedOnly")} />
          {t("logs.failedOnly")}
        </span>
      </div>
      <ErrorText error={recent.error?.message} />
      {(recent.data ?? []).length === 0 && !recent.loading && <Empty>{t("logs.empty")}</Empty>}
      {(recent.data ?? []).length > 0 && (
        <div className="table-wrap">
          <table className="log-table">
            <thead>
              <tr>
                <th>{t("logs.time")}</th>
                <th>{t("logs.client")}</th>
                <th>{t("logs.modelRoute")}</th>
                <th>{t("logs.status")}</th>
                <th className="num">{t("logs.tokens")}</th>
                <th className="num">TTFT / {t("logs.total")}</th>
                <th className="num">{t("logs.cost")}</th>
              </tr>
            </thead>
            <tbody>
              {(recent.data ?? []).map((r, i) => {
                // Stable across polls (new rows shift indexes), so only new rows animate in.
                const rowId = r.id ?? `${r.ts_ms}|${r.requested_model}|${r.duration_ms}|${i}`;
                const open = expanded.has(rowId);
                const failed = r.status !== "ok";
                return (
                  <Fragment key={rowId}>
                    <tr className={`${failed ? "failed" : ""} ${open ? "open" : ""}`} onClick={() => toggle(rowId)}>
                      <td className="small nowrap" title={time(r.ts_ms)}>
                        <button
                          type="button"
                          className="row-toggle"
                          aria-expanded={open}
                          aria-label={t("logs.showDecision")}
                          onClick={(e) => {
                            e.stopPropagation();
                            toggle(rowId);
                          }}
                        >
                          {open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                        </button>
                        {shortTime(r.ts_ms)}
                      </td>
                      <td className="small">{r.client ?? "-"}</td>
                      <td style={{ minWidth: 200 }}>
                        <div className="break">{r.requested_model}</div>
                        {r.route && r.route !== r.requested_model && <div className="small muted break">{r.route}</div>}
                      </td>
                      <td>
                        <Badge tone={r.status === "ok" ? "ok" : r.status === "client_closed" ? "warn" : "bad"} dot>
                          {r.error_category ?? tv("status", r.status)}
                        </Badge>
                        {r.attempts > 1 && <div className="small muted">{t("logs.tries", { n: r.attempts })}</div>}
                      </td>
                      <td className="num">
                        {num(r.input_tokens)} / {num(r.output_tokens)}
                      </td>
                      <td className="num">
                        <div>{ms(r.ttft_ms)}</div>
                        <div className="small muted">{ms(r.duration_ms)}</div>
                      </td>
                      <td className="num">{usd(r.cost_usd)}</td>
                    </tr>
                    {open && (
                      <tr className="log-detail">
                        <td colSpan={7} className="small">
                          <span className="muted">{t("logs.protocol")} </span>
                          {r.protocol_in} → {r.protocol_out ?? "-"}
                          {r.decision && (
                            <>
                              <span className="muted"> {t("logs.decision")} </span>
                              <span className="break">{r.decision}</span>
                            </>
                          )}
                        </td>
                      </tr>
                    )}
                  </Fragment>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </Page>
  );
}
