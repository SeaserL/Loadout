// SPDX-License-Identifier: Apache-2.0
//
// Light / dark / follow-system theme. The choice is a display preference kept in the
// WebView's local storage (like the language), applied as `data-theme` on <html>, and
// mirrored to the native window so its background and title bar match (no white flash).

import { useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";

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

function resolve(p: ThemePreference): Theme {
  if (p === "system") return media?.matches ? "dark" : "light";
  return p;
}

let lastApplied: string | undefined;

function apply() {
  const theme = resolve(preference);
  const root = document.documentElement;
  root.dataset.theme = theme;
  root.style.colorScheme = theme;
  const key = `${preference}:${theme}`;
  if (key !== lastApplied) {
    lastApplied = key;
    // Outside Tauri (a plain browser during development) this fails harmlessly.
    void invoke("set_window_theme", { preference, dark: theme === "dark" }).catch(() => undefined);
  }
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

/** Applies the stored theme before the first render and follows system changes. */
export function initTheme() {
  apply();
  media?.addEventListener?.("change", () => {
    if (preference === "system") apply();
  });
}

function subscribe(l: () => void) {
  listeners.add(l);
  return () => {
    listeners.delete(l);
  };
}

export function useTheme(): { preference: ThemePreference; theme: Theme; setPreference: (p: ThemePreference) => void } {
  const p = useSyncExternalStore(subscribe, () => preference);
  return { preference: p, theme: resolve(p), setPreference: setThemePreference };
}
