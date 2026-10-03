// SPDX-License-Identifier: Apache-2.0

import { Fragment, useState } from "react";
import { ChevronDown, ChevronRight, RefreshCw, Search } from "lucide-react";
import { api } from "../api";
import { Badge, Card, Empty, ErrorText, FilterBar, FilterField, IconButton, Notice, Page, Switch } from "../components";
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
    <Page
      title={t("logs.title")}
      actions={
        <IconButton label={t("common.refresh")} onClick={() => void recent.reload()}>
          <RefreshCw size={16} />
        </IconButton>
      }
    >
      <Notice>{t("logs.privacy")}</Notice>
      <FilterBar>
        <FilterField label={t("logs.searchLabel")} grow>
          <span className="search">
            <Search size={16} aria-hidden />
            <input placeholder={t("logs.search")} value={search} onChange={(e) => setSearch(e.target.value)} />
          </span>
        </FilterField>
        <FilterField label={t("logs.client")}>
          <select value={client} onChange={(e) => setClient(e.target.value)}>
            <option value="">{t("logs.allClients")}</option>
            {(clients.data ?? []).map((c) => (
              <option key={c.id} value={c.name}>
                {c.name}
              </option>
            ))}
          </select>
        </FilterField>
        <FilterField label={t("stats.period")}>
          <select value={hours} onChange={(e) => setHours(Number(e.target.value))}>
            {PERIODS.map((p) => (
              <option key={p.hours} value={p.hours}>
                {t(p.label)}
              </option>
            ))}
          </select>
        </FilterField>
        <FilterField label={t("logs.rows")}>
          <select value={limit} onChange={(e) => setLimit(Number(e.target.value))}>
            {[50, 100, 500, 1000].map((n) => (
              <option key={n} value={n}>
                {t("logs.upTo", { n })}
              </option>
            ))}
          </select>
        </FilterField>
        <div className="filter-check">
          <Switch checked={errorsOnly} onChange={setErrorsOnly} label={t("logs.failedOnly")} />
          {t("logs.failedOnly")}
        </div>
      </FilterBar>
      <ErrorText error={recent.error?.message} />
      <Card>
        {(recent.data ?? []).length === 0 && !recent.loading && <Empty>{t("logs.empty")}</Empty>}
        {(recent.data ?? []).length > 0 && (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>{t("logs.time")}</th>
                  <th>{t("logs.client")}</th>
                  <th>{t("logs.modelRoute")}</th>
                  <th>{t("logs.protocol")}</th>
                  <th>{t("logs.status")}</th>
                  <th className="num">{t("logs.tokens")}</th>
                  <th className="num">TTFT / {t("logs.total")}</th>
                  <th className="num">{t("logs.cost")}</th>
                </tr>
              </thead>
              <tbody>
                {(recent.data ?? []).map((r, i) => {
                  const key = `${r.ts_ms}-${i}`;
                  // Stable across polls (new rows shift indexes).
                  const rowId = `${r.ts_ms}|${r.requested_model}|${r.duration_ms}`;
                  const open = expanded.has(rowId);
                  return (
                    <Fragment key={key}>
                      <tr>
                        <td className="small" style={{ whiteSpace: "nowrap" }} title={time(r.ts_ms)}>
                          <button
                            type="button"
                            className="icon-btn"
                            aria-expanded={open}
                            aria-label={t("logs.showDecision")}
                            title={r.decision ?? t("logs.showDecision")}
                            onClick={() => toggle(rowId)}
                            style={{ width: 20, height: 20, marginRight: 4, verticalAlign: "middle" }}
                          >
                            {open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                          </button>
                          {shortTime(r.ts_ms)}
                        </td>
                        <td className="small">{r.client ?? "-"}</td>
                        <td style={{ minWidth: 200 }}>
                          <div className="mono break">{r.requested_model}</div>
                          {r.route && r.route !== r.requested_model && (
                            <div className="mono small muted break">→ {r.route}</div>
                          )}
                        </td>
                        <td className="small" style={{ whiteSpace: "nowrap" }}>
                          {r.protocol_in} → {r.protocol_out ?? "-"}
                          {r.mode === "converted" && <div className="muted">{t("logs.converted")}</div>}
                        </td>
                        <td>
                          <Badge tone={r.status === "ok" ? "ok" : r.status === "client_closed" ? "warn" : "bad"}>
                            {tv("status", r.status)}
                            {r.error_category ? ` · ${r.error_category}` : ""}
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
                        <tr>
                          <td colSpan={8} className="small">
                            <span className="muted">{t("logs.decision")}: </span>
                            {r.decision ? (
                              <span className="mono break">{r.decision}</span>
                            ) : (
                              <span className="muted">{t("logs.noDecision")}</span>
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
      </Card>
    </Page>
  );
}
