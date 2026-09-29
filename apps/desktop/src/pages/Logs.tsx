// SPDX-License-Identifier: Apache-2.0

import { useState } from "react";
import { api } from "../api";
import { Badge, Card, Empty, ErrorText, Page } from "../components";
import { ms, num, time, usd } from "../format";
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
    <Page title={t("logs.title")}>
      <div className="notice small">{t("logs.privacy")}</div>
      <div className="row wrap" style={{ marginBottom: 12 }}>
        <input placeholder={t("logs.search")} value={search} onChange={(e) => setSearch(e.target.value)} />
        <select value={client} onChange={(e) => setClient(e.target.value)}>
          <option value="">{t("logs.allClients")}</option>
          {(clients.data ?? []).map((c) => (
            <option key={c.id} value={c.name}>
              {c.name}
            </option>
          ))}
        </select>
        <select value={hours} onChange={(e) => setHours(Number(e.target.value))}>
          {PERIODS.map((p) => (
            <option key={p.hours} value={p.hours}>
              {t(p.label)}
            </option>
          ))}
        </select>
        <label className="row small">
          <input type="checkbox" checked={errorsOnly} onChange={(e) => setErrorsOnly(e.target.checked)} />
          {t("logs.failedOnly")}
        </label>
        <div className="spacer" />
        <select value={limit} onChange={(e) => setLimit(Number(e.target.value))}>
          {[50, 100, 500, 1000].map((n) => (
            <option key={n} value={n}>
              {t("logs.upTo", { n })}
            </option>
          ))}
        </select>
      </div>
      <ErrorText error={recent.error?.message} />
      <Card>
        {(recent.data ?? []).length === 0 && !recent.loading && <Empty>{t("logs.empty")}</Empty>}
        {(recent.data ?? []).length > 0 && (
          <table>
            <thead>
              <tr>
                <th>{t("logs.time")}</th>
                <th>{t("logs.client")}</th>
                <th>{t("logs.modelRoute")}</th>
                <th>{t("logs.protocol")}</th>
                <th>{t("logs.status")}</th>
                <th className="num">{t("logs.tokens")}</th>
                <th className="num">TTFT</th>
                <th className="num">{t("logs.total")}</th>
                <th className="num">{t("logs.cost")}</th>
              </tr>
            </thead>
            <tbody>
              {(recent.data ?? []).map((r, i) => (
                <tr key={`${r.ts_ms}-${i}`}>
                  <td className="small">{time(r.ts_ms)}</td>
                  <td className="small">{r.client ?? "-"}</td>
                  <td>
                    <div className="mono">{r.requested_model}</div>
                    {r.route && r.route !== r.requested_model && <div className="mono small muted">→ {r.route}</div>}
                  </td>
                  <td className="small">
                    {r.protocol_in}→{r.protocol_out ?? "-"}
                    {r.mode === "converted" && <span className="muted"> {t("logs.converted")}</span>}
                  </td>
                  <td>
                    <Badge tone={r.status === "ok" ? "ok" : r.status === "client_closed" ? "warn" : "bad"}>
                      {tv("status", r.status)}
                      {r.error_category ? ` · ${r.error_category}` : ""}
                    </Badge>
                    {r.attempts > 1 && <span className="small muted"> {t("logs.tries", { n: r.attempts })}</span>}
                  </td>
                  <td className="num">
                    {num(r.input_tokens)} / {num(r.output_tokens)}
                  </td>
                  <td className="num">{ms(r.ttft_ms)}</td>
                  <td className="num">{ms(r.duration_ms)}</td>
                  <td className="num">{usd(r.cost_usd)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>
    </Page>
  );
}
