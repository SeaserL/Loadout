// SPDX-License-Identifier: Apache-2.0
//
// Account health on the accounts page and dedicated endpoints such as the SenseNova unified
// API (ExecPlan 021, ADR-015). Types and calls live here, next to their only users.

import { useState } from "react";
import { Link2 } from "lucide-react";
import { type AccountInfo, type TargetStatus, call } from "../api";
import { Badge, Button, CopyField, ErrorText, Field, Modal, Notice, Switch, useConfirm } from "../components";
import { duration, shortTime } from "../format";
import { useAction } from "../hooks";
import { t, type MessageKey } from "../i18n";

export interface EndpointInfo {
  name: string;
  provider: string;
  enabled: boolean;
  /** Aliases; null = every account of the provider in the current pool. */
  members: string[] | null;
  default_model: string | null;
  wait_secs: number;
  /** Routable now, in failover order. */
  active_accounts: string[];
  models: string[];
  openai_base_url: string;
  anthropic_base_url: string;
}

export const endpointApi = {
  list: () => call<EndpointInfo[]>("endpoint.list"),
  save: (p: {
    name: string | null;
    provider: string;
    enabled: boolean;
    members: string[] | null;
    default_model: string | null;
    wait_secs: number;
  }) => call<EndpointInfo>("endpoint.save", p),
  remove: (name: string) => call<{ revoked: boolean }>("endpoint.remove", { name }),
};

// ---- account health ------------------------------------------------------------------------

/** States worse first; RECOVERING = the wait ended, the next request or probe decides. */
const SEVERITY: Record<string, number> = {
  AUTH_FAILED: 5,
  QUOTA_EXHAUSTED: 4,
  RATE_LIMITED: 3,
  COOLDOWN: 2,
  RECOVERING: 1,
};
/** How long "recovered at …" stays on the card. */
const RECOVERED_SHOWN_MS = 10 * 60_000;

export interface AccountHealth {
  state: string;
  /** Upstream model of the worst target. */
  model: string;
  /** Other models in a blocking state. */
  others: number;
  until: number | null;
  error: string | null;
  recoveredAt: number | null;
}

/** The worst health state among an account's routes, or a recent recovery. */
export function accountHealth(statuses: TargetStatus[] | undefined, a: AccountInfo): AccountHealth | undefined {
  const prefix = `${a.provider}/${a.alias}/`;
  const mine = (statuses ?? []).filter((s) => s.name.startsWith(prefix));
  const rank = (s: TargetStatus) => SEVERITY[s.state] ?? 0;
  const bad = mine.filter((s) => rank(s) > 0).sort((x, y) => rank(y) - rank(x));
  const models = new Set(bad.map((s) => s.name.slice(prefix.length)));
  const recovered = Math.max(0, ...mine.map((s) => s.recovered_at_ms ?? 0));
  const w = bad[0];
  if (w) {
    return {
      state: w.state,
      model: w.name.slice(prefix.length),
      others: models.size - 1,
      until: w.unavailable_until_ms,
      error: w.last_error,
      recoveredAt: null,
    };
  }
  if (recovered > 0 && Date.now() - recovered < RECOVERED_SHOWN_MS) {
    return { state: "RECOVERED", model: "", others: 0, until: null, error: null, recoveredAt: recovered };
  }
  return undefined;
}

function secondsLeft(ms: number | null): string {
  if (ms == null) return "";
  const s = Math.max(0, Math.ceil((ms - Date.now()) / 1000));
  if (s < 120) return t("hl.inSec", { n: s });
  return t("hl.inDur", { d: duration(s * 1000) });
}

export function HealthBadge(props: { health: AccountHealth | undefined }) {
  const h = props.health;
  if (!h) return null;
  if (h.state === "RECOVERED") {
    return <Badge tone="ok">{t("hl.recovered", { time: shortTime(h.recoveredAt) })}</Badge>;
  }
  const label = t(`hl.${h.state}` as MessageKey);
  const parts = [label, h.model];
  if (h.others > 0) parts.push(t("hl.more", { n: h.others }));
  if (h.until != null) parts.push(secondsLeft(h.until));
  if (h.state === "AUTH_FAILED" && h.until == null) parts.push(t("hl.needsYou"));
  const tone = h.state === "AUTH_FAILED" || h.state === "QUOTA_EXHAUSTED" ? "bad" : "warn";
  const text = parts.filter(Boolean).join(" · ");
  return (
    <div className="health-line">
      <Badge tone={tone} title={text} dot>
        <span className="ellipsis">{text}</span>
      </Badge>
      {h.error && (
        <span className="small muted ellipsis" title={h.error}>
          {h.error}
        </span>
      )}
    </div>
  );
}

// ---- dedicated endpoints -------------------------------------------------------------------

export function EndpointButton(props: { endpoint: EndpointInfo | undefined; onOpen: () => void }) {
  const on = props.endpoint?.enabled;
  return (
    <Button icon={<Link2 size={16} />} kind={on ? "primary" : undefined} onClick={props.onOpen}>
      {on ? t("ep.on", { n: props.endpoint?.active_accounts.length ?? 0 }) : t("ep.button")}
    </Button>
  );
}

export function EndpointDialog(props: {
  provider: { slug: string; display_name: string };
  /** The provider's accounts. */
  accounts: AccountInfo[];
  endpoint: EndpointInfo | undefined;
  statuses: TargetStatus[] | undefined;
  onClose: () => void;
  onDone: () => void;
}) {
  const action = useAction();
  const confirm = useConfirm();
  const e = props.endpoint;
  const [enabled, setEnabled] = useState(e?.enabled ?? true);
  const [auto, setAuto] = useState(e ? e.members == null : true);
  const [picked, setPicked] = useState<Set<string>>(
    new Set(e?.members ?? props.accounts.filter((a) => a.enabled).map((a) => a.alias)),
  );
  const [defaultModel, setDefaultModel] = useState(e?.default_model ?? "");
  const [wait, setWait] = useState(String(e?.wait_secs ?? 30));
  const [saved, setSaved] = useState<EndpointInfo | undefined>(e);

  const toggle = (alias: string) => {
    const next = new Set(picked);
    if (next.has(alias)) next.delete(alias);
    else next.add(alias);
    setPicked(next);
  };
  const save = () =>
    void action.run(async () => {
      const w = Number(wait);
      if (!Number.isInteger(w) || w < 0 || w > 300) throw new Error(t("ep.errWait"));
      if (!auto && picked.size === 0) throw new Error(t("ep.errMembers"));
      const r = await endpointApi.save({
        name: e?.name ?? null,
        provider: props.provider.slug,
        enabled,
        members: auto ? null : [...picked],
        default_model: defaultModel || null,
        wait_secs: w,
      });
      setSaved(r);
      props.onDone();
    });
  const remove = () =>
    void (async () => {
      if (!e) return;
      const ok = await confirm({
        title: t("ep.remove"),
        message: t("ep.removeConfirm", { name: e.name }),
        confirmLabel: t("ep.remove"),
        danger: true,
      });
      if (!ok) return;
      await action.run(async () => {
        await endpointApi.remove(e.name);
        props.onDone();
        props.onClose();
      });
    })();
  const shown = saved ?? e;

  return (
    <Modal
      title={t("ep.title", { name: props.provider.display_name })}
      onClose={props.onClose}
      wide
      footer={
        <>
          {e && (
            <Button kind="danger" disabled={action.busy} onClick={remove}>
              {t("ep.remove")}
            </Button>
          )}
          <Button onClick={props.onClose}>{t("common.close")}</Button>
          <Button kind="primary" disabled={action.busy} onClick={save}>
            {e ? t("common.save") : t("ep.create")}
          </Button>
        </>
      }
    >
      <Notice>{t("ep.intro", { provider: props.provider.display_name })}</Notice>
      <Switch checked={enabled} label={t("ep.enabled")} onChange={setEnabled} />
      <span className="small"> {t("ep.enabled")}</span>

      {shown && shown.enabled && (
        <div className="endpoint-urls">
          <Field label={t("ep.openaiUrl")}>
            <CopyField value={shown.openai_base_url} />
          </Field>
          <Field label={t("ep.anthropicUrl")} hint={t("ep.keyHint")}>
            <CopyField value={shown.anthropic_base_url} />
          </Field>
          <div className="small muted">
            {t("ep.models")}: <span className="mono">{shown.models.join(", ") || "—"}</span>
          </div>
        </div>
      )}

      <div className="strong" style={{ marginTop: 12 }}>
        {t("ep.members")}
      </div>
      <label className="row check-row">
        <input type="radio" checked={auto} onChange={() => setAuto(true)} />
        <span>{t("ep.membersAuto", { provider: props.provider.display_name })}</span>
      </label>
      <label className="row check-row">
        <input type="radio" checked={!auto} onChange={() => setAuto(false)} />
        <span>{t("ep.membersPick")}</span>
      </label>
      <div className="endpoint-members">
        {props.accounts.map((a) => {
          const order = shown?.active_accounts.indexOf(a.alias) ?? -1;
          const health = accountHealth(props.statuses, a);
          return (
            <label key={a.alias} className="row check-row">
              <input
                type="checkbox"
                disabled={auto}
                checked={auto ? true : picked.has(a.alias)}
                onChange={() => toggle(a.alias)}
              />
              <span className="mono">{a.alias}</span>
              {order >= 0 && <Badge>{t("ep.order", { n: order + 1 })}</Badge>}
              {shown?.enabled && order < 0 && <Badge>{t("ep.notRoutable")}</Badge>}
              <HealthBadge health={health} />
            </label>
          );
        })}
      </div>

      <Field label={t("ep.defaultModel")} hint={t("ep.defaultModelHint")}>
        <select value={defaultModel} onChange={(ev) => setDefaultModel(ev.target.value)}>
          <option value="">{t("ep.noDefault")}</option>
          {(shown?.models ?? []).map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </select>
      </Field>
      <Field label={t("ep.wait")} hint={t("ep.waitHint")}>
        <input type="number" min={0} max={300} value={wait} onChange={(ev) => setWait(ev.target.value)} />
      </Field>
      <ErrorText error={action.error} />
    </Modal>
  );
}
