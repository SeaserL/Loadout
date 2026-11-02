// SPDX-License-Identifier: Apache-2.0

import {
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
  type RefObject,
  createContext,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { Check, CircleAlert, CircleCheck, CircleHelp, Copy, Info, Languages, Lock, Monitor, Moon, Palette, Sun, X } from "lucide-react";
import { useRolling } from "./hooks";
import { LANGS, type Lang, type MessageKey, t, useLang } from "./i18n";
import { PALETTES, type ThemePreference, useMotion, usePalette, useTheme } from "./theme";

// ---- layout -------------------------------------------------------------------------------

/** The page's own chapter in the guide: explanations live there, not on the page. */
export function HelpLink(props: { section: string }) {
  return (
    <a className="help-link" href={`#/help/${props.section}`} title={t("nav.help")} aria-label={t("nav.help")}>
      <CircleHelp size={15} aria-hidden />
    </a>
  );
}

export function Page(props: {
  title: string;
  actions?: ReactNode;
  toolbar?: ReactNode;
  children: ReactNode;
  /** Forms and long text: keep lines readable on wide windows. */
  narrow?: boolean;
  help?: string;
}) {
  return (
    <section className={props.narrow ? "page narrow" : "page"}>
      <header className="page-header">
        <h1>{props.title}</h1>
        {props.help && <HelpLink section={props.help} />}
        {props.toolbar}
        <div className="spacer" />
        <div className="actions">{props.actions}</div>
      </header>
      {props.children}
    </section>
  );
}

export function Card(props: {
  title?: ReactNode;
  subtitle?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={`card ${props.className ?? ""}`}>
      {(props.title || props.actions) && (
        <div className="card-header">
          <div className="card-title">
            <h2>{props.title}</h2>
            {props.subtitle && <div className="card-subtitle">{props.subtitle}</div>}
          </div>
          <div className="actions">{props.actions}</div>
        </div>
      )}
      {props.children}
    </div>
  );
}

/** A value that rolls from what it was to what it is. */
export function Rolling(props: { value: number; format: (v: number) => string }) {
  return <>{props.format(useRolling(props.value))}</>;
}

/** A single-value tile for a KPI row. */
export function Kpi(props: { label: string; value: ReactNode; sub?: ReactNode; tone?: "bad" | "warn" }) {
  return (
    <div className={`kpi ${props.tone ?? ""}`}>
      <div className="kpi-label">{props.label}</div>
      <div className="kpi-value">{props.value}</div>
      {props.sub != null && <div className="kpi-sub">{props.sub}</div>}
    </div>
  );
}

/** A labelled control inside a filter bar. */
export function FilterField(props: { label: string; children: ReactNode; grow?: boolean }) {
  return (
    <label className={`filter-field ${props.grow ? "grow" : ""}`}>
      <span>{props.label}</span>
      {props.children}
    </label>
  );
}

export function FilterBar(props: { children: ReactNode }) {
  return <div className="filter-bar">{props.children}</div>;
}

// ---- controls -----------------------------------------------------------------------------

export function Button(props: {
  onClick?: () => void;
  children: ReactNode;
  kind?: "primary" | "danger" | "plain";
  icon?: ReactNode;
  disabled?: boolean;
  type?: "button" | "submit";
  title?: string;
  autoFocus?: boolean;
}) {
  return (
    <button
      type={props.type ?? "button"}
      className={`btn ${props.kind ?? "plain"}`}
      onClick={props.onClick}
      disabled={props.disabled}
      title={props.title}
      data-autofocus={props.autoFocus ? "" : undefined}
    >
      {props.icon}
      {props.children}
    </button>
  );
}

/** A square icon-only button; `label` is its tooltip and accessible name. */
export function IconButton(props: {
  label: string;
  onClick?: () => void;
  children: ReactNode;
  danger?: boolean;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      className={`icon-btn ${props.danger ? "danger" : ""}`}
      onClick={props.onClick}
      disabled={props.disabled}
      title={props.label}
      aria-label={props.label}
    >
      {props.children}
    </button>
  );
}

export function Switch(props: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  label: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={props.checked}
      aria-label={props.label}
      title={props.label}
      className={`switch ${props.checked ? "on" : ""}`}
      disabled={props.disabled}
      onClick={() => props.onChange(!props.checked)}
    >
      <span className="knob" />
    </button>
  );
}

export function Segmented<T extends string | number>(props: {
  value: T;
  options: { value: T; label: ReactNode; title?: string }[];
  onChange: (value: T) => void;
  label: string;
  className?: string;
}) {
  return (
    <div className={`segmented ${props.className ?? ""}`} role="radiogroup" aria-label={props.label}>
      {props.options.map((o) => (
        <button
          key={String(o.value)}
          type="button"
          role="radio"
          aria-checked={o.value === props.value}
          className={o.value === props.value ? "active" : ""}
          title={o.title}
          onClick={() => props.onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function ErrorText(props: { error?: string | null }) {
  if (!props.error) return null;
  return (
    <div className="error" role="alert">
      <CircleAlert size={16} aria-hidden />
      <span>{props.error}</span>
    </div>
  );
}

export function Notice(props: { children: ReactNode; tone?: "info" | "warn" }) {
  return (
    <div className={`notice ${props.tone ?? "info"}`}>
      <Info size={16} aria-hidden />
      <div>{props.children}</div>
    </div>
  );
}

export function Empty(props: { children: ReactNode; action?: ReactNode }) {
  return (
    <div className="empty">
      <div>{props.children}</div>
      {props.action && <div className="empty-action">{props.action}</div>}
    </div>
  );
}

export type Tone = "ok" | "warn" | "bad" | "info" | "muted" | "accent";

/** A status pill. */
export function Badge(props: { tone?: Tone; children: ReactNode; title?: string; dot?: boolean }) {
  return (
    <span className={`badge ${props.tone ?? "muted"}`} title={props.title}>
      {props.dot && <span className="badge-dot" />}
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

export function Field(props: { label: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <label className="field">
      <span className="label">{props.label}</span>
      {props.children}
      {props.hint && <span className="hint">{props.hint}</span>}
    </label>
  );
}

/** Tone for a share that is left: green, amber under 30 %, red under 10 %. */
export function remainingTone(remaining: number): "ok" | "warn" | "bad" {
  return remaining < 10 ? "bad" : remaining < 30 ? "warn" : "ok";
}

/** A quota window: label, "% left" in colour, reset time and a bar of what is left. */
export function QuotaBar(props: {
  label: ReactNode;
  remaining: number | null;
  detail?: ReactNode;
  resets?: string | null;
}) {
  const r = props.remaining;
  const tone = r == null ? "muted" : remainingTone(r);
  return (
    <div className="quota">
      <div className="quota-row">
        <span className="quota-label">{props.label}</span>
        {props.detail && <span className="quota-detail">{props.detail}</span>}
        <span className="spacer" />
        <span className={`quota-left ${tone}`}>{r == null ? "-" : t("common.left", { n: r.toFixed(0) })}</span>
        {props.resets && <span className="quota-reset">{props.resets}</span>}
      </div>
      <div className="quota-track" title={r == null ? undefined : t("common.left", { n: r.toFixed(0) })}>
        <div className={`quota-fill ${tone}`} style={{ width: `${r == null ? 0 : Math.max(0, Math.min(100, r))}%` }} />
      </div>
    </div>
  );
}

/** A horizontal share bar (e.g. requests per group). */
export function ShareBar(props: { value: number; max: number; tone?: string }) {
  const pct = props.max > 0 ? (props.value / props.max) * 100 : 0;
  return (
    <div className="share-track">
      <div className={`share-fill ${props.tone ?? ""}`} style={{ width: `${pct}%` }} />
    </div>
  );
}

// ---- clipboard / toast --------------------------------------------------------------------

type ToastTone = "ok" | "bad" | "info";
interface ToastItem {
  id: number;
  message: string;
  tone: ToastTone;
}

const ToastContext = createContext<(message: string, tone?: ToastTone) => void>(() => undefined);

export function ToastProvider(props: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);
  const next = useRef(1);
  const show = useCallback((message: string, tone: ToastTone = "ok") => {
    const id = next.current++;
    setItems((cur) => [...cur.slice(-2), { id, message, tone }]);
    window.setTimeout(() => setItems((cur) => cur.filter((x) => x.id !== id)), 2800);
  }, []);
  return (
    <ToastContext.Provider value={show}>
      {props.children}
      <div className="toasts" role="status" aria-live="polite">
        {items.map((i) => (
          <div key={i.id} className={`toast ${i.tone}`}>
            {i.tone === "bad" ? <CircleAlert size={16} aria-hidden /> : <CircleCheck size={16} aria-hidden />}
            <span>{i.message}</span>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast() {
  return useContext(ToastContext);
}

export async function copy(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

/** Copies `text` and confirms with a toast. */
export function useCopy() {
  const toast = useToast();
  return useCallback(
    (text: string) =>
      void copy(text).then((ok) => toast(ok ? t("common.copied") : t("common.copyFailed"), ok ? "ok" : "bad")),
    [toast],
  );
}

/** A read-only monospace value with a copy button. */
export function CopyField(props: { value: string; label?: string; secret?: boolean }) {
  const doCopy = useCopy();
  const [done, setDone] = useState(false);
  useEffect(() => {
    if (!done) return;
    const id = window.setTimeout(() => setDone(false), 1500);
    return () => window.clearTimeout(id);
  }, [done]);
  return (
    <div className={`copy-field ${props.secret ? "secret" : ""}`}>
      <span className="copy-value mono" title={props.value}>
        {props.value}
      </span>
      <IconButton
        label={props.label ?? t("common.copy")}
        onClick={() => {
          doCopy(props.value);
          setDone(true);
        }}
      >
        {done ? <Check size={16} /> : <Copy size={16} />}
      </IconButton>
    </div>
  );
}

// ---- dialogs ------------------------------------------------------------------------------

const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** Keeps keyboard focus inside `ref` while mounted, and restores it afterwards. */
function useFocusTrap(ref: RefObject<HTMLDivElement | null>) {
  useLayoutEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const node = ref.current;
    if (node) {
      const preferred =
        node.querySelector<HTMLElement>("[data-autofocus]") ??
        node.querySelector<HTMLElement>(
          ".modal-body input:not([disabled]), .modal-body select:not([disabled]), .modal-body textarea:not([disabled])",
        );
      (preferred ?? node).focus();
    }
    return () => {
      if (previous && document.contains(previous)) previous.focus();
    };
  }, [ref]);
}

function trapKeys(e: ReactKeyboardEvent<HTMLDivElement>, node: HTMLDivElement | null, onClose: () => void) {
  if (e.key === "Escape") {
    e.stopPropagation();
    onClose();
    return;
  }
  if (e.key !== "Tab" || !node) return;
  const items = [...node.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((el) => el.offsetParent !== null);
  if (items.length === 0) {
    e.preventDefault();
    return;
  }
  const first = items[0] as HTMLElement;
  const last = items[items.length - 1] as HTMLElement;
  if (e.shiftKey && (document.activeElement === first || document.activeElement === node)) {
    e.preventDefault();
    last.focus();
  } else if (!e.shiftKey && document.activeElement === last) {
    e.preventDefault();
    first.focus();
  }
}

export function Modal(props: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  wide?: boolean;
  footer?: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useFocusTrap(ref);
  return (
    <div
      className="modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) props.onClose();
      }}
    >
      <div
        ref={ref}
        className={`modal ${props.wide ? "wide" : ""}`}
        role="dialog"
        aria-modal="true"
        aria-label={props.title}
        tabIndex={-1}
        onKeyDown={(e) => trapKeys(e, ref.current, props.onClose)}
      >
        <div className="modal-header">
          <h2>{props.title}</h2>
          <button type="button" className="icon-btn" onClick={props.onClose} aria-label={t("common.close")}>
            <X size={18} />
          </button>
        </div>
        <div className="modal-body">{props.children}</div>
        {props.footer && <div className="modal-footer">{props.footer}</div>}
      </div>
    </div>
  );
}

export interface ConfirmOptions {
  title: string;
  message?: ReactNode;
  confirmLabel?: string;
  danger?: boolean;
}

const ConfirmContext = createContext<(o: ConfirmOptions) => Promise<boolean>>(async () => false);

/** Hosts the confirmation dialog; `useConfirm()` opens it and resolves with the answer. */
export function ConfirmProvider(props: { children: ReactNode }) {
  const [pending, setPending] = useState<{ options: ConfirmOptions; resolve: (ok: boolean) => void }>();
  const ask = useCallback(
    (options: ConfirmOptions) =>
      new Promise<boolean>((resolve) => {
        setPending({ options, resolve });
      }),
    [],
  );
  const answer = (ok: boolean) => {
    pending?.resolve(ok);
    setPending(undefined);
  };
  return (
    <ConfirmContext.Provider value={ask}>
      {props.children}
      {pending && <ConfirmDialog options={pending.options} onAnswer={answer} />}
    </ConfirmContext.Provider>
  );
}

export function useConfirm() {
  return useContext(ConfirmContext);
}

function ConfirmDialog(props: { options: ConfirmOptions; onAnswer: (ok: boolean) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  useFocusTrap(ref);
  const o = props.options;
  return (
    <div
      className="modal-backdrop top"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) props.onAnswer(false);
      }}
    >
      <div
        ref={ref}
        className="modal confirm"
        role="alertdialog"
        aria-modal="true"
        aria-label={o.title}
        tabIndex={-1}
        onKeyDown={(e) => trapKeys(e, ref.current, () => props.onAnswer(false))}
      >
        <div className="confirm-body">
          <div className={`confirm-icon ${o.danger ? "danger" : ""}`}>
            <CircleAlert size={20} aria-hidden />
          </div>
          <div>
            <h2>{o.title}</h2>
            {o.message && <div className="confirm-message">{o.message}</div>}
          </div>
        </div>
        <div className="modal-footer">
          <Button onClick={() => props.onAnswer(false)} autoFocus>
            {t("common.cancel")}
          </Button>
          <Button kind={o.danger ? "danger" : "primary"} onClick={() => props.onAnswer(true)}>
            {o.confirmLabel ?? t("common.confirm")}
          </Button>
        </div>
      </div>
    </div>
  );
}

// ---- shell pieces -------------------------------------------------------------------------

/** Shown instead of a page while the daemon is not running. */
export function LockedPage(props: { title: string; hint: ReactNode; action: ReactNode; error?: string | null }) {
  return (
    <section className="page">
      <header className="page-header">
        <h1>{props.title}</h1>
      </header>
      <div className="locked-panel">
        <div className="locked-icon">
          <Lock size={22} aria-hidden />
        </div>
        <h2>{t("shell.notRunning")}</h2>
        <p>{props.hint}</p>
        <ErrorText error={props.error} />
        {props.action}
      </div>
    </section>
  );
}

export function ThemeSwitch() {
  const { preference, setPreference } = useTheme();
  const options: { value: ThemePreference; label: ReactNode; title: string }[] = [
    { value: "light", label: <Sun size={15} aria-hidden />, title: t("theme.light") },
    { value: "dark", label: <Moon size={15} aria-hidden />, title: t("theme.dark") },
    { value: "system", label: <Monitor size={15} aria-hidden />, title: t("theme.system") },
  ];
  return (
    <Segmented
      className="theme-switch"
      label={t("theme.label")}
      value={preference}
      onChange={setPreference}
      options={options.map((o) => ({
        ...o,
        label: (
          <>
            {o.label}
            <span>{o.title}</span>
          </>
        ),
      }))}
    />
  );
}

/** The gateway: three slabs of one stack, drawn with depth. The stack opens while traffic passes. */
export function HubMark() {
  const slab = (y: number, cls: string, opacity: number) => (
    <g className={`slab ${cls}`} opacity={opacity}>
      <path d={`M12 ${y} L32 ${y + 10} V${y + 15} L12 ${y + 5} Z`} fill="url(#hub-left)" stroke="url(#hub-left)" strokeWidth="1.6" strokeLinejoin="round" />
      <path d={`M52 ${y} L32 ${y + 10} V${y + 15} L52 ${y + 5} Z`} fill="url(#hub-right)" stroke="url(#hub-right)" strokeWidth="1.6" strokeLinejoin="round" />
      <path d={`M32 ${y - 10} L52 ${y} L32 ${y + 10} L12 ${y} Z`} fill="url(#hub-top)" stroke="url(#hub-top)" strokeWidth="1.6" strokeLinejoin="round" />
      <path d={`M13.5 ${y - 0.4} L32 ${y - 9.6} L44 ${y - 3.6}`} fill="none" stroke="#ffffff" strokeOpacity="0.45" strokeWidth="1" strokeLinecap="round" strokeLinejoin="round" />
    </g>
  );
  return (
    <svg className="hub-mark" viewBox="0 0 64 64" aria-hidden>
      <defs>
        <linearGradient id="hub-top" x1="0.1" y1="0" x2="0.9" y2="1">
          <stop offset="0" style={{ stopColor: "var(--hub-a)" }} />
          <stop offset="1" style={{ stopColor: "var(--hub-b)" }} />
        </linearGradient>
        <linearGradient id="hub-left" x1="0" y1="0" x2="1" y2="0">
          <stop offset="0" style={{ stopColor: "var(--hub-b)" }} />
          <stop offset="1" style={{ stopColor: "var(--hub-c)" }} />
        </linearGradient>
        <linearGradient id="hub-right" x1="1" y1="0" x2="0" y2="0">
          <stop offset="0" style={{ stopColor: "var(--hub-b)" }} />
          <stop offset="1" style={{ stopColor: "var(--hub-c)" }} />
        </linearGradient>
      </defs>
      {slab(38, "slab-low", 0.62)}
      {slab(29, "slab-mid", 0.82)}
      {slab(20, "slab-top", 1)}
    </svg>
  );
}

/** Each preset drawn in its own colours, so choosing is seeing. */
export function PaletteGrid() {
  const { palette, setPalette } = usePalette();
  return (
    <div className="palette-grid" role="radiogroup" aria-label={t("settings.palette")}>
      {PALETTES.map((id) => (
        <button
          key={id}
          type="button"
          role="radio"
          aria-checked={palette === id}
          className={`palette-card ${palette === id ? "on" : ""}`}
          data-palette={id}
          onClick={() => setPalette(id)}
        >
          <svg viewBox="0 0 96 34" aria-hidden>
            <path d="M2 6 C26 6 28 17 48 17" stroke="var(--flow-1)" />
            <path d="M2 17 L48 17" stroke="var(--flow-2)" />
            <path d="M2 28 C26 28 28 17 48 17" stroke="var(--flow-3)" />
            <path d="M48 17 C68 17 70 8 94 8" stroke="var(--flow-4)" />
            <path d="M48 17 C68 17 70 26 94 26" stroke="var(--flow-5)" />
            <circle cx="48" cy="17" r="6" fill="var(--accent)" stroke="none" />
          </svg>
          {t(`palette.${id}` as MessageKey)}
        </button>
      ))}
    </div>
  );
}

/** Everything that moves answers to this one switch. */
export function MotionSwitch() {
  const { on, setMotion } = useMotion();
  return (
    <span className="check-row small">
      <Switch checked={on} label={t("settings.motion")} onChange={setMotion} />
      {t("settings.motion")}
    </span>
  );
}

/** Colours and light or dark, reachable from any page so they can be judged on any page. */
export function AppearanceMenu() {
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const away = (e: PointerEvent) => {
      if (!box.current?.contains(e.target as globalThis.Node)) setOpen(false);
    };
    const esc = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("pointerdown", away);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("pointerdown", away);
      window.removeEventListener("keydown", esc);
    };
  }, [open]);
  return (
    <div className="appearance" ref={box}>
      <button
        type="button"
        className="icon-btn"
        title={t("settings.appearance")}
        aria-label={t("settings.appearance")}
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        <Palette size={15} aria-hidden />
      </button>
      {open && (
        <div className="appearance-pop">
          <PaletteGrid />
          <ThemeSwitch />
          <MotionSwitch />
        </div>
      )}
    </div>
  );
}

export function LanguageSelect(props: { compact?: boolean }) {
  const { lang, setLang } = useLang();
  const select = (
    <select value={lang} onChange={(e) => setLang(e.target.value as Lang)} aria-label={t("shell.language")}>
      {LANGS.map((l) => (
        <option key={l.id} value={l.id}>
          {l.label}
        </option>
      ))}
    </select>
  );
  if (!props.compact) return select;
  return (
    <label className="lang-select">
      <Languages size={16} aria-hidden />
      {select}
    </label>
  );
}
