// SPDX-License-Identifier: Apache-2.0

import { useState } from "react";
import { api } from "../api";
import { Card, Empty, ErrorText, Page } from "../components";
import { ms, num, usd } from "../format";
import { usePoll } from "../hooks";
import { type MessageKey, t } from "../i18n";

const GROUPS: { id: string; label: MessageKey }[] = [
  { id: "model", label: "stats.group.model" },
  { id: "route", label: "stats.group.route" },
  { id: "provider", label: "stats.group.provider" },
  { id: "client", label: "stats.group.client" },
  { id: "day", label: "stats.group.day" },
];

export function Stats() {
  const [group, setGroup] = useState("model");
  const [days, setDays] = useState(7);
  const stats = usePoll(() => api.stats(Date.now() - days * 86400000, group), 10000, [group, days]);
  const rows = stats.data ?? [];
  const total = rows.reduce(
    (sum, r) => ({
      requests: sum.requests + r.requests,
      errors: sum.errors + r.errors,
      input: sum.input + r.input_tokens,
      output: sum.output + r.output_tokens,
      cost: sum.cost + r.cost_usd,
    }),
    { requests: 0, errors: 0, input: 0, output: 0, cost: 0 },
  );
  const maxRequests = Math.max(1, ...rows.map((r) => r.requests));
  const groupLabel = GROUPS.find((g) => g.id === group)?.label ?? "stats.group.model";

  return (
    <Page
      title={t("stats.title")}
      actions={
        <select value={days} onChange={(e) => setDays(Number(e.target.value))}>
          {[1, 7, 30, 90, 365].map((d) => (
            <option key={d} value={d}>
              {d === 1 ? t("stats.last24h") : t("stats.lastDays", { n: d })}
            </option>
          ))}
        </select>
      }
    >
      <div className="grid cols-4">
        <Card>
          <div className="stat">
            <div className="label">{t("stats.requests")}</div>
            <div className="value">{num(total.requests)}</div>
          </div>
        </Card>
        <Card>
          <div className="stat">
            <div className="label">{t("stats.failed")}</div>
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
            <div className="label">{t("stats.tokens")}</div>
            <div className="value">
              {num(total.input)} / {num(total.output)}
            </div>
          </div>
        </Card>
        <Card>
          <div className="stat">
            <div className="label">{t("stats.cost")}</div>
            <div className="value">{usd(total.cost)}</div>
          </div>
        </Card>
      </div>
      <div className="tabs">
        {GROUPS.map((g) => (
          <button key={g.id} className={group === g.id ? "active" : ""} onClick={() => setGroup(g.id)}>
            {t("stats.by", { group: t(g.label) })}
          </button>
        ))}
      </div>
      <ErrorText error={stats.error?.message} />
      <Card>
        {rows.length === 0 && !stats.loading && <Empty>{t("stats.empty")}</Empty>}
        {rows.length > 0 && (
          <table>
            <thead>
              <tr>
                <th>{t(groupLabel)}</th>
                <th style={{ width: "22%" }} />
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
      <p className="small muted">{t("stats.note")}</p>
    </Page>
  );
}
