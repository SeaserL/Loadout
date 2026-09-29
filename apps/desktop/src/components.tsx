// SPDX-License-Identifier: Apache-2.0

import { type ReactNode, useEffect } from "react";
import { LANGS, type Lang, t, useLang } from "./i18n";

export function Page(props: { title: string; actions?: ReactNode; children: ReactNode }) {
  return (
    <section className="page">
      <header className="page-header">
        <h1>{props.title}</h1>
        <div className="actions">{props.actions}</div>
      </header>
      {props.children}
    </section>
  );
}

export function Card(props: { title?: ReactNode; actions?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <div className={`card ${props.className ?? ""}`}>
      {(props.title || props.actions) && (
        <div className="card-header">
          <h2>{props.title}</h2>
          <div className="actions">{props.actions}</div>
        </div>
      )}
      {props.children}
    </div>
  );
}

export function Button(props: {
  onClick?: () => void;
  children: ReactNode;
  kind?: "primary" | "danger" | "plain";
  disabled?: boolean;
  type?: "button" | "submit";
  title?: string;
}) {
  return (
    <button
      type={props.type ?? "button"}
      className={`btn ${props.kind ?? "plain"}`}
      onClick={props.onClick}
      disabled={props.disabled}
      title={props.title}
    >
      {props.children}
    </button>
  );
}

export function ErrorText(props: { error?: string | null }) {
  if (!props.error) return null;
  return <div className="error">{props.error}</div>;
}

export function Empty(props: { children: ReactNode }) {
  return <div className="empty">{props.children}</div>;
}

export function Badge(props: { tone?: "ok" | "warn" | "bad" | "info" | "muted"; children: ReactNode; title?: string }) {
  return (
    <span className={`badge ${props.tone ?? "muted"}`} title={props.title}>
      {props.children}
    </span>
  );
}

export function stateTone(state: string): "ok" | "warn" | "bad" | "muted" {
  switch (state) {
    case "HEALTHY":
      return "ok";
    case "UNKNOWN":
    case "RECOVERING":
      return "muted";
    case "DEGRADED":
    case "COOLDOWN":
    case "RATE_LIMITED":
      return "warn";
    default:
      return "bad";
  }
}

export function tierTone(tier: string): "ok" | "info" | "muted" | "warn" {
  switch (tier) {
    case "ULTRA_FAST":
      return "ok";
    case "FAST":
      return "info";
    case "NORMAL":
      return "warn";
    default:
      return "muted";
  }
}

export function Modal(props: { title: string; onClose: () => void; children: ReactNode; wide?: boolean }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") props.onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [props]);
  return (
    <div className="modal-backdrop" onMouseDown={props.onClose}>
      <div className={`modal ${props.wide ? "wide" : ""}`} onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2>{props.title}</h2>
          <button className="close" onClick={props.onClose} aria-label={t("common.close")}>
            ×
          </button>
        </div>
        <div className="modal-body">{props.children}</div>
      </div>
    </div>
  );
}

export function Field(props: { label: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <label className="field">
      <span className="label">{props.label}</span>
      {props.children}
      {props.hint && <span className="hint">{props.hint}</span>}
    </label>
  );
}

/** A 0–100 bar; `remaining` colours it by how much is left. */
export function Meter(props: { remaining: number | null; label?: string }) {
  if (props.remaining == null) return <span className="muted">-</span>;
  const used = Math.max(0, Math.min(100, 100 - props.remaining));
  const tone = props.remaining < 10 ? "bad" : props.remaining < 30 ? "warn" : "ok";
  return (
    <div className="meter" title={t("common.left", { n: props.remaining.toFixed(0) })}>
      <div className={`meter-fill ${tone}`} style={{ width: `${used}%` }} />
      <span className="meter-label">{props.label ?? t("common.left", { n: props.remaining.toFixed(0) })}</span>
    </div>
  );
}

export function copy(text: string) {
  void navigator.clipboard?.writeText(text);
}

export function LanguageSelect(props: { compact?: boolean }) {
  const { lang, setLang } = useLang();
  return (
    <select
      value={lang}
      onChange={(e) => setLang(e.target.value as Lang)}
      aria-label={t("shell.language")}
      className={props.compact ? "lang-select" : undefined}
    >
      {LANGS.map((l) => (
        <option key={l.id} value={l.id}>
          {l.label}
        </option>
      ))}
    </select>
  );
}
