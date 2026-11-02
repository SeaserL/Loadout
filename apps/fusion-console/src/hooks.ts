// SPDX-License-Identifier: Apache-2.0

import { type RefObject, useCallback, useEffect, useRef, useState } from "react";
import { IpcFailure } from "./api";
import { motionAllowed, motionRate } from "./theme";

/** The rendered width of an element, kept current with a ResizeObserver (for SVG charts). */
export function useWidth<T extends HTMLElement>(): [RefObject<T | null>, number] {
  const ref = useRef<T>(null);
  const [width, setWidth] = useState(0);
  useEffect(() => {
    const node = ref.current;
    if (!node) return;
    setWidth(node.clientWidth);
    const observer = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width;
      if (w != null) setWidth(Math.round(w));
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  return [ref, width];
}

export interface Polled<T> {
  data: T | undefined;
  error: IpcFailure | undefined;
  loading: boolean;
  reload: () => Promise<void>;
}

/**
 * Loads `fetch` now and every `intervalMs` while the window is visible. IPC calls are
 * local and cheap, so polling keeps the UI simple.
 */
export function usePoll<T>(fetch: () => Promise<T>, intervalMs = 5000, deps: unknown[] = []): Polled<T> {
  const [data, setData] = useState<T>();
  const [error, setError] = useState<IpcFailure>();
  const [loading, setLoading] = useState(true);
  const fetchRef = useRef(fetch);
  fetchRef.current = fetch;

  const reload = useCallback(async () => {
    try {
      const value = await fetchRef.current();
      setData(value);
      setError(undefined);
    } catch (e) {
      setError(e instanceof IpcFailure ? e : new IpcFailure("ipc", String(e)));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
    if (intervalMs <= 0) return;
    const id = window.setInterval(() => {
      if (document.visibilityState === "visible") void reload();
    }, intervalMs);
    return () => window.clearInterval(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reload, intervalMs, ...deps]);

  return { data, error, loading, reload };
}

/** A number that rolls to its new value, so a change is felt rather than read. */
export function useRolling(value: number) {
  const [shown, setShown] = useState(value);
  const from = useRef(value);
  useEffect(() => {
    if (!motionAllowed()) {
      from.current = value;
      setShown(value);
      return;
    }
    const start = performance.now();
    const origin = from.current;
    const length = 650 / motionRate();
    let frame = requestAnimationFrame(function step(now) {
      const p = Math.min(1, (now - start) / length);
      const v = origin + (value - origin) * (1 - (1 - p) ** 3);
      from.current = v;
      setShown(v);
      if (p < 1) frame = requestAnimationFrame(step);
    });
    return () => cancelAnimationFrame(frame);
  }, [value]);
  return shown;
}

/** Runs an action, tracking busy state and the last error. */
export function useAction() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const run = useCallback(async <T,>(action: () => Promise<T>): Promise<T | undefined> => {
    setBusy(true);
    setError(undefined);
    try {
      return await action();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      return undefined;
    } finally {
      setBusy(false);
    }
  }, []);
  return { busy, error, setError, run };
}
