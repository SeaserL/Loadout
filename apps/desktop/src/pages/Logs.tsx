// SPDX-License-Identifier: Apache-2.0

import { useState } from "react";
import { api } from "../api";
import { Badge, Card, Empty, ErrorText, Page } from "../components";
import { ms, num, time, usd } from "../format";
import { usePoll } from "../hooks";

export function Logs() {
  const [limit, setLimit] = useState(100);
  const recent = usePoll(() => api.recent(limit), 4000, [limit]);
  return (
    <Page
      title="Request log"
      actions={
        <select value={limit} onChange={(e) => setLimit(Number(e.target.value))}>
          {[50, 100, 500, 1000].map((n) => (
            <option key={n} value={n}>
              last {n}
            </option>
          ))}
        </select>
      }
    >
      <div className="notice small">
        Metadata only: prompts, responses and tool arguments are never stored. Entries older than 30 days are
        deleted automatically.
      </div>
      <ErrorText error={recent.error?.message} />
      <Card>
        {(recent.data ?? []).length === 0 && !recent.loading && <Empty>No requests yet.</Empty>}
        {(recent.data ?? []).length > 0 && (
          <table>
            <thead>
              <tr>
                <th>Time</th>
                <th>Client</th>
                <th>Model → route</th>
                <th>Protocol</th>
                <th>Status</th>
                <th className="num">Tokens in / out</th>
                <th className="num">TTFT</th>
                <th className="num">Total</th>
                <th className="num">Cost</th>
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
                    {r.mode === "converted" && <span className="muted"> (converted)</span>}
                  </td>
                  <td>
                    <Badge tone={r.status === "ok" ? "ok" : r.status === "client_closed" ? "warn" : "bad"}>
                      {r.status}
                      {r.error_category ? ` · ${r.error_category}` : ""}
                    </Badge>
                    {r.attempts > 1 && <span className="small muted"> {r.attempts} tries</span>}
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
