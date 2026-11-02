// SPDX-License-Identifier: Apache-2.0
//
// A year of use, one square per day, each month its own block. Every square opens that day's brief.

import { type CSSProperties, useMemo, useState } from "react";
import type { StatsEntry } from "./api";
import { HelpLink, Rolling, Segmented } from "./components";
import { dayStart, dayTitle, localDay, num, tokens } from "./format";
import { type MessageKey, t } from "./i18n";

type View = "day" | "week" | "total";
const VIEWS: { id: View; label: MessageKey }[] = [
  { id: "day", label: "heat.day" },
  { id: "week", label: "heat.week" },
  { id: "total", label: "heat.total" },
];
const MONTHS_SHOWN = 12;

type Cell = { day: string; tokens: number; requests: number; value: number; level: number; future: boolean };

/** Monday-based weekday: 0 is Monday. */
const weekday = (d: Date) => (d.getDay() + 6) % 7;

function buildYear(rows: StatsEntry[], view: View) {
  const today = localDay(Date.now());
  const byDay = new Map(rows.map((r) => [r.key, r]));
  const now = new Date();
  const months: { key: string; label: string; offset: number; days: Cell[] }[] = [];
  let running = 0;
  const flat: Cell[] = [];
  for (let m = MONTHS_SHOWN - 1; m >= 0; m--) {
    const first = new Date(now.getFullYear(), now.getMonth() - m, 1);
    const days: Cell[] = [];
    for (let d = new Date(first); d.getMonth() === first.getMonth(); d.setDate(d.getDate() + 1)) {
      const day = localDay(d.getTime());
      const e = byDay.get(day);
      const used = e ? e.input_tokens + e.output_tokens : 0;
      running += used;
      const cell = { day, tokens: used, requests: e?.requests ?? 0, value: used, level: 0, future: day > today };
      if (view === "total") cell.value = cell.future ? 0 : running;
      days.push(cell);
      flat.push(cell);
    }
    months.push({
      key: `${first.getFullYear()}-${first.getMonth()}`,
      label: first.toLocaleDateString(undefined, { month: "short" }),
      offset: weekday(first),
      days,
    });
  }
  if (view === "week") {
    // Every day of a week shows that week's total, so the rhythm of the year reads as columns.
    let bucket: Cell[] = [];
    const flush = () => {
      const sum = bucket.reduce((s, c) => s + c.tokens, 0);
      for (const c of bucket) c.value = c.future ? 0 : sum;
      bucket = [];
    };
    for (const c of flat) {
      bucket.push(c);
      if (weekday(new Date(dayStart(c.day))) === 6) flush();
    }
    flush();
  }
  const max = Math.max(1, ...flat.map((c) => c.value));
  for (const c of flat) {
    if (c.value <= 0) continue;
    // The square root keeps a quiet day visible beside a very busy one.
    const p = view === "total" ? c.value / max : Math.sqrt(c.value / max);
    c.level = Math.max(1, Math.ceil(p * 4));
  }
  return { months, flat, today };
}

function streaks(flat: Cell[], today: string) {
  let longest = 0;
  let run = 0;
  let current = 0;
  for (const c of flat) {
    if (c.future) break;
    if (c.requests > 0) {
      run++;
      longest = Math.max(longest, run);
    } else if (c.day !== today) {
      // Today does not break a streak just because nothing has been sent yet.
      run = 0;
    }
    current = run;
  }
  return { longest, current };
}

/** `rows` is undefined until the year has loaded; the blocks arrive with their data, not before it. */
export function UsageHeatmap(props: { rows: StatsEntry[] | undefined; picked?: string; onPick: (day: string) => void }) {
  const [view, setView] = useState<View>("day");
  const [tip, setTip] = useState<{ cell: Cell; x: number; y: number }>();
  const { months, flat, today } = useMemo(() => buildYear(props.rows ?? [], view), [props.rows, view]);
  const total = flat.reduce((s, c) => s + c.tokens, 0);
  const peak = flat.reduce((s, c) => Math.max(s, c.tokens), 0);
  const { longest, current } = useMemo(() => streaks(flat, today), [flat, today]);

  return (
    <section className="heat">
      <header className="section-head">
        <h2>
          {t("heat.title")} <HelpLink section="stats" />
        </h2>
        <Segmented label={t("heat.title")} value={view} onChange={setView} options={VIEWS.map((v) => ({ value: v.id, label: t(v.label) }))} />
      </header>
      <dl className="heat-figures">
        <div>
          <dd>
            <Rolling value={total} format={tokens} />
          </dd>
          <dt>{t("heat.totalTokens")}</dt>
        </div>
        <div>
          <dd>
            <Rolling value={peak} format={tokens} />
          </dd>
          <dt>{t("heat.peak")}</dt>
        </div>
        <div>
          <dd>{t("heat.days", { n: longest })}</dd>
          <dt>{t("heat.longest")}</dt>
        </div>
        <div>
          <dd>{t("heat.days", { n: current })}</dd>
          <dt>{t("heat.current")}</dt>
        </div>
      </dl>
      <div className="heat-sizer" onMouseLeave={() => setTip(undefined)}>
        <div className={`heat-months ${props.rows ? "ready" : ""}`}>
          {months.map((month, order) => (
            <div className="heat-month" key={month.key} style={{ "--m": order } as CSSProperties}>
              <div className="heat-grid">
                {Array.from({ length: month.offset }, (_, i) => (
                  <i key={`pad${i}`} className="heat-pad" />
                ))}
                {month.days.map((c) => (
                  <button
                    key={c.day}
                    type="button"
                    data-day={c.day}
                    disabled={c.future}
                    className={`heat-cell l${c.level} ${c.future ? "future" : ""} ${c.day === today ? "today" : ""} ${props.picked === c.day ? "picked" : ""}`}
                    aria-label={`${dayTitle(c.day)} ${tokens(c.tokens)}`}
                    onClick={() => props.onPick(c.day)}
                    onMouseEnter={(e) => {
                      const box = e.currentTarget.closest(".heat-sizer")?.getBoundingClientRect();
                      const r = e.currentTarget.getBoundingClientRect();
                      if (box) setTip({ cell: c, x: r.left - box.left + r.width / 2, y: r.top - box.top });
                    }}
                  />
                ))}
              </div>
              <span className="heat-label">{month.label}</span>
            </div>
          ))}
        </div>
        {tip && (
          <div className="heat-tip" style={{ left: tip.x, top: tip.y }}>
            <strong>{dayTitle(tip.cell.day)}</strong>
            <span>
              {tokens(tip.cell.tokens)} {t("dash.metricTokens")}
            </span>
            <span>{t("flow.requests", { n: num(tip.cell.requests) })}</span>
          </div>
        )}
      </div>
      <div className="heat-scale small muted" aria-hidden>
        {t("heat.less")}
        {[0, 1, 2, 3, 4].map((l) => (
          <i key={l} className={`heat-cell static l${l}`} />
        ))}
        {t("heat.more")}
      </div>
    </section>
  );
}
