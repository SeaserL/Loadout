// SPDX-License-Identifier: Apache-2.0

import { locale, t } from "./i18n";

export function time(ms: number | null | undefined): string {
  if (ms == null) return "-";
  return new Date(ms).toLocaleString(locale());
}

export function date(ms: number | null | undefined): string {
  if (ms == null) return "-";
  return new Date(ms).toLocaleDateString(locale());
}

/** "in 3 h 20 min" style distance to a future time. */
export function until(ms: number | null | undefined): string {
  if (ms == null) return "-";
  const diff = ms - Date.now();
  if (diff <= 0) return t("common.now");
  const min = Math.round(diff / 60000);
  if (min < 60) return t("common.inMin", { n: min });
  const h = Math.floor(min / 60);
  if (h < 48) return t("common.inHours", { h, m: min % 60 });
  return t("common.inDays", { n: Math.round(h / 24) });
}

export function ago(ms: number | null | undefined): string {
  if (ms == null) return t("common.never");
  const s = Math.round((Date.now() - ms) / 1000);
  if (s < 60) return t("common.secAgo", { n: s });
  if (s < 3600) return t("common.minAgo", { n: Math.round(s / 60) });
  if (s < 86400) return t("common.hoursAgo", { n: Math.round(s / 3600) });
  return t("common.daysAgo", { n: Math.round(s / 86400) });
}

export function num(n: number | null | undefined): string {
  if (n == null) return "-";
  if (Math.abs(n) >= 1e6) return `${(n / 1e6).toFixed(1)}M`;
  if (Math.abs(n) >= 1e4) return `${(n / 1e3).toFixed(1)}k`;
  return n.toLocaleString(locale());
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
      return t("tier.ULTRA_FAST");
    case "FAST":
      return t("tier.FAST");
    case "NORMAL":
      return t("tier.NORMAL");
    default:
      return t("tier.UNMEASURED");
  }
}

/** "3 d 4 h" / "2 h 5 min" / "12 min" style length of time. */
export function duration(ms: number | null | undefined): string {
  if (ms == null || ms < 0) return "-";
  const min = Math.floor(ms / 60000);
  if (min < 60) return t("common.durMin", { n: min });
  const h = Math.floor(min / 60);
  if (h < 24) return t("common.durHours", { h, m: min % 60 });
  return t("common.durDays", { d: Math.floor(h / 24), h: h % 24 });
}

/** Short reset countdown for quota bars: "2 h 50 min", "4 d 19 h". */
export function resetIn(ms: number | null | undefined): string | null {
  if (ms == null) return null;
  const diff = ms - Date.now();
  return diff <= 0 ? t("common.now") : duration(diff);
}

/** Compact token count: 1.25M, 35.4k, 980. */
export function tokens(n: number | null | undefined): string {
  if (n == null) return "-";
  if (Math.abs(n) >= 1e9) return `${(n / 1e9).toFixed(2)}B`;
  if (Math.abs(n) >= 1e6) return `${(n / 1e6).toFixed(2)}M`;
  if (Math.abs(n) >= 1e3) return `${(n / 1e3).toFixed(1)}k`;
  return String(Math.round(n));
}
