// SPDX-License-Identifier: Apache-2.0

import { useState } from "react";
import { api } from "../api";
import { Card, Empty, ErrorText, Page } from "../components";
import { ms, num, usd } from "../format";
import { usePoll } from "../hooks";

const GROUPS = [
  { id: "model", label: "Model" },
  { id: "route", label: "Route" },
  { id: "provider", label: "Provider" },
  { id: "client", label: "Client" },
  { id: "day", label: "Day (UTC)" },
];

export function Stats() {
  const [group, setGroup] = useState("model");
  const [days, setDays] = useState(7);
  const stats = usePoll(() => api.stats(Date.now() - days * 86400000, group), 10000, [group, days]);
  const rows = stats.data ?? [];
  const total = rows.reduce(
    (t, r) => ({
      requests: t.requests + r.requests,
      errors: t.errors + r.errors,
      input: t.input + r.input_tokens,
      output: t.output + r.output_tokens,
      cost: t.cost + r.cost_usd,
    }),
    { requests: 0, errors: 0, input: 0, output: 0, cost: 0 },
  );
  const maxRequests = Math.max(1, ...rows.map((r) => r.requests));

  return (
    <Page
      title="Statistics"
      actions={
        <select value={days} onChange={(e) => setDays(Number(e.target.value))}>
          {[1, 7, 30, 90, 365].map((d) => (
            <option key={d} value={d}>
              last {d === 1 ? "24 hours" : `${d} days`}
            </option>
          ))}
        </select>
      }
    >
      <div className="grid cols-4">
        <Card>
          <div className="stat">
            <div className="label">Requests</div>
            <div className="value">{num(total.requests)}</div>
          </div>
        </Card>
        <Card>
          <div className="stat">
            <div className="label">Failed</div>
            <div className="value">
              {num(total.errors)}
              {total.requests > 0 && (
                <span className="small muted"> {((total.errors / total.requests) * 100).toFixed(1)}%</span>
              )}
            </div>
          </div>
        </Card>
        <Card>
          <div className="stat">
            <div className="label">Tokens in / out</div>
            <div className="value">
              {num(total.input)} / {num(total.output)}
            </div>
          </div>
        </Card>
        <Card>
          <div className="stat">
            <div className="label">Estimated cost</div>
            <div className="value">{usd(total.cost)}</div>
          </div>
        </Card>
      </div>
      <div className="tabs">
        {GROUPS.map((g) => (
          <button key={g.id} className={group === g.id ? "active" : ""} onClick={() => setGroup(g.id)}>
            By {g.label.replace(/^./, (c) => c.toLowerCase())}
          </button>
        ))}
      </div>
      <ErrorText error={stats.error?.message} />
      <Card>
        {rows.length === 0 && !stats.loading && <Empty>No requests in this period.</Empty>}
        {rows.length > 0 && (
          <table>
            <thead>
              <tr>
                <th>{GROUPS.find((g) => g.id === group)?.label}</th>
                <th style={{ width: "22%" }} />
                <th className="num">Requests</th>
                <th className="num">Failed</th>
                <th className="num">Tokens in / out</th>
                <th className="num">Cost</th>
                <th className="num">Avg TTFT</th>
                <th className="num">Avg tok/s</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={r.key}>
                  <td className="mono">{r.key}</td>
                  <td>
                    <div className="meter" style={{ height: 10, minWidth: 60 }}>
                      <div className="meter-fill ok" style={{ width: `${(r.requests / maxRequests) * 100}%` }} />
                    </div>
                  </td>
                  <td className="num">{num(r.requests)}</td>
                  <td className="num">{r.errors > 0 ? num(r.errors) : "-"}</td>
                  <td className="num">
                    {num(r.input_tokens)} / {num(r.output_tokens)}
                  </td>
                  <td className="num">{usd(r.cost_usd)}</td>
                  <td className="num">{ms(r.avg_ttft_ms)}</td>
                  <td className="num">{r.avg_tps != null ? r.avg_tps.toFixed(0) : "-"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>
      <p className="small muted">
        Costs are estimated from model prices (Models page) and the token counts reported by upstreams.
        Request history is kept for 400 days.
      </p>
    </Page>
  );
}
