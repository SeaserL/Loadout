// SPDX-License-Identifier: Apache-2.0

import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import { Button, ErrorText } from "./components";
import { useAction, usePoll } from "./hooks";
import { Dashboard } from "./pages/Dashboard";
import { Accounts } from "./pages/Accounts";
import { Models } from "./pages/Models";
import { Routes } from "./pages/Routes";
import { Clients } from "./pages/Clients";
import { Logs } from "./pages/Logs";
import { Stats } from "./pages/Stats";
import { Settings } from "./pages/Settings";

const PAGES = [
  { id: "dashboard", label: "Dashboard" },
  { id: "accounts", label: "Accounts & usage" },
  { id: "models", label: "Models" },
  { id: "routes", label: "Routes" },
  { id: "clients", label: "Client keys" },
  { id: "stats", label: "Statistics" },
  { id: "logs", label: "Request log" },
  { id: "settings", label: "Settings" },
] as const;

type PageId = (typeof PAGES)[number]["id"];

function currentPage(): PageId {
  const id = window.location.hash.replace(/^#\/?/, "");
  return (PAGES.find((p) => p.id === id)?.id ?? "dashboard") as PageId;
}

export function App() {
  const [page, setPage] = useState<PageId>(currentPage);
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
              {p.label}
            </a>
          ))}
        </nav>
        <div className="footer">
          <div>
            <span className={`dot ${running ? "ok" : "bad"}`} />
            {running ? "Gateway running" : status.loading ? "Connecting…" : "Gateway stopped"}
          </div>
          {status.data && (
            <div className="mono small">http://{status.data.gateway.addr}/v1</div>
          )}
          {status.data && <div className="small">v{status.data.version}</div>}
        </div>
      </aside>
      <main>
        {!running && !status.loading && (
          <div className="banner">
            <div>
              <strong>The Loadout gateway is not running.</strong>
              <div className="small muted">
                {notRunning
                  ? "Start it to manage accounts and serve requests. It keeps running when this window closes."
                  : status.error?.message}
              </div>
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
              {start.busy ? "Starting…" : "Start gateway"}
            </Button>
          </div>
        )}
        {running && status.data?.recovered_from && (
          <div className="banner">
            <div>
              <strong>The database was damaged and has been replaced by an empty one.</strong>
              <div className="small">
                The damaged file was kept at <span className="mono">{status.data.recovered_from}</span>. Restore your
                configuration under Settings → Backup → Import, using the newest file in{" "}
                <span className="mono">{status.data.data_dir}\backups\auto</span>.
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
