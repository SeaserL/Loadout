// SPDX-License-Identifier: Apache-2.0

import { useState } from "react";
import { api, type StatsEntry } from "../api";
import { type StackedPoint, StackedBars } from "../charts";
import { Empty, ErrorText, Kpi, Page, Rolling, Segmented, ShareBar } from "../components";
import { count, dayStart, localDay, ms, num, tokens, usd } from "../format";
import { UsageHeatmap } from "../Heatmap";
import { DayBrief } from "./DayBrief";
import { usePoll } from "../hooks";
import { type MessageKey, t } from "../i18n";

const GROUPS: { id: string; label: MessageKey }[] = [
  { id: "model", label: "stats.group.model" },
  { id: "route", label: "stats.group.route" },
  { id: "provider", label: "stats.group.provider" },
  { id: "client", label: "stats.group.client" },
  { id: "day", label: "stats.group.day" },
];

const DAY = 86400000;

/** Midnight here, `days - 1` days ago. */
function firstOf(days: number): number {
  const d = new Date();
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() - (days - 1)).getTime();
}

/** One point per local day from `days` ago to today. */
function dailyPoints(rows: StatsEntry[], days: number): StackedPoint[] {
  const byKey = new Map(rows.map((r) => [r.key, r]));
  const points: StackedPoint[] = [];
  const now = new Date();
  const span = Math.max(1, Math.min(days, 366));
  for (let i = span - 1; i >= 0; i--) {
    const key = localDay(new Date(now.getFullYear(), now.getMonth(), now.getDate() - i).getTime());
    const r = byKey.get(key);
    points.push({ label: key.slice(5), title: key, day: key, input: r?.input_tokens ?? 0, output: r?.output_tokens ?? 0 });
  }
  return points;
}

function sum(rows: StatsEntry[]) {
  const total = {
    requests: 0,
    errors: 0,
    input: 0,
    output: 0,
    cost: 0,
    ttftWeighted: 0,
    ttftCount: 0,
    cacheRead: 0,
    cacheWrite: 0,
    cacheSavings: 0,
  };
  for (const r of rows) {
    total.cacheRead += r.cache_read_tokens ?? 0;
    total.cacheWrite += r.cache_write_tokens ?? 0;
    total.cacheSavings += r.cache_savings_usd ?? 0;
    total.requests += r.requests;
    total.errors += r.errors;
    total.input += r.input_tokens;
    total.output += r.output_tokens;
    total.cost += r.cost_usd;
    if (r.avg_ttft_ms != null) {
      total.ttftWeighted += r.avg_ttft_ms * r.requests;
      total.ttftCount += r.requests;
    }
  }
  return total;
}

export function Stats(props: { day?: string }) {
  const [group, setGroup] = useState("model");
  const [days, setDays] = useState(7);
  const [picked, setPicked] = useState(props.day);
  const since = () => Date.now() - days * DAY;
  const stats = usePoll(() => api.stats(since(), group), 10000, [group, days]);
  const daily = usePoll(() => api.stats(firstOf(days === 1 ? 2 : days), "day"), 30000, [days]);
  const year = usePoll(() => {
    const now = new Date();
    return api.stats(new Date(now.getFullYear(), now.getMonth() - 11, 1).getTime(), "day");
  }, 60000);
  const usage = usePoll(() => api.usage(false), 30000);
  const rows = stats.data ?? [];
  const total = sum(rows);
  const maxRequests = Math.max(1, ...rows.map((r) => r.requests));
  const groupLabel = GROUPS.find((g) => g.id === group)?.label ?? "stats.group.model";
  const tokenTotal = total.input + total.output;
  const points = dailyPoints(daily.data ?? [], days === 1 ? 2 : days);
  const successRate = total.requests > 0 ? ((total.requests - total.errors) / total.requests) * 100 : null;

  return (
    <Page
      title={t("stats.title")}
      help="stats"
      actions={
        <select value={days} aria-label={t("stats.period")} onChange={(e) => setDays(Number(e.target.value))}>
          {[1, 7, 30, 90, 365].map((d) => (
            <option key={d} value={d}>
              {d === 1 ? t("stats.last24h") : t("stats.lastDays", { n: d })}
            </option>
          ))}
        </select>
      }
    >
      <div className="balances">
        {(usage.data ?? [])
          .filter((u) => u.balances.length > 0)
          .flatMap((u) =>
            u.balances.map((b) => (
              <div className="balance-card" key={`${u.provider}/${u.alias}/${b.currency}`}>
                <div className="strong ellipsis">
                  {u.provider}
                  <span className="small muted"> {u.alias}</span>
                </div>
                <div className="row">
                  <span className="small muted">{t("acc.balance")}</span>
                  <span className="spacer" />
                  <span className="balance-value">
                    {b.currency} {b.amount.toFixed(2)}
                  </span>
                </div>
              </div>
            )),
          )}
      </div>
      <div className="kpis">
        <Kpi label={t("stats.requests")} value={<Rolling value={total.requests} format={count} />} />
        <Kpi
          label={t("stats.successRate")}
          value={successRate == null ? "-" : <Rolling value={successRate} format={(v) => `${v.toFixed(1)}%`} />}
          sub={total.errors > 0 ? t("stats.failedN", { n: num(total.errors) }) : undefined}
          tone={successRate != null && successRate < 95 ? "warn" : undefined}
        />
        <Kpi label={t("stats.input")} value={<Rolling value={total.input} format={tokens} />} />
        <Kpi label={t("stats.output")} value={<Rolling value={total.output} format={tokens} />} />
        <Kpi label={t("stats.cost")} value={<Rolling value={total.cost} format={usd} />} />
        <Kpi
          label={t("stats.avgTtft")}
          value={total.ttftCount > 0 ? <Rolling value={total.ttftWeighted / total.ttftCount} format={ms} /> : "-"}
        />
      </div>

      <ErrorText error={stats.error?.message ?? daily.error?.message} />

      <div className="stats-layout">
        <section>
          <header className="section-head">
            <h2>{t("stats.trend")}</h2>
            <div className="legend">
              <span>
                <i className="input" />
                {t("stats.input")}
              </span>
              <span>
                <i className="output" />
                {t("stats.output")}
              </span>
            </div>
          </header>
          <StackedBars points={points} picked={picked} onPick={setPicked} />
        </section>
        <section>
          <header className="section-head">
            <h2>{t("stats.composition")}</h2>
          </header>
          <div className="composition">
            {[
              { label: t("stats.input"), value: total.input, tone: "input" },
              { label: t("stats.output"), value: total.output, tone: "output" },
            ].map((c) => (
              <div className="composition-row" key={c.tone}>
                <div className="row">
                  <span className="strong">{c.label}</span>
                  <span className="spacer" />
                  <span className="num">{c.value.toLocaleString()}</span>
                  <span className="muted num" style={{ minWidth: 48, textAlign: "right" }}>
                    {tokenTotal > 0 ? `${((c.value / tokenTotal) * 100).toFixed(1)}%` : "-"}
                  </span>
                </div>
                <ShareBar value={c.value} max={tokenTotal} tone={c.tone} />
              </div>
            ))}
            {[
              { label: t("stats.cacheRead"), value: total.cacheRead, key: "cache-read" },
              { label: t("stats.cacheWrite"), value: total.cacheWrite, key: "cache-write" },
            ].map((c) => (
              <div className="composition-row" key={c.key}>
                <div className="row">
                  <span>{c.label}</span>
                  <span className="spacer" />
                  <span className="num">{c.value.toLocaleString()}</span>
                  <span className="muted num" style={{ minWidth: 48, textAlign: "right" }}>
                    {total.input > 0 ? `${((c.value / total.input) * 100).toFixed(1)}%` : "-"}
                  </span>
                </div>
                <ShareBar value={c.value} max={total.input} tone="input" />
              </div>
            ))}
            <div className="row">
              <span className="strong">{t("stats.cacheHitRate")}</span>
              <span className="spacer" />
              <span className="num">
                {total.input > 0 ? `${Math.min(100, (total.cacheRead / total.input) * 100).toFixed(1)}%` : "-"}
              </span>
            </div>
            <div className="row">
              <span className="strong">{t("stats.cacheSavings")}</span>
              <span className="spacer" />
              <span className="num">{usd(total.cacheSavings)}</span>
            </div>
          </div>
        </section>
      </div>

      <UsageHeatmap rows={year.data} picked={picked} onPick={setPicked} />

      <section>
        <header className="section-head">
          <Segmented
            label={t("stats.groupBy")}
            value={group}
            onChange={setGroup}
            options={GROUPS.map((g) => ({ value: g.id, label: t(g.label) }))}
          />
        </header>
        {rows.length === 0 && !stats.loading && <Empty>{t("stats.empty")}</Empty>}
        {rows.length > 0 && (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>{t(groupLabel)}</th>
                  <th style={{ width: "18%" }} />
                  <th className="num">{t("stats.requests")}</th>
                  <th className="num">{t("stats.failed")}</th>
                  <th className="num">{t("stats.tokens")}</th>
                  <th className="num">{t("stats.costCol")}</th>
                  <th className="num">{t("stats.avgTtft")}</th>
                  <th className="num">{t("stats.avgTps")}</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => (
                  <tr key={r.key}>
                    <td>{r.key}</td>
                    <td>
                      <ShareBar value={r.requests} max={maxRequests} />
                    </td>
                    <td className="num">{num(r.requests)}</td>
                    <td className="num" style={r.errors > 0 ? { color: "var(--bad)" } : undefined}>
                      {r.errors > 0 ? num(r.errors) : "-"}
                    </td>
                    <td className="num">
                      {tokens(r.input_tokens)} / {tokens(r.output_tokens)}
                    </td>
                    <td className="num">{usd(r.cost_usd)}</td>
                    <td className="num">{ms(r.avg_ttft_ms)}</td>
                    <td className="num">{r.avg_tps != null ? r.avg_tps.toFixed(0) : "-"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
      {picked && dayStart(picked) > 0 && <DayBrief day={picked} onPick={setPicked} onClose={() => setPicked(undefined)} />}
    </Page>
  );
}
