// SPDX-License-Identifier: Apache-2.0

import { api, type AccountUsage, type DaemonStatus } from "../api";
import { Badge, Card, Empty, Meter, Page, copy, tierTone } from "../components";
import { ago, ms, num, tierLabel, until, usd } from "../format";
import { usePoll } from "../hooks";

function totals(usage: AccountUsage[] | undefined) {
  const t = { requests: 0, input: 0, output: 0, cost: 0, monthCost: 0 };
  for (const u of usage ?? []) {
    t.requests += u.spend_today.requests;
    t.input += u.spend_today.input_tokens;
    t.output += u.spend_today.output_tokens;
    t.cost += u.spend_today.cost_usd;
    t.monthCost += u.spend_month.cost_usd;
  }
  return t;
}

export function Dashboard(props: { status: DaemonStatus }) {
  const usage = usePoll(() => api.usage(false), 10000);
  const virtuals = usePoll(api.virtuals, 5000);
  const routes = usePoll(api.routeStatus, 5000);
  const recent = usePoll(() => api.recent({ limit: 500, status: "error", since_ms: Date.now() - 86400000 }), 5000);
  const t = totals(usage.data);
  const gateway = `http://${props.status.gateway.addr}/v1`;
  const errorsToday = (recent.data ?? []).filter(
    (r) => r.status !== "ok" && Date.now() - r.ts_ms < 86400000,
  ).length;
  const byName = new Map((routes.data ?? []).map((r) => [r.name, r]));

  return (
    <Page title="Dashboard">
      <div className="notice row">
        <div>
          <strong>All your AI, one endpoint.</strong>
          <div className="mono">{gateway}</div>
        </div>
        <div className="spacer" />
        <button className="btn" onClick={() => copy(gateway)}>
          Copy address
        </button>
      </div>

      <div className="grid cols-4">
        <Card>
          <div className="stat">
            <div className="label">Requests today</div>
            <div className="value">{num(t.requests)}</div>
          </div>
        </Card>
        <Card>
          <div className="stat">
            <div className="label">Tokens today (in / out)</div>
            <div className="value">
              {num(t.input)} / {num(t.output)}
            </div>
          </div>
        </Card>
        <Card>
          <div className="stat">
            <div className="label">API spend today · month</div>
            <div className="value">
              {usd(t.cost)} · {usd(t.monthCost)}
            </div>
          </div>
        </Card>
        <Card>
          <div className="stat">
            <div className="label">Failed requests (24 h)</div>
            <div className="value">{errorsToday}</div>
          </div>
        </Card>
      </div>

      <div className="grid cols-2">
        <Card title="Virtual models">
          {(virtuals.data ?? []).length === 0 && <Empty>No routes yet.</Empty>}
          <table>
            <tbody>
              {(virtuals.data ?? []).map((v) => {
                const first = v.routes[0];
                const r = first ? byName.get(first) : undefined;
                return (
                  <tr key={v.name}>
                    <td className="mono">{v.name}</td>
                    <td>{first ?? <span className="muted">no route yet</span>}</td>
                    <td>{r && <Badge tone={tierTone(r.tier)}>{tierLabel(r.tier)}</Badge>}</td>
                    <td className="num small muted">{r ? ms(r.ttft_p50_ms) : ""}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </Card>

        <Card title="Accounts">
          {(usage.data ?? []).length === 0 && <Empty>No accounts yet. Add one under Accounts.</Empty>}
          <table>
            <tbody>
              {(usage.data ?? []).map((u) => {
                const tightest = u.windows
                  .filter((w) => w.remaining_percent != null)
                  .sort((a, b) => (a.remaining_percent ?? 100) - (b.remaining_percent ?? 100))[0];
                const balance = u.balances[0];
                return (
                  <tr key={`${u.provider}/${u.alias}`}>
                    <td>
                      <div>
                        {u.provider}/{u.alias}
                      </div>
                      <div className="small muted">
                        {u.error ? (
                          <span style={{ color: "var(--bad)" }}>{u.error}</span>
                        ) : u.supported ? (
                          `updated ${ago(u.fetched_at_ms)}`
                        ) : null}
                      </div>
                    </td>
                    <td style={{ width: 220 }}>
                      {tightest ? (
                        <>
                          <Meter
                            remaining={tightest.remaining_percent}
                            label={`${tightest.label}: ${tightest.remaining_percent?.toFixed(0)}% left`}
                          />
                          <div className="small muted">resets {until(tightest.resets_at_ms)}</div>
                        </>
                      ) : balance ? (
                        <span>
                          {balance.currency} {balance.amount.toFixed(2)}
                        </span>
                      ) : (
                        <span className="muted small">{u.supported ? "no data yet" : "usage estimated"}</span>
                      )}
                    </td>
                    <td className="num small">{usd(u.spend_month.cost_usd)} this month</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </Card>
      </div>
    </Page>
  );
}
