// SPDX-License-Identifier: Apache-2.0

import { useCallback, useEffect, useRef, useState } from "react";
import { IpcFailure } from "./api";

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
