// SPDX-License-Identifier: Apache-2.0
//
// One day at a glance: what it cost, where the tokens went, when it was busy. It stays open beside
// the charts, so picking another day just changes what it shows.

import { useEffect, useMemo, useRef, useState } from "react";
import { ChevronLeft, ChevronRight, X } from "lucide-react";
import { api, type StatsEntry } from "../api";
import { IconButton, Rolling, Segmented } from "../components";
import { count, dayEnd, dayStart, dayTitle, localDay, tokens, usd } from "../format";
import { usePoll } from "../hooks";
import { type MessageKey, t } from "../i18n";

type By = "model" | "account" | "client";
const BY: { id: By; label: MessageKey }[] = [
  { id: "model", label: "brief.model" },
  { id: "account", label: "brief.account" },
  { id: "client", label: "brief.client" },
];
const MAX_SLICES = 6;
const R = 58;
const GAP = 0.7;

type Slice = { name: string; value: number; hue: number; share: number };

const worth = (e: StatsEntry) => e.input_tokens + e.output_tokens;

/** The largest few by name, the rest as one slice. Falls back to requests on a day without tokens. */
function slicesOf(rows: StatsEntry[]): Slice[] {
  const value = rows.some((r) => worth(r) > 0) ? worth : (r: StatsEntry) => r.requests;
  const sorted = rows.filter((r) => value(r) > 0).sort((a, b) => value(b) - value(a));
  const total = sorted.reduce((s, r) => s + value(r), 0);
  const top = sorted.length > MAX_SLICES ? sorted.slice(0, MAX_SLICES - 1) : sorted;
  const out = top.map((r, i) => ({
    name: r.key || t("flow.unrecorded"),
    value: value(r),
    hue: i + 1,
    share: total > 0 ? value(r) / total : 0,
  }));
  if (sorted.length > top.length) {
    const rest = sorted.slice(top.length).reduce((s, r) => s + value(r), 0);
    out.push({ name: t("brief.other"), value: rest, hue: 0, share: total > 0 ? rest / total : 0 });
  }
  return out;
}

/** Arcs open from the top when the ring appears or its slices change. */
function Ring(props: { slices: Slice[]; total: number; focus: number | undefined; onFocus: (i: number | undefined) => void }) {
  const [open, setOpen] = useState(false);
  const shape = props.slices.map((s) => `${s.name}:${s.share.toFixed(4)}`).join("|");
  useEffect(() => {
    setOpen(false);
    const id = requestAnimationFrame(() => requestAnimationFrame(() => setOpen(true)));
    return () => cancelAnimationFrame(id);
  }, [shape]);
  const picked = props.focus == null ? undefined : props.slices[props.focus];
  let start = 0;
  return (
    <div className="brief-ring">
      <svg viewBox="0 0 150 150" role="img" aria-label={t("brief.share")}>
        <circle className="ring-track" cx="75" cy="75" r={R} pathLength={100} />
        {props.slices.map((s, i) => {
          const length = Math.max(0, s.share * 100 - (props.slices.length > 1 ? GAP : 0));
          const offset = start;
          start += s.share * 100;
          return (
            <circle
              key={s.name}
              className={`ring-arc ${props.focus != null && props.focus !== i ? "dim" : ""} ${props.focus === i ? "on" : ""}`}
              cx="75"
              cy="75"
              r={R}
              pathLength={100}
              style={{
                stroke: s.hue ? `var(--flow-${s.hue})` : "var(--faint)",
                strokeDasharray: open ? `${length} ${100 - length}` : "0 100",
                strokeDashoffset: open ? -offset : 0,
              }}
              onMouseEnter={() => props.onFocus(i)}
              onMouseLeave={() => props.onFocus(undefined)}
            />
          );
        })}
      </svg>
      <div className="ring-center">
        <strong>
          <Rolling value={picked ? picked.value : props.total} format={tokens} />
        </strong>
        <span className="small muted ellipsis">{picked ? picked.name : t("dash.metricTokens")}</span>
      </div>
    </div>
  );
}

export function DayBrief(props: { day: string; onPick: (day: string) => void; onClose: () => void }) {
  const { day } = props;
  const [by, setBy] = useState<By>("model");
  const [focus, setFocus] = useState<number>();
  const box = useRef<HTMLElement>(null);
  const today = localDay(Date.now());
  const brief = usePoll(
    async () => {
      const since = dayStart(day);
      const until = dayEnd(day);
      const [model, account, client, hour] = await Promise.all([
        api.stats(since, "model", until),
        api.stats(since, "account", until),
        api.stats(since, "client", until),
        api.stats(since, "hour", until),
      ]);
      return { day, model, account, client, hour };
    },
    day === today ? 15000 : 0,
    [day],
  );
  // What is on screen stays until the new day has arrived, so switching days does not flash empty.
  const data = brief.data;
  const rows = data?.[by] ?? [];
  const slices = useMemo(() => slicesOf(rows), [rows]);
  const totals = useMemo(() => {
    const out = { requests: 0, errors: 0, tokens: 0, cost: 0 };
    for (const r of data?.model ?? []) {
      out.requests += r.requests;
      out.errors += r.errors;
      out.tokens += worth(r);
      out.cost += r.cost_usd;
    }
    return out;
  }, [data]);
  const hours = useMemo(() => {
    const map = new Map((data?.hour ?? []).map((h) => [Number(h.key), h]));
    return Array.from({ length: 24 }, (_, h) => {
      const e = map.get(h);
      return { hour: h, tokens: e ? worth(e) : 0, requests: e?.requests ?? 0 };
    });
  }, [data]);
  const peak = Math.max(1, ...hours.map((h) => h.tokens || h.requests));
  const busiest = hours.reduce((a, b) => ((b.tokens || b.requests) > (a.tokens || a.requests) ? b : a), hours[0] as (typeof hours)[number]);
  const move = (delta: number) => {
    const d = new Date(dayStart(day));
    d.setDate(d.getDate() + delta);
    const next = localDay(d.getTime());
    if (next <= today) props.onPick(next);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") props.onClose();
      // A select or a segmented control keeps its own arrow keys.
      if ((e.target as Element).closest?.("input, select, textarea, .segmented")) return;
      if (e.key === "ArrowLeft") move(-1);
      if (e.key === "ArrowRight") move(1);
    };
    // A day on a chart changes what is shown; anywhere else closes it.
    const onDown = (e: PointerEvent) => {
      const target = e.target as Element;
      if (!box.current?.contains(target) && !target.closest?.("[data-day]")) props.onClose();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointerdown", onDown);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onDown);
    };
  });

  const rate = totals.requests > 0 ? ((totals.requests - totals.errors) / totals.requests) * 100 : null;

  return (
    <aside className="brief" ref={box} aria-label={dayTitle(day)}>
      <header className="brief-head">
        <h2>{dayTitle(day)}</h2>
        <div className="spacer" />
        <IconButton label={t("brief.prev")} onClick={() => move(-1)}>
          <ChevronLeft size={16} />
        </IconButton>
        <IconButton label={t("brief.next")} disabled={day >= today} onClick={() => move(1)}>
          <ChevronRight size={16} />
        </IconButton>
        <IconButton label={t("common.close")} onClick={props.onClose}>
          <X size={16} />
        </IconButton>
      </header>

      <dl className="brief-figures">
        <div>
          <dt>{t("stats.requests")}</dt>
          <dd>
            <Rolling value={totals.requests} format={count} />
          </dd>
        </div>
        <div>
          <dt>{t("stats.successRate")}</dt>
          <dd className={rate != null && rate < 95 ? "warn-text" : ""}>
            {rate == null ? "-" : <Rolling value={rate} format={(v) => `${v.toFixed(1)}%`} />}
          </dd>
        </div>
        <div>
          <dt>{t("stats.cost")}</dt>
          <dd>
            <Rolling value={totals.cost} format={usd} />
          </dd>
        </div>
      </dl>

      {data && totals.requests === 0 && <div className="calm muted">{t("brief.empty")}</div>}
      {totals.requests > 0 && (
        <>
          <Segmented
            label={t("brief.share")}
            value={by}
            onChange={(v) => {
              setBy(v);
              setFocus(undefined);
            }}
            options={BY.map((b) => ({ value: b.id, label: t(b.label) }))}
          />
          <Ring slices={slices} total={slices.reduce((s, x) => s + x.value, 0)} focus={focus} onFocus={setFocus} />
          <ul className="brief-legend">
            {slices.map((s, i) => (
              <li
                key={s.name}
                className={focus != null && focus !== i ? "dim" : ""}
                onMouseEnter={() => setFocus(i)}
                onMouseLeave={() => setFocus(undefined)}
              >
                <i style={{ background: s.hue ? `var(--flow-${s.hue})` : "var(--faint)" }} />
                <span className="ellipsis" title={s.name}>
                  {s.name}
                </span>
                <span className="num muted">{tokens(s.value)}</span>
                <span className="num">{s.share < 0.01 ? "<1" : (s.share * 100).toFixed(0)}%</span>
              </li>
            ))}
          </ul>

          <div className="brief-hours" aria-label={t("brief.hours")}>
            {hours.map((h) => (
              <span
                key={h.hour}
                className={h === busiest && peak > 1 ? "peak" : ""}
                style={{ "--h": `${Math.max(h.tokens || h.requests ? 6 : 2, ((h.tokens || h.requests) / peak) * 100)}%`, "--i": h.hour } as React.CSSProperties}
                title={`${String(h.hour).padStart(2, "0")}:00  ${tokens(h.tokens)}  ${t("flow.requests", { n: h.requests })}`}
              />
            ))}
          </div>
          <div className="brief-hour-axis small muted">
            <span>00</span>
            <span>06</span>
            <span>12</span>
            <span>18</span>
            <span>24</span>
          </div>
        </>
      )}
    </aside>
  );
}
