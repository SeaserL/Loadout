// SPDX-License-Identifier: Apache-2.0

import { CircleStop, RefreshCw, RotateCw } from "lucide-react";
import { api, type AccountUsage, type DaemonStatus } from "../api";
import {
  Badge,
  Button,
  Card,
  CopyField,
  Empty,
  ErrorText,
  Kpi,
  Page,
  QuotaBar,
  tierTone,
  useConfirm,
} from "../components";
import { ago, duration, ms, num, resetIn, tierLabel, tokens, usd } from "../format";
import { useAction, usePoll } from "../hooks";
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

export function Dashboard(props: { status: DaemonStatus; onDaemonChange: () => Promise<void> }) {
  const usage = usePoll(() => api.usage(false), 10000);
  const virtuals = usePoll(api.virtuals, 5000);
  const routes = usePoll(api.routeStatus, 5000);
  const recent = usePoll(() => api.recent({ limit: 500, status: "error", since_ms: Date.now() - 86400000 }), 5000);
  const sum = totals(usage.data);
  const errorsToday = (recent.data ?? []).filter(
    (r) => r.status !== "ok" && Date.now() - r.ts_ms < 86400000,
  ).length;
  const byName = new Map((routes.data ?? []).map((r) => [r.name, r]));

  return (
    <Page title={t("dash.title")}>
      <div className="grid cols-2 collapse home-top" style={{ marginBottom: 16 }}>
        <RuntimeCard status={props.status} onDaemonChange={props.onDaemonChange} />
        <EndpointsCard addr={props.status.gateway.addr} />
      </div>

      <div className="kpis">
        <Kpi label={t("dash.requestsToday")} value={num(sum.requests)} />
        <Kpi label={t("dash.tokensIn")} value={tokens(sum.input)} />
        <Kpi label={t("dash.tokensOut")} value={tokens(sum.output)} />
        <Kpi label={t("dash.spendToday")} value={usd(sum.cost)} sub={t("dash.thisMonth", { cost: usd(sum.monthCost) })} />
        <Kpi label={t("dash.failed")} value={num(errorsToday)} tone={errorsToday > 0 ? "warn" : undefined} />
      </div>

      <div className="grid cols-2 collapse">
        <Card title={t("dash.virtuals")} subtitle={t("dash.virtualsHint")}>
          {(virtuals.data ?? []).length === 0 && <Empty>{t("dash.noRoutes")}</Empty>}
          {(virtuals.data ?? []).map((v) => {
            const first = v.routes[0];
            const r = first ? byName.get(first) : undefined;
            return (
              <div className="list-row" key={v.name}>
                <div style={{ minWidth: 0, flex: 1 }}>
                  <div className="mono strong">{v.name}</div>
                  <div className="small muted ellipsis">
                    {first ? <span className="mono">{first}</span> : t("dash.noRouteYet")}
                  </div>
                </div>
                {r && <Badge tone={tierTone(r.tier)}>{tierLabel(r.tier)}</Badge>}
                <span className="small muted num" style={{ minWidth: 60, textAlign: "right" }}>
                  {r ? ms(r.ttft_p50_ms) : ""}
                </span>
              </div>
            );
          })}
        </Card>

        <Card title={t("dash.accounts")} subtitle={t("dash.accountsHint")}>
          {(usage.data ?? []).length === 0 && (
            <Empty action={<a href="#/accounts">{t("dash.goAccounts")}</a>}>{t("dash.noAccounts")}</Empty>
          )}
          {(usage.data ?? []).map((u) => {
            const tightest = u.windows
              .filter((w) => w.remaining_percent != null)
              .sort((a, b) => (a.remaining_percent ?? 100) - (b.remaining_percent ?? 100))[0];
            const balance = u.balances[0];
            return (
              <div className="list-row" key={`${u.provider}/${u.alias}`} style={{ alignItems: "flex-start" }}>
                <div style={{ width: 160, minWidth: 0, flex: "none" }}>
                  <div className="strong ellipsis">
                    {u.provider}/{u.alias}
                  </div>
                  <div className="small muted ellipsis">
                    {u.error ? (
                      <span style={{ color: "var(--bad)" }} title={u.error}>
                        {u.error}
                      </span>
                    ) : u.supported ? (
                      t("dash.updated", { when: ago(u.fetched_at_ms) })
                    ) : (
                      t("dash.estimated")
                    )}
                  </div>
                  <div className="small muted">{t("dash.thisMonth", { cost: usd(u.spend_month.cost_usd) })}</div>
                </div>
                <div style={{ flex: 1, minWidth: 0 }}>
                  {tightest ? (
                    <QuotaBar
                      label={tightest.label}
                      remaining={tightest.remaining_percent}
                      resets={resetIn(tightest.resets_at_ms)}
                    />
                  ) : balance ? (
                    <div className="strong">
                      {balance.currency} {balance.amount.toFixed(2)}
                    </div>
                  ) : (
                    <span className="muted small">{u.supported ? t("dash.noData") : ""}</span>
                  )}
                </div>
              </div>
            );
          })}
        </Card>
      </div>
    </Page>
  );
}

function RuntimeCard(props: { status: DaemonStatus; onDaemonChange: () => Promise<void> }) {
  const s = props.status;
  const action = useAction();
  const confirm = useConfirm();
  const stop = async () => {
    if (!(await confirm({ title: t("settings.confirmStopTitle"), message: t("settings.confirmStop"), danger: true, confirmLabel: t("settings.stop") })))
      return;
    await action.run(async () => {
      await api.stopDaemon();
      await props.onDaemonChange();
    });
  };
  const restart = async () => {
    if (!(await confirm({ title: t("dash.confirmRestartTitle"), message: t("dash.confirmRestart"), confirmLabel: t("dash.restart") })))
      return;
    await action.run(async () => {
      await api.stopDaemon();
      await api.startDaemon();
      await props.onDaemonChange();
    });
  };
  return (
    <Card
      title={t("dash.runtime")}
      actions={
        <Badge tone={s.gateway.listening ? "ok" : "warn"} dot>
          {s.gateway.listening ? t("dash.listening") : t("dash.notListening")}
        </Badge>
      }
    >
      <dl className="kv">
        <dt>{t("settings.address")}</dt>
        <dd className="mono">{s.gateway.addr}</dd>
        <dt>{t("dash.pid")}</dt>
        <dd className="mono">{s.pid}</dd>
        <dt>{t("settings.version")}</dt>
        <dd>
          v{s.version} <span className="muted small">{t("dash.schema", { n: s.schema_version })}</span>
        </dd>
        <dt>{t("dash.uptime")}</dt>
        <dd>{duration(Date.now() - s.started_at_ms)}</dd>
        <dt>{t("settings.dataDir")}</dt>
        <dd className="mono small" title={s.data_dir}>
          {s.data_dir}
        </dd>
      </dl>
      <ErrorText error={action.error} />
      <div className="card-foot">
        <Button kind="danger" icon={<CircleStop size={16} />} disabled={action.busy} onClick={() => void stop()}>
          {t("settings.stop")}
        </Button>
        <Button icon={<RotateCw size={16} />} disabled={action.busy} onClick={() => void restart()}>
          {action.busy ? t("dash.restarting") : t("dash.restart")}
        </Button>
        <Button icon={<RefreshCw size={16} />} disabled={action.busy} onClick={() => void props.onDaemonChange()}>
          {t("dash.refresh")}
        </Button>
      </div>
    </Card>
  );
}

function EndpointsCard(props: { addr: string }) {
  const base = `http://${props.addr}`;
  const rows = [
    { name: t("dash.epOpenai"), proto: "/v1/chat/completions", url: `${base}/v1` },
    { name: t("dash.epResponses"), proto: "/v1/responses", url: `${base}/v1` },
    { name: t("dash.epAnthropic"), proto: "/v1/messages", url: base },
  ];
  return (
    <Card title={t("dash.endpoints")} subtitle={t("dash.endpointsHint")}>
      {rows.map((r) => (
        <div className="endpoint" key={r.proto}>
          <div>
            <div className="endpoint-name">{r.name}</div>
            <div className="endpoint-proto mono">{r.proto}</div>
          </div>
          <CopyField value={r.url} />
        </div>
      ))}
      <div className="small muted" style={{ marginTop: 4 }}>
        {t("dash.keyHint")} <a href="#/clients">{t("dash.goClients")}</a>
      </div>
    </Card>
  );
}
