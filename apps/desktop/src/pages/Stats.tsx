// SPDX-License-Identifier: Apache-2.0

import { useState } from "react";
import { RefreshCw } from "lucide-react";
import { api, type StatsEntry } from "../api";
import { type StackedPoint, StackedBars } from "../charts";
import {
  Card,
  Empty,
  ErrorText,
  FilterBar,
  FilterField,
  IconButton,
  Kpi,
  Page,
  Segmented,
  ShareBar,
} from "../components";
import { ms, num, tokens, usd } from "../format";
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

/** One point per UTC day from `days` ago to today (the daemon groups by UTC date). */
function dailyPoints(rows: StatsEntry[], days: number): StackedPoint[] {
  const byKey = new Map(rows.map((r) => [r.key, r]));
  const points: StackedPoint[] = [];
  const today = Math.floor(Date.now() / DAY) * DAY;
  const count = Math.max(1, Math.min(days, 366));
  for (let i = count - 1; i >= 0; i--) {
    const key = new Date(today - i * DAY).toISOString().slice(0, 10);
    const r = byKey.get(key);
    points.push({ label: key.slice(5), title: key, input: r?.input_tokens ?? 0, output: r?.output_tokens ?? 0 });
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

export function Stats() {
  const [group, setGroup] = useState("model");
  const [days, setDays] = useState(7);
  const since = () => Date.now() - days * DAY;
  const stats = usePoll(() => api.stats(since(), group), 10000, [group, days]);
  const daily = usePoll(() => api.stats(since(), "day"), 30000, [days]);
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
      actions={
        <IconButton
          label={t("common.refresh")}
          onClick={() => void Promise.all([stats.reload(), daily.reload()])}
        >
          <RefreshCw size={16} />
        </IconButton>
      }
    >
      <FilterBar>
        <FilterField label={t("stats.period")}>
          <select value={days} onChange={(e) => setDays(Number(e.target.value))}>
            {[1, 7, 30, 90, 365].map((d) => (
              <option key={d} value={d}>
                {d === 1 ? t("stats.last24h") : t("stats.lastDays", { n: d })}
              </option>
            ))}
          </select>
        </FilterField>
        <FilterField label={t("stats.groupBy")}>
          <Segmented
            label={t("stats.groupBy")}
            value={group}
            onChange={setGroup}
            options={GROUPS.map((g) => ({ value: g.id, label: t(g.label) }))}
          />
        </FilterField>
      </FilterBar>

      <div className="kpis">
        <Kpi label={t("stats.requests")} value={num(total.requests)} />
        <Kpi
          label={t("stats.successRate")}
          value={successRate == null ? "-" : `${successRate.toFixed(1)}%`}
          sub={total.errors > 0 ? t("stats.failedN", { n: num(total.errors) }) : undefined}
          tone={successRate != null && successRate < 95 ? "warn" : undefined}
        />
        <Kpi label={t("stats.input")} value={tokens(total.input)} />
        <Kpi label={t("stats.output")} value={tokens(total.output)} />
        <Kpi label={t("stats.cost")} value={usd(total.cost)} />
        <Kpi
          label={t("stats.avgTtft")}
          value={total.ttftCount > 0 ? ms(total.ttftWeighted / total.ttftCount) : "-"}
        />
      </div>

      <ErrorText error={stats.error?.message ?? daily.error?.message} />

      <div className="stats-layout">
        <Card
          title={t("stats.trend")}
          subtitle={t("stats.trendHint")}
          actions={
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
          }
        >
          <StackedBars points={points} />
        </Card>
        <Card title={t("stats.composition")} subtitle={t("stats.compositionHint")}>
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
            <div className="small muted">{t("stats.cacheTrackedNote")}</div>
          </div>
        </Card>
      </div>

      <Card title={t("stats.by", { group: t(groupLabel) })}>
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
                    <td className="mono">{r.key}</td>
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
      </Card>
      <p className="small muted">{t("stats.note")}</p>
    </Page>
  );
}
