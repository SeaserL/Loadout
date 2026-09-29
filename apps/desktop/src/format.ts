// SPDX-License-Identifier: Apache-2.0

export function time(ms: number | null | undefined): string {
  if (ms == null) return "-";
  return new Date(ms).toLocaleString();
}

export function date(ms: number | null | undefined): string {
  if (ms == null) return "-";
  return new Date(ms).toLocaleDateString();
}

/** "in 3 h 20 min" style distance to a future time. */
export function until(ms: number | null | undefined): string {
  if (ms == null) return "-";
  const diff = ms - Date.now();
  if (diff <= 0) return "now";
  const min = Math.round(diff / 60000);
  if (min < 60) return `in ${min} min`;
  const h = Math.floor(min / 60);
  if (h < 48) return `in ${h} h ${min % 60} min`;
  return `in ${Math.round(h / 24)} d`;
}

export function ago(ms: number | null | undefined): string {
  if (ms == null) return "never";
  const s = Math.round((Date.now() - ms) / 1000);
  if (s < 60) return `${s}s ago`;
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return `${Math.round(s / 86400)} d ago`;
}

export function num(n: number | null | undefined): string {
  if (n == null) return "-";
  if (Math.abs(n) >= 1e6) return `${(n / 1e6).toFixed(1)}M`;
  if (Math.abs(n) >= 1e4) return `${(n / 1e3).toFixed(1)}k`;
  return n.toLocaleString();
}

export function usd(n: number | null | undefined): string {
  if (n == null) return "-";
  if (n !== 0 && Math.abs(n) < 0.01) return `$${n.toFixed(4)}`;
  return `$${n.toFixed(2)}`;
}

export function ms(n: number | null | undefined): string {
  return n == null ? "-" : `${Math.round(n)} ms`;
}

export function tierLabel(tier: string): string {
  switch (tier) {
    case "ULTRA_FAST":
      return "Ultra fast";
    case "FAST":
      return "Fast";
    case "NORMAL":
      return "Normal";
    default:
      return "Unmeasured";
  }
}
