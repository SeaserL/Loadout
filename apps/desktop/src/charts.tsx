// SPDX-License-Identifier: Apache-2.0
//
// Small hand-written SVG charts (ExecPlan 009: no chart library).

import { useWidth } from "./hooks";
import { tokens } from "./format";
import { t } from "./i18n";

export interface StackedPoint {
  /** Axis label, e.g. "10-04". */
  label: string;
  /** Tooltip heading, e.g. the full date. */
  title: string;
  input: number;
  output: number;
}

/** Rounds `n` up to 1, 2, 2.5 or 5 × a power of ten, for axis ticks. */
function niceMax(n: number): number {
  if (n <= 0) return 1;
  const p = 10 ** Math.floor(Math.log10(n));
  for (const m of [1, 2, 2.5, 5, 10]) {
    if (m * p >= n) return m * p;
  }
  return 10 * p;
}

/** Axis ticks are round numbers: "500k", "1.5M" rather than "500.0k", "1.50M". */
function axisLabel(v: number): string {
  return tokens(v).replace(/\.0+(?=[kMB])/, "").replace(/(\.\d*?)0+(?=[kMB])/, "$1");
}

const HEIGHT = 240;
const PAD_LEFT = 48;
const PAD_BOTTOM = 24;
const PAD_TOP = 8;

/** Stacked input/output token bars, one per point. */
export function StackedBars(props: { points: StackedPoint[] }) {
  const [ref, width] = useWidth<HTMLDivElement>();
  const n = Math.max(1, props.points.length);
  const max = niceMax(Math.max(0, ...props.points.map((p) => p.input + p.output)));
  const plotW = Math.max(0, width - PAD_LEFT);
  const plotH = HEIGHT - PAD_BOTTOM - PAD_TOP;
  const slot = plotW / n;
  const barW = Math.max(2, Math.min(36, slot * 0.68));
  const y = (v: number) => PAD_TOP + plotH - (v / max) * plotH;
  const labelEvery = Math.max(1, Math.ceil(n / Math.max(1, Math.floor(plotW / 64))));
  const ticks = [0, 0.25, 0.5, 0.75, 1].map((f) => f * max);

  return (
    <div ref={ref}>
      {width > 0 && (
        <svg className="chart" width={width} height={HEIGHT} role="img" aria-label={t("stats.trend")}>
          {ticks.map((v) => (
            <g key={v}>
              <line className="grid-line" x1={PAD_LEFT} x2={width} y1={y(v)} y2={y(v)} />
              <text x={PAD_LEFT - 8} y={y(v) + 4} textAnchor="end">
                {axisLabel(v)}
              </text>
            </g>
          ))}
          {props.points.map((p, i) => {
            const cx = PAD_LEFT + slot * i + slot / 2;
            const x = cx - barW / 2;
            const inTop = y(p.input);
            const outTop = y(p.input + p.output);
            return (
              <g key={p.title}>
                <rect className="hit" x={PAD_LEFT + slot * i} y={PAD_TOP} width={slot} height={plotH} rx={4} />
                {p.input > 0 && (
                  <rect className="bar-input" x={x} y={inTop} width={barW} height={y(0) - inTop} rx={2} />
                )}
                {p.output > 0 && (
                  <rect className="bar-output" x={x} y={outTop} width={barW} height={inTop - outTop} rx={2} />
                )}
                {i % labelEvery === 0 && (
                  <text x={cx} y={HEIGHT - 6} textAnchor="middle">
                    {p.label}
                  </text>
                )}
                <title>
                  {`${p.title}\n${t("stats.input")}: ${p.input.toLocaleString()}\n${t("stats.output")}: ${p.output.toLocaleString()}`}
                </title>
              </g>
            );
          })}
        </svg>
      )}
    </div>
  );
}
