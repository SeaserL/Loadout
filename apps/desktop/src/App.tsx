// SPDX-License-Identifier: Apache-2.0

import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import { Button, ErrorText, LanguageSelect } from "./components";
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

const PAGES: { id: string; label: MessageKey }[] = [
  { id: "dashboard", label: "nav.dashboard" },
  { id: "accounts", label: "nav.accounts" },
  { id: "models", label: "nav.models" },
  { id: "routes", label: "nav.routes" },
  { id: "clients", label: "nav.clients" },
  { id: "stats", label: "nav.stats" },
  { id: "logs", label: "nav.logs" },
  { id: "settings", label: "nav.settings" },
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

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <div className="logo">L</div>
          Loadout
        </div>
        <nav className="nav">
          {PAGES.map((p) => (
            <a key={p.id} href={`#/${p.id}`} className={p.id === page ? "active" : ""}>
              {t(p.label)}
            </a>
          ))}
        </nav>
        <div className="footer">
          <div>
            <span className={`dot ${running ? "ok" : "bad"}`} />
            {running ? t("shell.running") : status.loading ? t("shell.connecting") : t("shell.stopped")}
          </div>
          {status.data && <div className="mono small">http://{status.data.gateway.addr}/v1</div>}
          {status.data && <div className="small">v{status.data.version}</div>}
          <div style={{ marginTop: 8 }}>
            <LanguageSelect compact />
          </div>
        </div>
      </aside>
      <main>
        {!running && !status.loading && (
          <div className="banner">
            <div>
              <strong>{t("shell.notRunning")}</strong>
              <div className="small muted">{notRunning ? t("shell.notRunningHint") : status.error?.message}</div>
              <ErrorText error={start.error} />
            </div>
            <Button
              kind="primary"
              disabled={start.busy}
              onClick={() =>
                void start.run(async () => {
                  await api.startDaemon();
                  await status.reload();
                })
              }
            >
              {start.busy ? t("shell.starting") : t("shell.start")}
            </Button>
          </div>
        )}
        {running && status.data?.recovered_from && (
          <div className="banner">
            <div>
              <strong>{t("shell.recovered")}</strong>
              <div className="small">
                {tr("shell.recoveredHint", {
                  file: <span className="mono">{status.data.recovered_from}</span>,
                  dir: <span className="mono">{status.data.data_dir}\backups\auto</span>,
                })}
              </div>
            </div>
          </div>
        )}
        {running && status.data && (
          <>
            {page === "dashboard" && <Dashboard status={status.data} />}
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
