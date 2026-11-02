// SPDX-License-Identifier: Apache-2.0
//
// Token flow, upstream on the left and downstream on the right. The strands are the request log;
// what moves on them is requests moving right now, as fast as bytes actually reach the client.
// Nothing moves unless a real request does: an idle gateway is a still picture.
//
// Click an end to follow it; drag it to place it. The order is a display preference of this window.

import {
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type RefObject,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { GripVertical, Plus, RotateCcw } from "lucide-react";
import type { FlowLink, LiveDone, LiveRequest } from "./api";
import { HubMark } from "./components";
import { num, tokens as fmtTokens, usd } from "./format";
import { t } from "./i18n";
import { motionAllowed, useMotion } from "./theme";

export type FlowMetric = "tokens" | "requests";
type Side = "left" | "right";
type Node = { id: string; label: string; sub?: string; value: number; hue: number | null };
type Strand = { x1: number; y1: number; x2: number; y2: number };
type Drawn = { key: string; s: Strand; side: "up" | "down"; node: string; nth: number; of: number; hue: number | null };
type Order = Record<Side, string[]>;

const WIDTH = 1000;
const ROW = 58;
const MAX_NODES = 6;
const LABEL = 190;
const HUB_X = WIDTH / 2;
const HUB_R = 31;
const MAX_STRANDS = 30;
const HUES = 6;
const OTHER = "\0other";
const UNRECORDED = "\0none";
const ORDER_KEY = "loadout.flow.order";
export const PROBE_CLIENT = "Loadout 测速";

const worth = (l: FlowLink, metric: FlowMetric) =>
  metric === "requests" ? l.requests : l.input_tokens + l.output_tokens;
const upstreamId = (r: { provider: string; account: string }) => `${r.provider}/${r.account}`;
const downstreamId = (r: { client: string }) => r.client || UNRECORDED;
const hueVar = (hue: number | null) => (hue == null ? "var(--faint)" : `var(--flow-${hue})`);

function loadOrder(): Order {
  try {
    const v = JSON.parse(window.localStorage.getItem(ORDER_KEY) ?? "{}") as Partial<Order>;
    return { left: Array.isArray(v.left) ? v.left : [], right: Array.isArray(v.right) ? v.right : [] };
  } catch {
    return { left: [], right: [] };
  }
}

function saveOrder(order: Order) {
  try {
    window.localStorage.setItem(ORDER_KEY, JSON.stringify(order));
  } catch {
    // Not remembered, but still applied for this session.
  }
}

/** The largest ends, and always the ones moving right now: a live comet must land on its own name. */
function collapse(entries: Map<string, Node>, live: Set<string>, gone: (id: string) => boolean = () => false): Node[] {
  const sorted = [...entries.values()].sort((a, b) => b.value - a.value);
  // A client whose key was revoked is history: it counts, but does not get a row of its own.
  const own = sorted.filter((n) => live.has(n.id) || !gone(n.id));
  if (own.length === sorted.length && sorted.length <= MAX_NODES) return sorted;
  const moving = own.filter((n) => live.has(n.id)).slice(0, MAX_NODES - 1);
  const kept = new Set([...moving, ...own.filter((n) => !live.has(n.id)).slice(0, MAX_NODES - 1 - moving.length)]);
  const rest = sorted.filter((n) => !kept.has(n));
  return [
    ...sorted.filter((n) => kept.has(n)),
    { id: OTHER, label: t("flow.others", { n: rest.length }), value: rest.reduce((s, n) => s + n.value, 0), hue: null },
  ];
}

/** Colours follow the name, not the row, so an end keeps its colour wherever it is dragged. */
function paint(nodes: Node[], offset: number): Node[] {
  const names = nodes.filter((n) => n.id !== OTHER).map((n) => n.id).sort();
  return nodes.map((n) => (n.id === OTHER ? n : { ...n, hue: ((names.indexOf(n.id) + offset) % HUES) + 1 }));
}

/** Saved order first, new ends after it by size; "the rest" stays at the bottom. */
function arrange(nodes: Node[], saved: string[]): Node[] {
  const rank = (n: Node) => {
    if (n.id === OTHER) return Number.MAX_SAFE_INTEGER;
    const i = saved.indexOf(n.id);
    return i < 0 ? saved.length : i;
  };
  return [...nodes].sort((a, b) => rank(a) - rank(b) || b.value - a.value);
}

const strandPath = (s: Strand) => {
  const mid = (s.x1 + s.x2) / 2;
  return `M${s.x1},${s.y1} C${mid},${s.y1} ${mid},${s.y2} ${s.x2},${s.y2}`;
};

/** A point on the same curve `strandPath` draws. */
function pointOn(s: Strand, u: number): [number, number] {
  const mid = (s.x1 + s.x2) / 2;
  const v = 1 - u;
  const a = v * v * v;
  const b = 3 * v * v * u;
  const c = 3 * v * u * u;
  const d = u * u * u;
  return [a * s.x1 + b * mid + c * mid + d * s.x2, a * s.y1 + b * s.y1 + c * s.y2 + d * s.y2];
}

/** Bytes per second that reach the client become particles per second. */
const particlesPerSecond = (bytesPerSec: number) => Math.min(26, Math.max(2.5, bytesPerSec / 550));

// ---- the moving layer ------------------------------------------------------------------------

type Rgb = [number, number, number];
type Ink = { hues: Rgb[]; other: Rgb; bad: Rgb; hub: Rgb; dark: boolean };
type Comet = {
  from: Strand;
  to: Strand;
  fromHue: number | null;
  toHue: number | null;
  up: string;
  down: string;
  t: number;
  speed: number;
  size: number;
  wobble: number;
  seed: number;
  hold: number;
  failed: boolean;
  /** The request on its way up, before anything comes back. */
  back: boolean;
};
type Stream = {
  key: string;
  up: string;
  down: string;
  from: Drawn[];
  to: Drawn[];
  rate: number;
  answering: boolean;
  ending: boolean;
  level: number;
  clock: number;
};
type Spark = { x: number; y: number; rgb: Rgb; age: number };

function hexToRgb(hex: string, fallback: Rgb): Rgb {
  const h = hex.trim().replace("#", "");
  const full = h.length === 3 ? h.split("").map((c) => c + c).join("") : h;
  if (!/^[0-9a-f]{6}$/i.test(full)) return fallback;
  const n = Number.parseInt(full, 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function readInk(node: Element): Ink {
  const css = getComputedStyle(node);
  const of = (name: string, fallback: Rgb) => hexToRgb(css.getPropertyValue(name), fallback);
  return {
    hues: Array.from({ length: HUES }, (_, i) => of(`--flow-${i + 1}`, [13, 148, 136])),
    other: of("--faint", [185, 185, 194]),
    bad: of("--bad", [214, 69, 69]),
    hub: of("--comet", [13, 148, 136]),
    dark: document.documentElement.dataset.theme === "dark",
  };
}

const rgb = (c: Rgb) => `rgb(${c[0]},${c[1]},${c[2]})`;
const mix = (a: Rgb, b: Rgb, p: number): Rgb => [
  Math.round(a[0] + (b[0] - a[0]) * p),
  Math.round(a[1] + (b[1] - a[1]) * p),
  Math.round(a[2] + (b[2] - a[2]) * p),
];

/** It only runs while something moves, so an idle dashboard costs nothing. */
function useFlowMotion(
  canvas: RefObject<HTMLCanvasElement | null>,
  hubY: number,
  streamsRef: RefObject<Map<string, Stream>>,
  cometsRef: RefObject<Comet[]>,
  focusRef: RefObject<string | undefined>,
  wake: RefObject<() => void>,
) {
  useEffect(() => {
    const node = canvas.current;
    if (!node) return;
    const ctx = node.getContext("2d");
    if (!ctx) return;
    const paths = new WeakMap<Strand, Path2D>();
    const pathOf = (s: Strand) => {
      let p = paths.get(s);
      if (!p) {
        p = new Path2D(strandPath(s));
        paths.set(s, p);
      }
      return p;
    };
    const sparks: Spark[] = [];
    let frame = 0;
    let last = 0;
    let energy = 0;
    let ticks = 0;
    let ink = readInk(node);

    const fit = () => {
      const dpr = window.devicePixelRatio || 1;
      node.width = Math.round(node.clientWidth * dpr);
      node.height = Math.round(node.clientHeight * dpr);
    };
    fit();
    const observer = new ResizeObserver(fit);
    observer.observe(node);

    const colourOf = (hue: number | null): Rgb => (hue == null ? ink.other : (ink.hues[hue - 1] ?? ink.hub));

    const draw = (now: number) => {
      frame = 0;
      const dt = last ? Math.min(0.05, (now - last) / 1000) : 0.016;
      last = now;
      if (++ticks % 40 === 1) ink = readInk(node);
      const streams = streamsRef.current;
      const comets = cometsRef.current;
      const focus = focusRef.current;
      const k = node.width / WIDTH;
      const dpr = node.width / (node.clientWidth || 1);
      const seen = (up: string, down: string) => (!focus || up === focus || down === focus ? 1 : 0.1);
      ctx.clearRect(0, 0, node.width, node.height);
      ctx.globalCompositeOperation = ink.dark ? "lighter" : "source-over";
      ctx.lineCap = "round";

      // The current: the ribbon itself runs, faster when more comes through, and eases in and out.
      for (const [key, s] of streams) {
        const target = s.ending ? 0 : s.answering ? 1 : 0.38;
        s.level += (target - s.level) * Math.min(1, dt * (s.ending ? 2 : 3));
        if (s.ending && s.level < 0.02) {
          streams.delete(key);
          continue;
        }
        s.clock += dt * (46 + s.rate * 8);
        const weight = seen(s.up, s.down) * s.level;
        ctx.save();
        ctx.scale(k, k);
        ctx.lineWidth = (1.2 * dpr) / k;
        for (const list of [s.from, s.to]) {
          const step = Math.max(1, Math.floor(list.length / 12));
          for (let i = 0; i < list.length; i += step) {
            const d = list[i] as Drawn;
            const seed = (i * 37 + d.nth * 11) % 97;
            ctx.setLineDash([22 + seed % 34, 80 + (seed * 3) % 110]);
            ctx.lineDashOffset = -(s.clock * (0.75 + (seed % 7) * 0.09)) + seed * 7;
            ctx.strokeStyle = rgb(colourOf(d.hue));
            ctx.globalAlpha = weight * (ink.dark ? 0.34 : 0.5);
            ctx.stroke(pathOf(d.s));
          }
        }
        ctx.restore();
        // Arrivals are random, not metered: real traffic comes in clumps.
        if (s.answering && !s.ending && Math.random() < s.rate * dt) {
          const from = s.from[Math.floor(Math.random() * s.from.length)];
          const to = s.to[Math.floor(Math.random() * s.to.length)];
          if (from && to) {
            comets.push({
              from: from.s,
              to: to.s,
              fromHue: from.hue,
              toHue: to.hue,
              up: s.up,
              down: s.down,
              t: 0,
              speed: 0.42 + Math.random() * 0.36,
              size: 0.8 + 2 * Math.random() ** 3,
              wobble: Math.random() * 2.4,
              seed: Math.random() * 6.28,
              hold: 0,
              failed: false,
              back: false,
            });
          }
        }
      }
      ctx.setLineDash([]);

      for (let i = comets.length - 1; i >= 0; i--) {
        const c = comets[i] as Comet;
        // Held inside the gateway for a moment: taken in on one side, sent out on the other.
        if (c.hold > 0) {
          c.hold -= dt;
          continue;
        }
        const before = c.t;
        const along = (Math.max(0, c.t) * 2) % 1;
        c.t += c.speed * dt * (0.68 + 0.64 * Math.sin(Math.PI * along));
        // A burst is queued behind the start line; it has not left yet.
        if (c.t <= 0) continue;
        if (before < 0.5 && c.t >= 0.5) {
          energy = Math.min(1, energy + 0.1 + c.size * 0.05);
          // A failed answer dies at the gateway: it never reaches the client.
          if (c.failed) {
            comets.splice(i, 1);
            continue;
          }
          if (!c.back) {
            c.hold = 0.02 + Math.random() * 0.11;
            continue;
          }
        }
        if (c.t >= 1) {
          if (!c.back) {
            const [x, y] = pointOn(c.to, 1);
            sparks.push({ x, y, rgb: colourOf(c.toHue), age: 0 });
          }
          comets.splice(i, 1);
          continue;
        }
        const at = (v: number): [number, number] => {
          const clamped = Math.max(0, Math.min(1, v));
          const p = c.back ? 1 - clamped : clamped;
          const [x, y] = p < 0.5 ? pointOn(c.from, p * 2) : pointOn(c.to, (p - 0.5) * 2);
          return [x, y + c.wobble * Math.sin(c.seed + p * 15)];
        };
        const colour = (v: number): Rgb => {
          if (c.failed) return ink.bad;
          if (c.back) return ink.other;
          const p = Math.max(0, Math.min(1, (v - 0.32) / 0.36));
          return mix(colourOf(c.fromHue), colourOf(c.toHue), p);
        };
        const fade = Math.min(1, c.t * 9, (1 - c.t) * 9, c.failed ? (0.5 - c.t) * 6 + 0.4 : 1) * seen(c.up, c.down);
        const steps = 8;
        const reach = 0.011 + c.size * 0.006;
        let [px, py] = at(c.t);
        for (let s = 1; s <= steps; s++) {
          const v = c.t - s * reach;
          const [x, y] = at(v);
          // Across the gateway the tail would jump: it is hidden under the hub anyway.
          ctx.globalAlpha = fade * (1 - s / (steps + 1)) ** 2 * 0.8;
          ctx.strokeStyle = rgb(colour(v));
          ctx.lineWidth = c.size * (1 - s / (steps + 2)) * dpr;
          ctx.beginPath();
          ctx.moveTo(px * k, py * k);
          ctx.lineTo(x * k, y * k);
          ctx.stroke();
          px = x;
          py = y;
        }
        const [hx, hy] = at(c.t);
        ctx.fillStyle = rgb(colour(c.t));
        ctx.globalAlpha = fade * (ink.dark ? 0.2 : 0.14);
        ctx.beginPath();
        ctx.arc(hx * k, hy * k, c.size * 3.4 * dpr, 0, Math.PI * 2);
        ctx.fill();
        ctx.globalAlpha = fade;
        ctx.beginPath();
        ctx.arc(hx * k, hy * k, c.size * dpr, 0, Math.PI * 2);
        ctx.fill();
      }

      // What lands on a client ripples where it lands.
      for (let i = sparks.length - 1; i >= 0; i--) {
        const s = sparks[i] as Spark;
        s.age += dt;
        if (s.age >= 0.45) {
          sparks.splice(i, 1);
          continue;
        }
        const p = s.age / 0.45;
        ctx.globalAlpha = (1 - p) ** 2 * 0.55;
        ctx.strokeStyle = rgb(s.rgb);
        ctx.lineWidth = dpr;
        ctx.beginPath();
        ctx.arc(s.x * k, s.y * k, (1.5 + p * 8) * dpr, 0, Math.PI * 2);
        ctx.stroke();
      }

      // The gateway warms with what passes through and cools slowly.
      if (energy > 0.01) {
        const cx = HUB_X * k;
        const cy = hubY * k;
        const r = HUB_R * k * (1.3 + energy * 0.85);
        const glow = ctx.createRadialGradient(cx, cy, HUB_R * k * 0.72, cx, cy, r);
        const [a, b, c] = ink.hub;
        glow.addColorStop(0, `rgba(${a},${b},${c},${0.42 * energy})`);
        glow.addColorStop(1, `rgba(${a},${b},${c},0)`);
        ctx.globalAlpha = 1;
        ctx.fillStyle = glow;
        ctx.beginPath();
        ctx.arc(cx, cy, r, 0, Math.PI * 2);
        ctx.fill();
        energy *= 1 - dt * 1.35;
      } else {
        energy = 0;
      }
      ctx.globalAlpha = 1;
      ctx.globalCompositeOperation = "source-over";

      if (motionAllowed() && (comets.length > 0 || streams.size > 0 || sparks.length > 0 || energy > 0)) {
        frame = requestAnimationFrame(draw);
      } else {
        last = 0;
        if (!motionAllowed()) {
          comets.length = 0;
          sparks.length = 0;
          energy = 0;
        }
        ctx.clearRect(0, 0, node.width, node.height);
      }
    };

    wake.current = () => {
      if (!frame && motionAllowed()) frame = requestAnimationFrame(draw);
    };
    wake.current();
    return () => {
      observer.disconnect();
      if (frame) cancelAnimationFrame(frame);
      wake.current = () => undefined;
    };
  }, [canvas, hubY, streamsRef, cometsRef, focusRef, wake]);
}

// ---- the chart -------------------------------------------------------------------------------

export function Flow(props: {
  links: FlowLink[];
  metric: FlowMetric;
  active: LiveRequest[];
  /** Requests logged since the last look: the ones too short to catch in flight show up here. */
  done: { rows: LiveDone[]; tick: number };
  /** Shown as the two ends until there is traffic to draw. */
  accounts: { provider: string; alias: string }[];
  clients: string[];
}) {
  const { links, metric, active, done } = props;
  const [hover, setHover] = useState<string>();
  const [pinned, setPinned] = useState<string>();
  const [order, setOrder] = useState<Order>(loadOrder);
  const [drag, setDrag] = useState<{ id: string; side: Side; y: number; ids: string[] }>();

  const model = useMemo(() => {
    const upstream = new Map<string, Node>();
    const downstream = new Map<string, Node>();
    const ensureUp = (provider: string, account: string) => {
      const id = upstreamId({ provider, account });
      const n = upstream.get(id) ?? { id, label: provider, sub: account, value: 0, hue: null };
      upstream.set(id, n);
      return n;
    };
    const ensureDown = (client: string) => {
      const id = downstreamId({ client });
      const n = downstream.get(id) ?? { id, label: client || t("flow.unrecorded"), value: 0, hue: null };
      downstream.set(id, n);
      return n;
    };
    for (const l of links) {
      const value = worth(l, metric);
      if (value <= 0) continue;
      ensureUp(l.provider, l.account).value += value;
      ensureDown(l.client).value += value;
    }
    if (upstream.size === 0) {
      for (const a of props.accounts.slice(0, MAX_NODES)) ensureUp(a.provider, a.alias);
      for (const c of props.clients.slice(0, MAX_NODES)) ensureDown(c);
    }
    for (const r of active) {
      ensureUp(r.provider, r.account);
      ensureDown(r.client);
    }
    const left = paint(collapse(upstream, new Set(active.map(upstreamId))), 0);
    const current = new Set(props.clients);
    const right = paint(
      collapse(
        downstream,
        new Set(active.map(downstreamId)),
        (id) => current.size > 0 && !current.has(id) && id !== UNRECORDED && id !== PROBE_CLIENT,
      ),
      2,
    );
    const known = (list: Node[], id: string) => (list.some((n) => n.id === id) ? id : OTHER);
    const pairs = new Map<string, { up: string; down: string; value: number; link: FlowLink[] }>();
    for (const l of links) {
      const value = worth(l, metric);
      if (value <= 0) continue;
      const up = known(left, upstreamId(l));
      const down = known(right, downstreamId(l));
      const key = `${up}\0${down}`;
      const p = pairs.get(key) ?? { up, down, value: 0, link: [] };
      p.value += value;
      p.link.push(l);
      pairs.set(key, p);
    }
    return { left, right, pairs: [...pairs.values()], total: left.reduce((s, n) => s + n.value, 0), known };
    // The configured ends only matter before there is traffic.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [links, metric, active, props.accounts, props.clients]);

  // Ends keep their rows while traffic changes the numbers: a chart that reshuffles itself cannot
  // be read. Largest first when an end first appears; the metric starts the order over.
  const settled = useRef({ metric, left: [] as string[], right: [] as string[] });
  if (settled.current.metric !== metric) settled.current = { metric, left: [], right: [] };
  const settle = (nodes: Node[], list: string[], chosen: string[]) => {
    for (const n of [...nodes].sort((a, b) => b.value - a.value)) {
      if (n.id !== OTHER && !list.includes(n.id)) list.push(n.id);
    }
    return [...chosen, ...list.filter((id) => !chosen.includes(id))];
  };
  const left = useMemo(
    () => arrange(model.left, settle(model.left, settled.current.left, drag?.side === "left" ? drag.ids : order.left)),
    [model.left, order.left, drag],
  );
  const right = useMemo(
    () => arrange(model.right, settle(model.right, settled.current.right, drag?.side === "right" ? drag.ids : order.right)),
    [model.right, order.right, drag],
  );

  const rows = Math.max(left.length, right.length, 2);
  const height = rows * ROW + 16;
  const center = height / 2;
  const leftEdge = LABEL;
  const rightEdge = WIDTH - LABEL;
  const format = (v: number) => (metric === "requests" ? num(v) : fmtTokens(v));
  const slot = (list: Node[], i: number) => center + (i - (list.length - 1) / 2) * ROW;
  const yOf = (list: Node[], i: number) => (drag && list[i]?.id === drag.id ? drag.y : slot(list, i));

  const strands = useMemo(() => {
    const out: Drawn[] = [];
    const share = (value: number) =>
      model.total > 0 ? Math.max(2, Math.round((value / model.total) * MAX_STRANDS)) : 2;
    const place = (list: Node[], side: "up" | "down") => {
      list.forEach((node, i) => {
        const count = share(node.value);
        const y = drag && node.id === drag.id ? drag.y : center + (i - (list.length - 1) / 2) * ROW;
        for (let n = 0; n < count; n++) {
          const spread = (n - (count - 1) / 2) * 2.4;
          out.push({
            key: `${side}-${node.id}-${n}`,
            s:
              side === "up"
                ? { x1: leftEdge, y1: y + spread, x2: HUB_X - HUB_R, y2: center + spread * 0.42 }
                : { x1: HUB_X + HUB_R, y1: center + spread * 0.42, x2: rightEdge, y2: y + spread },
            side,
            node: node.id,
            nth: n,
            of: count,
            hue: node.hue,
          });
        }
      });
    };
    place(left, "up");
    place(right, "down");
    return out;
  }, [left, right, model.total, center, leftEdge, rightEdge, drag]);

  const strandsOf = useMemo(() => {
    const m = new Map<string, Drawn[]>();
    for (const s of strands) m.set(s.node, [...(m.get(s.node) ?? []), s]);
    return m;
  }, [strands]);

  // ---- focus ------------------------------------------------------------------------------

  const focus = pinned ?? (drag ? undefined : hover);
  useEffect(() => {
    // An end that is gone from the picture cannot stay followed.
    if (pinned && !left.some((n) => n.id === pinned) && !right.some((n) => n.id === pinned)) setPinned(undefined);
  }, [pinned, left, right]);

  const focused = useMemo(() => {
    if (!focus) return undefined;
    const partners = new Map<string, number>();
    let total = 0;
    for (const p of model.pairs) {
      if (p.up === focus) {
        partners.set(p.down, (partners.get(p.down) ?? 0) + p.value);
        total += p.value;
      }
      if (p.down === focus) {
        partners.set(p.up, (partners.get(p.up) ?? 0) + p.value);
        total += p.value;
      }
    }
    return { id: focus, partners, total };
  }, [focus, model.pairs]);

  const valueOf = useMemo(() => new Map([...left, ...right].map((n) => [n.id, n.value])), [left, right]);
  /** On the far side only as much of a ribbon lights up as really comes from what is followed. */
  const strandState = (s: Drawn) => {
    if (!focused) return "";
    if (s.node === focused.id) return "focus";
    const part = focused.partners.get(s.node);
    if (part == null) return "dim";
    const whole = valueOf.get(s.node) ?? 0;
    return s.nth < Math.max(1, Math.ceil((whole > 0 ? part / whole : 1) * s.of)) ? "focus" : "dim";
  };
  const nodeDimmed = (id: string) => !!focused && id !== focused.id && !focused.partners.has(id);

  const detail = useMemo(() => {
    if (!focus) return undefined;
    const rel = model.pairs.filter((p) => p.up === focus || p.down === focus).flatMap((p) => p.link);
    if (rel.length === 0) return undefined;
    const models = new Map<string, number>();
    const out = { requests: 0, errors: 0, input: 0, output: 0, cost: 0 };
    for (const l of rel) {
      out.requests += l.requests;
      out.errors += l.errors;
      out.input += l.input_tokens;
      out.output += l.output_tokens;
      out.cost += l.cost_usd ?? 0;
      models.set(l.model, (models.get(l.model) ?? 0) + worth(l, metric));
    }
    return { ...out, top: [...models.entries()].sort((a, b) => b[1] - a[1]).slice(0, 4) };
  }, [focus, model.pairs, metric]);

  const liveNodes = useMemo(() => {
    const s = new Set<string>();
    for (const r of active) {
      s.add(model.known(model.left, upstreamId(r)));
      s.add(model.known(model.right, downstreamId(r)));
    }
    return s;
  }, [active, model]);

  // ---- live layer -------------------------------------------------------------------------

  const canvas = useRef<HTMLCanvasElement>(null);
  const box = useRef<HTMLDivElement>(null);
  const streams = useRef(new Map<string, Stream>());
  const comets = useRef<Comet[]>([]);
  const focusRef = useRef<string | undefined>(undefined);
  focusRef.current = focus;
  const wake = useRef<() => void>(() => undefined);
  const seen = useRef(new Map<string, { bytes: number; at: number }>());
  const justEnded = useRef(new Map<string, number>());
  useFlowMotion(canvas, center, streams, comets, focusRef, wake);
  const { on: motion } = useMotion();
  useEffect(() => {
    if (motion) wake.current();
  }, [motion]);

  useEffect(() => {
    const now = performance.now();
    const alive = new Set<string>();
    for (const r of active) {
      const up = model.known(model.left, upstreamId(r));
      const down = model.known(model.right, downstreamId(r));
      const key = `${r.id}\0${up}`;
      alive.add(key);
      const before = seen.current.get(key);
      const dt = before ? (now - before.at) / 1000 : 0;
      const bytesPerSec = before && dt > 0 ? (r.bytes - before.bytes) / dt : 0;
      seen.current.set(key, { bytes: r.bytes, at: now });
      // A reply that is not streamed has no byte count yet: a steady trickle until it lands.
      const measured = r.bytes > 0 ? particlesPerSecond(bytesPerSec) : 3;
      const from = strandsOf.get(up) ?? [];
      const to = strandsOf.get(down) ?? [];
      const known = streams.current.get(key);
      if (known) {
        known.from = from;
        known.to = to;
        known.up = up;
        known.down = down;
        known.rate += (measured - known.rate) * 0.45;
        known.answering = r.answering;
        known.ending = false;
      } else {
        streams.current.set(key, { key, up, down, from, to, rate: measured, answering: r.answering, ending: false, level: 0, clock: Math.random() * 200 });
        const a = from[Math.floor(from.length / 2)];
        const b = to[Math.floor(to.length / 2)];
        if (a && b) {
          comets.current.push({ from: a.s, to: b.s, fromHue: a.hue, toHue: b.hue, up, down, t: 0, speed: 1.1, size: 1.1, wobble: 0, seed: 0, hold: 0, failed: false, back: true });
        }
      }
    }
    for (const [key, s] of streams.current) {
      if (!alive.has(key) && !s.ending) {
        s.ending = true;
        justEnded.current.set(s.up, now);
        seen.current.delete(key);
      }
    }
    wake.current();
  }, [active, model, strandsOf]);

  useEffect(() => {
    const now = performance.now();
    for (const r of done.rows.slice(-16)) {
      const up = model.known(model.left, upstreamId(r));
      const down = model.known(model.right, downstreamId(r));
      const from = strandsOf.get(up) ?? [];
      const to = strandsOf.get(down) ?? [];
      if (from.length === 0 || to.length === 0) continue;
      const ended = justEnded.current.get(up);
      const watched = r.ok && ended != null && now - ended < 6000;
      if (watched) justEnded.current.delete(up);
      // Failures always show; a reply already watched in flight needs no second burst.
      const count = !r.ok ? 2 : watched ? 0 : Math.min(16, 5 + Math.round(Math.log2(r.output_tokens + 1)));
      for (let i = 0; i < count; i++) {
        const a = from[Math.floor(Math.random() * from.length)] as Drawn;
        const b = to[Math.floor(Math.random() * to.length)] as Drawn;
        comets.current.push({
          from: a.s,
          to: b.s,
          fromHue: a.hue,
          toHue: b.hue,
          up,
          down,
          t: -Math.random() * 0.35,
          speed: 0.5 + Math.random() * 0.4,
          size: 0.8 + 2 * Math.random() ** 3,
          wobble: Math.random() * 2.4,
          seed: Math.random() * 6.28,
          hold: 0,
          failed: !r.ok,
          back: false,
        });
      }
    }
    wake.current();
    // Only a new batch of rows should fire; re-layout must not replay them.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [done.tick]);

  // ---- picking up an end ------------------------------------------------------------------

  const press = useRef<{ id: string; side: Side; startY: number; moved: boolean } | null>(null);
  const skipClick = useRef(false);
  const latestDrag = useRef<{ id: string; side: Side; y: number; ids: string[] } | undefined>(undefined);

  const commit = (side: Side, ids: string[]) => {
    const next = { ...order, [side]: ids };
    setOrder(next);
    saveOrder(next);
  };
  const resetOrder = (side: Side) => commit(side, []);

  const onPointerDown = (e: ReactPointerEvent<HTMLButtonElement>, n: Node, side: Side) => {
    if (e.button !== 0) return;
    press.current = { id: n.id, side, startY: e.clientY, moved: false };
    e.currentTarget.setPointerCapture(e.pointerId);
  };
  const onPointerMove = (e: ReactPointerEvent<HTMLButtonElement>, n: Node, side: Side, list: Node[]) => {
    const p = press.current;
    if (!p || p.id !== n.id || n.id === OTHER) return;
    if (!p.moved && Math.abs(e.clientY - p.startY) < 4) return;
    p.moved = true;
    const rect = box.current?.getBoundingClientRect();
    if (!rect) return;
    const y = Math.max(8, Math.min(height - 8, ((e.clientY - rect.top) / rect.height) * height));
    const movable = list.filter((x) => x.id !== OTHER && x.id !== n.id).map((x) => x.id);
    const first = center - ((list.length - 1) / 2) * ROW;
    const at = Math.max(0, Math.min(movable.length, Math.round((y - first) / ROW)));
    const next = { id: n.id, side, y, ids: [...movable.slice(0, at), n.id, ...movable.slice(at)] };
    latestDrag.current = next;
    setDrag(next);
  };
  const onPointerUp = (e: ReactPointerEvent<HTMLButtonElement>, n: Node, side: Side, list: Node[]) => {
    // A quick drop arrives before the last move has rendered: place it from where it was let go.
    onPointerMove(e, n, side, list);
    const p = press.current;
    press.current = null;
    if (!p?.moved) return;
    skipClick.current = true;
    if (latestDrag.current) commit(latestDrag.current.side, latestDrag.current.ids);
    latestDrag.current = undefined;
    setDrag(undefined);
  };
  const onPointerCancel = () => {
    press.current = null;
    latestDrag.current = undefined;
    setDrag(undefined);
  };
  const onNodeClick = (e: React.MouseEvent, n: Node) => {
    e.stopPropagation();
    if (skipClick.current) {
      skipClick.current = false;
      return;
    }
    setPinned(pinned === n.id ? undefined : n.id);
  };
  const onNodeKey = (e: ReactKeyboardEvent, n: Node, side: Side, list: Node[]) => {
    if (!e.altKey || (e.key !== "ArrowUp" && e.key !== "ArrowDown") || n.id === OTHER) return;
    e.preventDefault();
    const ids = list.filter((x) => x.id !== OTHER).map((x) => x.id);
    const from = ids.indexOf(n.id);
    const to = from + (e.key === "ArrowUp" ? -1 : 1);
    if (to < 0 || to >= ids.length) return;
    ids.splice(from, 1);
    ids.splice(to, 0, n.id);
    commit(side, ids);
  };

  const renderNode = (n: Node, side: Side, list: Node[], i: number) => {
    const part = focused?.partners.get(n.id);
    const base = part ?? n.value;
    const whole = part != null ? focused?.total || 1 : model.total;
    const pct = whole > 0 ? (base / whole) * 100 : 0;
    const title = n.sub ? `${n.label}/${n.sub}` : n.label;
    const dragging = drag?.id === n.id;
    return (
      <button
        key={n.id}
        type="button"
        className={`flow-node ${side} ${focus === n.id ? "focus" : ""} ${pinned === n.id ? "pinned" : ""} ${nodeDimmed(n.id) ? "dim" : ""} ${liveNodes.has(n.id) ? "live" : ""} ${dragging ? "dragging" : ""} ${n.id === OTHER ? "rest" : ""}`}
        style={{ top: `${(yOf(list, i) / height) * 100}%`, "--node": hueVar(n.hue) } as CSSProperties}
        aria-pressed={pinned === n.id}
        onPointerDown={(e) => onPointerDown(e, n, side)}
        onPointerMove={(e) => onPointerMove(e, n, side, list)}
        onPointerUp={(e) => onPointerUp(e, n, side, list)}
        onPointerCancel={onPointerCancel}
        onClick={(e) => onNodeClick(e, n)}
        onKeyDown={(e) => onNodeKey(e, n, side, list)}
        onMouseEnter={() => setHover(n.id)}
        onMouseLeave={() => setHover(undefined)}
      >
        <span className="flow-node-name" title={title}>
          {n.id !== OTHER && <GripVertical size={12} className="flow-grip" aria-hidden />}
          <span className="ellipsis">
            {n.label}
            {n.sub && <span className="flow-node-sub"> {n.sub}</span>}
          </span>
          <i className="flow-dot" aria-hidden />
        </span>
        {model.total > 0 && (
          <span className="flow-node-value">
            {format(base)}
            <span className="flow-node-pct"> {pct < 1 ? "<1" : pct.toFixed(0)}%</span>
          </span>
        )}
      </button>
    );
  };

  const hues = [...Array.from({ length: HUES }, (_, i) => i + 1), null];

  return (
    <div
      className="flow"
      onKeyDown={(e) => {
        if (e.key === "Escape" && pinned) setPinned(undefined);
      }}
    >
      <div
        className={`flow-canvas ${focused ? "focusing" : ""} ${drag ? "reordering" : ""}`}
        ref={box}
        style={{ aspectRatio: `${WIDTH} / ${height}` }}
        onClick={() => setPinned(undefined)}
      >
        <svg viewBox={`0 0 ${WIDTH} ${height}`} role="img" aria-label={t("flow.aria")} preserveAspectRatio="none">
          <defs>
            {hues.map((h) => (
              <linearGradient key={`u${h}`} id={`flow-up-${h ?? 0}`} gradientUnits="userSpaceOnUse" x1={leftEdge} y1="0" x2={HUB_X} y2="0">
                <stop offset="0" style={{ stopColor: hueVar(h), stopOpacity: 0.95 }} />
                <stop offset="1" style={{ stopColor: hueVar(h), stopOpacity: 0.32 }} />
              </linearGradient>
            ))}
            {hues.map((h) => (
              <linearGradient key={`d${h}`} id={`flow-down-${h ?? 0}`} gradientUnits="userSpaceOnUse" x1={HUB_X} y1="0" x2={rightEdge} y2="0">
                <stop offset="0" style={{ stopColor: hueVar(h), stopOpacity: 0.32 }} />
                <stop offset="1" style={{ stopColor: hueVar(h), stopOpacity: 0.95 }} />
              </linearGradient>
            ))}
          </defs>
          <g className="flow-grid">
            {Array.from({ length: 20 }, (_, x) =>
              Array.from({ length: Math.ceil(height / 26) }, (_, y) => (
                <circle key={`${x}-${y}`} cx={leftEdge + (x * (rightEdge - leftEdge)) / 19} cy={8 + y * 26} r={0.8} />
              )),
            )}
          </g>
          {strands.map((s) => {
            const d = strandPath(s.s);
            return (
              <path
                key={s.key}
                d={d}
                // Set as a style too, so a re-ordered ribbon glides to its place instead of jumping.
                style={{ d: `path("${d}")`, stroke: `url(#flow-${s.side}-${s.hue ?? 0})` } as CSSProperties}
                className={`flow-strand ${strandState(s)} ${liveNodes.has(s.node) ? "live" : ""} ${drag?.id === s.node ? "held" : ""}`}
              />
            );
          })}
        </svg>
        <canvas ref={canvas} className="flow-comets" aria-hidden />
        <span className="flow-end left">
          {order.left.length > 0 && (
            <button type="button" title={t("flow.resetOrder")} aria-label={t("flow.resetOrder")} onClick={(e) => { e.stopPropagation(); resetOrder("left"); }}>
              <RotateCcw size={11} aria-hidden />
            </button>
          )}
          {t("flow.upstream")}
        </span>
        <span className="flow-end right">
          {t("flow.downstream")}
          {order.right.length > 0 && (
            <button type="button" title={t("flow.resetOrder")} aria-label={t("flow.resetOrder")} onClick={(e) => { e.stopPropagation(); resetOrder("right"); }}>
              <RotateCcw size={11} aria-hidden />
            </button>
          )}
        </span>
        {left.map((n, i) => renderNode(n, "left", left, i))}
        {right.map((n, i) => renderNode(n, "right", right, i))}
        {left.length === 0 && (
          <a className="flow-ghost left" href="#/accounts">
            <Plus size={14} aria-hidden /> {t("acc.add")}
          </a>
        )}
        {right.length === 0 && (
          <a className="flow-ghost right" href="#/clients">
            <Plus size={14} aria-hidden /> {t("clients.new")}
          </a>
        )}
        <div className={`flow-hub ${active.length > 0 ? "live" : ""}`} aria-hidden style={{ top: `${(center / height) * 100}%` }}>
          <HubMark />
        </div>
      </div>
      <div className="flow-foot">
        {detail && (
          <>
            <span>{t("flow.requests", { n: num(detail.requests) })}</span>
            {detail.errors > 0 && <span className="bad-text">{t("flow.errors", { n: num(detail.errors) })}</span>}
            <span>{t("flow.inOut", { input: fmtTokens(detail.input), output: fmtTokens(detail.output) })}</span>
            {detail.cost > 0 && <span>{usd(detail.cost)}</span>}
            <span className="flow-models ellipsis">{detail.top.map(([m]) => m).join("  ")}</span>
          </>
        )}
      </div>
    </div>
  );
}
