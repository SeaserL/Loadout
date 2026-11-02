// SPDX-License-Identifier: Apache-2.0
//
// Light / dark / follow-system theme. The choice is a display preference kept in the
// WebView's local storage (like the language), applied as `data-theme` on <html>, and
// mirrored to the native window so its background and title bar match (no white flash).

import { useSyncExternalStore } from "react";

export type ThemePreference = "light" | "dark" | "system";
export type Theme = "light" | "dark";

const STORAGE_KEY = "loadout.theme";
const listeners = new Set<() => void>();
const media = typeof window !== "undefined" ? window.matchMedia?.("(prefers-color-scheme: dark)") : undefined;

function readPreference(): ThemePreference {
  try {
    const v = window.localStorage.getItem(STORAGE_KEY);
    if (v === "light" || v === "dark" || v === "system") return v;
  } catch {
    // Storage unavailable: follow the system.
  }
  return "system";
}

let preference: ThemePreference = readPreference();

/** Colour presets, each with a light and a dark form (styles.css). Set apart from light/dark. */
export const PALETTES = ["turquoise", "porcelain", "wisteria", "amber", "rouge", "ink"] as const;
export type PaletteId = (typeof PALETTES)[number];
const PALETTE_KEY = "loadout.palette";

function readPalette(): PaletteId {
  try {
    const v = window.localStorage.getItem(PALETTE_KEY);
    if ((PALETTES as readonly string[]).includes(v ?? "")) return v as PaletteId;
  } catch {
    // Storage unavailable: the default colours.
  }
  return "turquoise";
}

let palette: PaletteId = readPalette();

/** The global motion switch. Off, or when the system asks for less motion, nothing animates. */
const MOTION_KEY = "loadout.motion";
const reduced = typeof window !== "undefined" ? window.matchMedia?.("(prefers-reduced-motion: reduce)") : undefined;

function readMotion(): boolean {
  try {
    return window.localStorage.getItem(MOTION_KEY) !== "off";
  } catch {
    return true;
  }
}

let motion = readMotion();

export const motionAllowed = () => motion && !reduced?.matches;

function resolve(p: ThemePreference): Theme {
  if (p === "system") return media?.matches ? "dark" : "light";
  return p;
}

function apply() {
  const theme = resolve(preference);
  const root = document.documentElement;
  // Switch colours at once instead of letting every control fade separately.
  root.classList.add("theme-switching");
  window.setTimeout(() => root.classList.remove("theme-switching"), 50);
  root.dataset.theme = theme;
  root.dataset.palette = palette;
  root.dataset.motion = motionAllowed() ? "on" : "off";
  root.style.colorScheme = theme;
  for (const l of listeners) l();
}

export function setThemePreference(p: ThemePreference) {
  preference = p;
  try {
    window.localStorage.setItem(STORAGE_KEY, p);
  } catch {
    // Not remembered, but still applied for this session.
  }
  apply();
}

export function setPalette(p: PaletteId) {
  palette = p;
  try {
    window.localStorage.setItem(PALETTE_KEY, p);
  } catch {
    // Not remembered, but still applied for this session.
  }
  apply();
}

export function setMotion(on: boolean) {
  motion = on;
  try {
    window.localStorage.setItem(MOTION_KEY, on ? "on" : "off");
  } catch {
    // Not remembered, but still applied for this session.
  }
  apply();
}

/** Applies the stored theme before the first render and follows system changes. */
export function initTheme() {
  apply();
  media?.addEventListener?.("change", () => {
    if (preference === "system") apply();
  });
  reduced?.addEventListener?.("change", apply);
}

function subscribe(l: () => void) {
  listeners.add(l);
  return () => {
    listeners.delete(l);
  };
}

export function useMotion(): { on: boolean; setMotion: (on: boolean) => void } {
  return { on: useSyncExternalStore(subscribe, () => motion), setMotion };
}

export function usePalette(): { palette: PaletteId; setPalette: (p: PaletteId) => void } {
  return { palette: useSyncExternalStore(subscribe, () => palette), setPalette };
}

export function useTheme(): { preference: ThemePreference; theme: Theme; setPreference: (p: ThemePreference) => void } {
  const p = useSyncExternalStore(subscribe, () => preference);
  return { preference: p, theme: resolve(p), setPreference: setThemePreference };
}
