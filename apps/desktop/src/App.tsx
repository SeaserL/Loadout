// SPDX-License-Identifier: Apache-2.0

import { type ComponentType, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Boxes,
  ChartColumn,
  Gauge,
  House,
  KeyRound,
  Layers,
  Lock,
  Play,
  Route,
  ScrollText,
  Settings as SettingsIcon,
} from "lucide-react";
import { api } from "./api";
import { Button, LanguageSelect, LockedPage, Notice, ThemeSwitch } from "./components";
import { useAction, usePoll } from "./hooks";
import { type MessageKey, t, tr, useLang } from "./i18n";
import { Dashboard } from "./pages/Dashboard";
import { Accounts } from "./pages/Accounts";
import { Models } from "./pages/Models";
import { Routes } from "./pages/Routes";
import { Clients } from "./pages/Clients";
import { Logs } from "./pages/Logs";
import { Stats } from "./pages/Stats";
import { Settings } from "./pages/Settings";

const PAGES: { id: string; label: MessageKey; icon: ComponentType<{ size?: number }> }[] = [
  { id: "dashboard", label: "nav.dashboard", icon: House },
  { id: "accounts", label: "nav.accounts", icon: Gauge },
  { id: "models", label: "nav.models", icon: Boxes },
  { id: "routes", label: "nav.routes", icon: Route },
  { id: "clients", label: "nav.clients", icon: KeyRound },
  { id: "stats", label: "nav.stats", icon: ChartColumn },
  { id: "logs", label: "nav.logs", icon: ScrollText },
  { id: "settings", label: "nav.settings", icon: SettingsIcon },
];

function currentPage(): string {
  const id = window.location.hash.replace(/^#\/?/, "");
  return PAGES.find((p) => p.id === id)?.id ?? "dashboard";
}

export function App() {
  // Re-render everything when the language changes.
  useLang();
  const [page, setPage] = useState(currentPage);
  const status = usePoll(api.status, 3000);
  const reloadStatus = status.reload;
  const start = useAction();

  useEffect(() => {
    const onHash = () => setPage(currentPage());
    window.addEventListener("hashchange", onHash);
    const unlisten = listen("daemon-changed", () => void reloadStatus());
    return () => {
      window.removeEventListener("hashchange", onHash);
      void unlisten.then((f) => f());
    };
  }, [reloadStatus]);

  const running = !!status.data && !status.error;
  const notRunning = status.error?.code === "not_running";
  const locked = !running && !status.loading;
  const current = PAGES.find((p) => p.id === page) ?? (PAGES[0] as (typeof PAGES)[number]);

  const startDaemon = () =>
    void start.run(async () => {
      await api.startDaemon();
      await status.reload();
    });

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <div className="logo" aria-hidden>
            <Layers size={20} />
          </div>
          <div>
            <div className="brand-name">Loadout</div>
            <div className="brand-sub">{t("shell.tagline")}</div>
          </div>
        </div>
        <nav className="nav">
          {PAGES.map((p) => {
            const Icon = p.icon;
            return (
              <a
                key={p.id}
                href={`#/${p.id}`}
                className={`${p.id === page ? "active" : ""} ${locked ? "locked" : ""}`}
                aria-current={p.id === page ? "page" : undefined}
              >
                <Icon size={18} />
                <span>{t(p.label)}</span>
                {locked && <Lock size={14} className="nav-lock" aria-label={t("shell.notRunning")} />}
              </a>
            );
          })}
        </nav>
        <div className="sidebar-footer">
          <div className="daemon-status">
            <span className={`dot ${running ? "ok" : status.loading ? "" : "bad"}`} />
            <div className="daemon-text">
              <div>
                {running ? t("shell.running") : status.loading ? t("shell.connecting") : t("shell.stopped")}
              </div>
              {status.data && running && (
                <div className="mono small muted">
                  {status.data.gateway.addr} · v{status.data.version}
                </div>
              )}
            </div>
            {locked && (
              <button
                type="button"
                className="icon-btn"
                onClick={startDaemon}
                disabled={start.busy}
                title={t("shell.start")}
                aria-label={t("shell.start")}
              >
                <Play size={16} />
              </button>
            )}
          </div>
          <ThemeSwitch />
          <LanguageSelect compact />
        </div>
      </aside>
      <main>
        {locked && (
          <LockedPage
            title={t(current.label)}
            hint={notRunning ? t("shell.notRunningHint") : (status.error?.message ?? t("shell.notRunningHint"))}
            error={start.error}
            action={
              <Button kind="primary" icon={<Play size={16} />} disabled={start.busy} onClick={startDaemon}>
                {start.busy ? t("shell.starting") : t("shell.start")}
              </Button>
            }
          />
        )}
        {running && status.data?.recovered_from && (
          <Notice tone="warn">
            <strong>{t("shell.recovered")}</strong>
            <div className="small">
              {tr("shell.recoveredHint", {
                file: <span className="mono">{status.data.recovered_from}</span>,
                dir: <span className="mono">{status.data.data_dir}\backups\auto</span>,
              })}
            </div>
          </Notice>
        )}
        {running && status.data && (
          <>
            {page === "dashboard" && <Dashboard status={status.data} onDaemonChange={status.reload} />}
            {page === "accounts" && <Accounts />}
            {page === "models" && <Models />}
            {page === "routes" && <Routes />}
            {page === "clients" && <Clients gateway={status.data.gateway.addr} />}
            {page === "stats" && <Stats />}
            {page === "logs" && <Logs />}
            {page === "settings" && <Settings status={status.data} onDaemonChange={status.reload} />}
          </>
        )}
      </main>
    </div>
  );
}
