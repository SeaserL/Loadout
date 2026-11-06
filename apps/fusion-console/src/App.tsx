// SPDX-License-Identifier: Apache-2.0

import { type ReactNode, useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  BookOpen,
  Boxes,
  Cable,
  ChartColumn,
  KeyRound,
  ScrollText,
  Settings as SettingsIcon,
  Sparkles,
  Spline,
  Waypoints,
} from "lucide-react";
import { api } from "./api";
import { HubMark, LockedPage, Notice, useCopy } from "./components";
import { usePoll } from "./hooks";
import { type MessageKey, t, tr, useLang } from "./i18n";
import { Dashboard } from "./pages/Dashboard";
import { Accounts } from "./pages/Accounts";
import { Models } from "./pages/Models";
import { Routes } from "./pages/Routes";
import { Clients } from "./pages/Clients";
import { Logs } from "./pages/Logs";
import { Stats } from "./pages/Stats";
import { Settings } from "./pages/Settings";
import { Features } from "./pages/Features";
import { Help } from "./pages/Help";

type NavItem = { id: string; label: MessageKey; icon: ReactNode };

const SIZE = 16;
// The middle group is the path of a request: strung on one line, in the order things get set up.
const OVERVIEW: NavItem = { id: "dashboard", label: "nav.dashboard", icon: <Spline size={SIZE} aria-hidden /> };
const PIPELINE: NavItem[] = [
  { id: "accounts", label: "nav.accounts", icon: <KeyRound size={SIZE} aria-hidden /> },
  { id: "models", label: "nav.models", icon: <Boxes size={SIZE} aria-hidden /> },
  { id: "routes", label: "nav.routes", icon: <Waypoints size={SIZE} aria-hidden /> },
  { id: "clients", label: "nav.clients", icon: <Cable size={SIZE} aria-hidden /> },
];
const WATCH: NavItem[] = [
  { id: "stats", label: "nav.stats", icon: <ChartColumn size={SIZE} aria-hidden /> },
  { id: "logs", label: "nav.logs", icon: <ScrollText size={SIZE} aria-hidden /> },
];
const FOOT: NavItem[] = [
  { id: "features", label: "nav.features", icon: <Sparkles size={SIZE} aria-hidden /> },
  { id: "help", label: "nav.help", icon: <BookOpen size={SIZE} aria-hidden /> },
  { id: "settings", label: "nav.settings", icon: <SettingsIcon size={SIZE} aria-hidden /> },
];
const ALL = [OVERVIEW, ...PIPELINE, ...WATCH, ...FOOT];

function currentRoute(): { page: string; section?: string } {
  const [id, section] = window.location.hash.replace(/^#\/?/, "").split("/");
  return ALL.some((p) => p.id === id) ? { page: id as string, section } : { page: "dashboard" };
}

function NavLink(props: { item: NavItem; page: string; dot?: boolean }) {
  const on = props.item.id === props.page;
  const label = t(props.item.label);
  return (
    <a href={`#/${props.item.id}`} className={on ? "active" : ""} aria-current={on ? "page" : undefined} title={label}>
      <span className="nav-icon">{props.item.icon}</span>
      <span className="nav-label">{label}</span>
      {props.dot && <span className="nav-dot" aria-label={t("update.available", { version: "" })} />}
    </a>
  );
}

export function App() {
  // Re-render everything when the language changes.
  useLang();
  const [route, setRoute] = useState(currentRoute);
  const status = usePoll(api.status, 3000);
  const update = usePoll(api.updateStatus, 60000);
  const newVersion = update.data?.phase === "available" || update.data?.phase === "ready";
  const copy = useCopy();
  const page = route.page;
  const sidebar = useRef<HTMLElement>(null);
  const marker = useRef<HTMLSpanElement>(null);

  // One marker that slides between the links, so the eye follows the change of page.
  useLayoutEffect(() => {
    const place = () => {
      const link = sidebar.current?.querySelector<HTMLElement>(".nav a.active");
      if (!link || !sidebar.current || !marker.current) return;
      const top = link.getBoundingClientRect().top - sidebar.current.getBoundingClientRect().top + sidebar.current.scrollTop;
      marker.current.style.transform = `translateY(${top}px)`;
      marker.current.style.height = `${link.offsetHeight}px`;
      marker.current.style.opacity = "1";
    };
    place();
    void document.fonts?.ready.then(place);
    window.addEventListener("resize", place);
    return () => window.removeEventListener("resize", place);
  }, [page]);

  useEffect(() => {
    const onHash = () => {
      setRoute(currentRoute());
      // A new page starts at the top, not where the previous one was scrolled to, and the
      // keyboard continues from the page rather than from the link that opened it.
      const main = document.querySelector("main");
      main?.scrollTo(0, 0);
      main?.focus({ preventScroll: true });
    };
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  const running = !!status.data && !status.error;
  const noSession = status.error?.code === "no_session";
  const locked = !running && !status.loading;
  const current = ALL.find((p) => p.id === page);
  const addr = status.data?.gateway.addr;

  return (
    <div className="app">
      <aside className="sidebar" ref={sidebar}>
        <span className="nav-marker" ref={marker} aria-hidden />
        <a className="brand" href="#/dashboard" aria-label="Loadout">
          <HubMark />
          <span className="brand-name">Loadout</span>
        </a>
        <nav className="nav" aria-label={t("shell.nav")}>
          <NavLink item={OVERVIEW} page={page} />
          <div className="nav-pipeline">
            {PIPELINE.map((p) => (
              <NavLink key={p.id} item={p} page={page} />
            ))}
          </div>
          {WATCH.map((p) => (
            <NavLink key={p.id} item={p} page={page} />
          ))}
        </nav>
        <div className="spacer" />
        <nav className="nav" aria-label={t("shell.nav")}>
          {FOOT.map((p) => (
            <NavLink key={p.id} item={p} page={page} dot={p.id === "settings" && newVersion} />
          ))}
        </nav>
        <button
          type="button"
          className="gateway-line"
          disabled={!addr}
          title={running ? t("shell.running") : t("shell.stopped")}
          onClick={() => addr && copy(`http://${addr}/v1`)}
        >
          <span className={`dot ${running ? "ok" : status.loading ? "" : "bad"}`} />
          <span className="nav-label ellipsis">{addr ?? t("shell.connecting")}</span>
        </button>
      </aside>
      <main tabIndex={-1}>
        {locked && (
          <LockedPage
            title={current ? t(current.label) : t("nav.settings")}
            hint={noSession ? t("shell.noSession") : t("shell.notRunningHint")}
            action={null}
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
          <div className="page-stage" key={page}>
            {page === "dashboard" && <Dashboard status={status.data} />}
            {page === "accounts" && <Accounts />}
            {page === "models" && <Models />}
            {page === "routes" && <Routes gateway={status.data.gateway.addr} />}
            {page === "clients" && <Clients gateway={status.data.gateway.addr} />}
            {page === "stats" && <Stats day={route.section} />}
            {page === "logs" && <Logs />}
            {page === "features" && <Features />}
            {page === "help" && <Help section={route.section} gateway={status.data.gateway.addr} />}
            {page === "settings" && <Settings status={status.data} onDaemonChange={status.reload} />}
          </div>
        )}
      </main>
    </div>
  );
}
