// SPDX-License-Identifier: Apache-2.0
//
// Browser-session accounts on the Accounts page (ADR-014, ExecPlan 017 Stage 7): adding one
// with the two-step risk confirmation, its live state, and its settings.

import { useState } from "react";
import { AppWindow, CheckCheck, Settings2 } from "lucide-react";
import { api, type AccountInfo, type WebSessionStatus } from "../api";
import { Badge, Button, ErrorText, Field, Modal, Notice, type Tone } from "../components";
import { time } from "../format";
import { useAction, usePoll } from "../hooks";
import { t, type MessageKey } from "../i18n";

/** Built-in browser-session provider (ADR-014). */
export const WEB_SLUG = "web-chatgpt";
const ENABLE_KEY = "web_session.enabled";

const RISKS: MessageKey[] = ["web.risk1", "web.risk2", "web.risk3", "web.risk4", "web.risk5"];

export function WebRiskList() {
  return (
    <Notice tone="warn">
      <ul className="risk-list">
        {RISKS.map((k) => (
          <li key={k}>{t(k)}</li>
        ))}
      </ul>
    </Notice>
  );
}

/** Plain-text risk summary for confirmation prompts. */
export function webRiskText(): string {
  return RISKS.map((k) => `• ${t(k)}`).join("\n");
}

/** Adds a browser-session account: risks, checkbox, alias typed again (ADR-014 §8). */
export function WebAddDialog(props: { onClose: () => void; onDone: () => void }) {
  const action = useAction();
  const [alias, setAlias] = useState("main");
  const [understood, setUnderstood] = useState(false);
  const [typed, setTyped] = useState("");
  const [done, setDone] = useState(false);
  const ready = understood && alias.trim() !== "" && typed.trim() === alias.trim();
  const submit = () =>
    void action.run(async () => {
      const settings = await api.settings();
      if (settings.find((s) => s.key === ENABLE_KEY)?.value !== "true") {
        await api.setSetting(ENABLE_KEY, "true");
      }
      await api.webAdd(WEB_SLUG, alias.trim(), typed.trim());
      setDone(true);
      props.onDone();
    });
  return (
    <Modal
      title={t("web.addTitle")}
      onClose={props.onClose}
      footer={
        <>
          <Button onClick={props.onClose}>{done ? t("common.close") : t("common.cancel")}</Button>
          {!done && (
            <Button kind="danger" disabled={!ready || action.busy} onClick={submit}>
              {t("web.add")}
            </Button>
          )}
        </>
      }
    >
      {done ? (
        <Notice>{t("web.added")}</Notice>
      ) : (
        <>
          <WebRiskList />
          <Field label={t("acc.alias")} hint={t("acc.aliasHint")}>
            <input value={alias} onChange={(e) => setAlias(e.target.value.toLowerCase())} />
          </Field>
          <label className="row check-row">
            <input type="checkbox" checked={understood} onChange={(e) => setUnderstood(e.target.checked)} />
            <span>{t("web.understand")}</span>
          </label>
          <Field label={t("web.typeAlias", { alias: alias.trim() })} hint={t("web.enableFeature")}>
            <input
              value={typed}
              disabled={!understood}
              onChange={(e) => setTyped(e.target.value)}
              autoComplete="off"
            />
          </Field>
        </>
      )}
      <ErrorText error={action.error} />
    </Modal>
  );
}

function stateTone(s: WebSessionStatus): Tone {
  switch (s.state) {
    case "ready":
      return "ok";
    case "busy":
    case "starting":
      return "info";
    case "cooling":
    case "needs_attention":
      return "warn";
    default:
      return "muted";
  }
}

function stateLabel(s: WebSessionStatus): string {
  if (s.state === "stopped" && !s.enabled_globally) return t("web.off");
  if (s.state === "cooling") return t("web.state.cooling", { time: time(s.next_allowed_ms) });
  const known = ["ready", "busy", "starting", "stopped", "needs_attention"];
  return known.includes(s.state) ? t(`web.state.${s.state}` as MessageKey) : s.state;
}

/** Live state and actions of one browser-session account (polled). */
export function WebPanel(props: {
  account: AccountInfo;
  busy: boolean;
  onAction: (f: () => Promise<unknown>) => void;
  onSettings: (status: WebSessionStatus) => void;
}) {
  const a = props.account;
  const status = usePoll(() => api.webStatus(a.provider, a.alias), 5000);
  const s = status.data;
  const run = (f: () => Promise<unknown>) =>
    props.onAction(async () => {
      await f();
      await status.reload();
    });
  return (
    <div className="web-panel">
      <div className="row wrap" style={{ gap: 6 }}>
        <Badge tone="warn" title={t("web.badgeHint")}>
          {t("web.badge")}
        </Badge>
        {s && (
          <Badge tone={stateTone(s)} dot>
            {stateLabel(s)}
          </Badge>
        )}
        {s && <span className="small muted">{t("web.today", { n: s.today_count })}</span>}
      </div>
      {s?.state === "needs_attention" && s.attention && <Notice tone="warn">{s.attention}</Notice>}
      {s && (
        <div className="small muted ellipsis" title={s.default_conversation ?? undefined}>
          {t("web.conversation")}:{" "}
          {s.default_conversation ? (
            <span className="mono">
              {s.default_conversation}
              {s.bound_by === "auto" ? ` (${t("web.conversationAuto")})` : ""}
            </span>
          ) : (
            t("web.conversationNone")
          )}
        </div>
      )}
      <ErrorText error={status.error?.message} />
      <div className="row wrap" style={{ gap: 8 }}>
        <Button
          icon={<AppWindow size={16} />}
          disabled={props.busy || !s?.enabled_globally || !a.transport_opt_in || !a.enabled}
          onClick={() => run(() => api.webShow(a.provider, a.alias))}
        >
          {t("web.show")}
        </Button>
        {s?.state === "needs_attention" && (
          <Button icon={<CheckCheck size={16} />} disabled={props.busy} onClick={() => run(() => api.webRecheck(a.provider, a.alias))}>
            {t("web.recheck")}
          </Button>
        )}
        <Button icon={<Settings2 size={16} />} disabled={props.busy || !s} onClick={() => s && props.onSettings(s)}>
          {t("web.settings")}
        </Button>
      </div>
    </div>
  );
}

/** Conversation mode, limits and the default conversation. */
export function WebSettingsDialog(props: { status: WebSessionStatus; onClose: () => void; onDone: () => void }) {
  const s = props.status;
  const action = useAction();
  const [mode, setMode] = useState(s.mode);
  const [interval, setInterval] = useState(String(s.min_interval_secs));
  const [cap, setCap] = useState(String(s.daily_soft_cap));
  const [url, setUrl] = useState(s.default_conversation ?? "");
  const save = () =>
    void action.run(async () => {
      const n = (v: string) => {
        const x = Number(v.trim());
        if (!v.trim() || !Number.isInteger(x) || x < 0) throw new Error(t("web.errNumber"));
        return x;
      };
      await api.webConfig(s.provider, s.alias, {
        mode,
        min_interval_secs: n(interval),
        daily_soft_cap: n(cap),
      });
      const next = url.trim() || null;
      if (next !== (s.default_conversation ?? null)) {
        await api.webBind(s.provider, s.alias, next);
      }
      props.onDone();
      props.onClose();
    });
  return (
    <Modal
      title={t("web.settingsTitle", { name: `${s.provider}/${s.alias}` })}
      onClose={props.onClose}
      footer={
        <>
          <Button onClick={props.onClose}>{t("common.cancel")}</Button>
          <Button kind="primary" disabled={action.busy} onClick={save}>
            {t("common.save")}
          </Button>
        </>
      }
    >
      <Field label={t("web.mode")}>
        <select value={mode} onChange={(e) => setMode(e.target.value)}>
          <option value="persistent">{t("web.modePersistent")}</option>
          <option value="stateless">{t("web.modeStateless")}</option>
        </select>
      </Field>
      <Field label={t("web.minInterval")}>
        <input value={interval} onChange={(e) => setInterval(e.target.value)} inputMode="numeric" />
      </Field>
      <Field label={t("web.dailyCap")}>
        <input value={cap} onChange={(e) => setCap(e.target.value)} inputMode="numeric" />
      </Field>
      <Field label={t("web.bindUrl")} hint={t("web.bindHint")}>
        <input value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://chatgpt.com/c/…" />
      </Field>
      <ErrorText error={action.error} />
    </Modal>
  );
}
