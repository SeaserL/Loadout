// SPDX-License-Identifier: Apache-2.0

import { api, type AccountUsage, type DaemonStatus } from "../api";
import { Badge, Card, Empty, Meter, Page, copy, tierTone } from "../components";
import { ago, ms, num, tierLabel, until, usd } from "../format";
import { usePoll } from "../hooks";
import { t } from "../i18n";

function totals(usage: AccountUsage[] | undefined) {
  const sum = { requests: 0, input: 0, output: 0, cost: 0, monthCost: 0 };
  for (const u of usage ?? []) {
    sum.requests += u.spend_today.requests;
    sum.input += u.spend_today.input_tokens;
    sum.output += u.spend_today.output_tokens;
    sum.cost += u.spend_today.cost_usd;
    sum.monthCost += u.spend_month.cost_usd;
  }
  return sum;
}

export function Dashboard(props: { status: DaemonStatus }) {
  const usage = usePoll(() => api.usage(false), 10000);
  const virtuals = usePoll(api.virtuals, 5000);
  const routes = usePoll(api.routeStatus, 5000);
  const recent = usePoll(() => api.recent({ limit: 500, status: "error", since_ms: Date.now() - 86400000 }), 5000);
  const sum = totals(usage.data);
  const gateway = `http://${props.status.gateway.addr}/v1`;
  const errorsToday = (recent.data ?? []).filter(
    (r) => r.status !== "ok" && Date.now() - r.ts_ms < 86400000,
  ).length;
  const byName = new Map((routes.data ?? []).map((r) => [r.name, r]));

  return (
    <Page title={t("dash.title")}>
      <div className="notice row">
        <div>
          <strong>{t("dash.slogan")}</strong>
          <div className="mono">{gateway}</div>
        </div>
        <div className="spacer" />
        <button className="btn" onClick={() => copy(gateway)}>
          {t("dash.copyAddress")}
        </button>
      </div>

      <div className="grid cols-4">
        <Card>
          <div className="stat">
            <div className="label">{t("dash.requestsToday")}</div>
            <div className="value">{num(sum.requests)}</div>
          </div>
        </Card>
        <Card>
          <div className="stat">
            <div className="label">{t("dash.tokensToday")}</div>
            <div className="value">
              {num(sum.input)} / {num(sum.output)}
            </div>
          </div>
        </Card>
        <Card>
          <div className="stat">
            <div className="label">{t("dash.spend")}</div>
            <div className="value">
              {usd(sum.cost)} · {usd(sum.monthCost)}
            </div>
          </div>
        </Card>
        <Card>
          <div className="stat">
            <div className="label">{t("dash.failed")}</div>
            <div className="value">{errorsToday}</div>
          </div>
        </Card>
      </div>

      <div className="grid cols-2">
        <Card title={t("dash.virtuals")}>
          {(virtuals.data ?? []).length === 0 && <Empty>{t("dash.noRoutes")}</Empty>}
          <table>
            <tbody>
              {(virtuals.data ?? []).map((v) => {
                const first = v.routes[0];
                const r = first ? byName.get(first) : undefined;
                return (
                  <tr key={v.name}>
                    <td className="mono">{v.name}</td>
                    <td>{first ?? <span className="muted">{t("dash.noRouteYet")}</span>}</td>
                    <td>{r && <Badge tone={tierTone(r.tier)}>{tierLabel(r.tier)}</Badge>}</td>
                    <td className="num small muted">{r ? ms(r.ttft_p50_ms) : ""}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </Card>

        <Card title={t("dash.accounts")}>
          {(usage.data ?? []).length === 0 && <Empty>{t("dash.noAccounts")}</Empty>}
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
                          t("dash.updated", { when: ago(u.fetched_at_ms) })
                        ) : null}
                      </div>
                    </td>
                    <td style={{ width: 220 }}>
                      {tightest ? (
                        <>
                          <Meter
                            remaining={tightest.remaining_percent}
                            label={`${tightest.label}: ${t("common.left", { n: tightest.remaining_percent?.toFixed(0) ?? "-" })}`}
                          />
                          <div className="small muted">
                            {t("common.resets", { when: until(tightest.resets_at_ms) })}
                          </div>
                        </>
                      ) : balance ? (
                        <span>
                          {balance.currency} {balance.amount.toFixed(2)}
                        </span>
                      ) : (
                        <span className="muted small">{u.supported ? t("dash.noData") : t("dash.estimated")}</span>
                      )}
                    </td>
                    <td className="num small">{t("dash.thisMonth", { cost: usd(u.spend_month.cost_usd) })}</td>
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
