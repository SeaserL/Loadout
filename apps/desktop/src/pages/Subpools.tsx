// SPDX-License-Identifier: Apache-2.0
//
// Account sub-pools (ExecPlan 019) and the encrypted export / import of every API key
// (ExecPlan 020). Types and calls live here, next to their only users.

import { useState } from "react";
import { Layers, Pencil, Plus, ShieldAlert, Trash } from "lucide-react";
import { type AccountInfo, call } from "../api";
import { Badge, Button, ErrorText, Field, IconButton, Modal, Notice, Switch, useConfirm, useToast } from "../components";
import { useAction } from "../hooks";
import { t } from "../i18n";

export interface SubpoolInfo {
  name: string;
  active: boolean;
  /** `provider/alias`, sorted. */
  accounts: string[];
}

export interface SubpoolState {
  /** null = the main pool. */
  active: string | null;
  remember: boolean;
  subpools: SubpoolInfo[];
}

interface KeysExportResult {
  path: string;
  providers: number;
  accounts: number;
  skipped: string[];
}

interface KeysImportReport {
  providers: number;
  accounts: number;
  keys_updated: number;
  skipped: string[];
  sync_errors: string[];
}

/** What the daemon expects as the second confirmation (`KEYS_EXPORT_CONFIRM`). */
const CONFIRM_WORD = "EXPORT";
const MIN_PASSPHRASE = 8;

export const subpoolApi = {
  state: () => call<SubpoolState>("subpool.list"),
  save: (name: string, accounts: string[], renameFrom: string | null) =>
    call<SubpoolInfo>("subpool.save", { name, accounts, rename_from: renameFrom }),
  remove: (name: string) => call<{ revoked: boolean }>("subpool.remove", { name }),
  activate: (name: string | null) => call<SubpoolState>("subpool.activate", { name }),
  setRemember: (on: boolean) =>
    call("settings.set", { key: "subpool.remember_active", value: on ? "true" : "false" }),
};

const accountName = (a: AccountInfo) => `${a.provider}/${a.alias}`;

/** Switcher shown on top of the accounts page. */
export function SubpoolBar(props: {
  state: SubpoolState | undefined;
  busy: boolean;
  onSwitch: (name: string | null) => void;
  onManage: () => void;
}) {
  const s = props.state;
  const active = s?.subpools.find((p) => p.active);
  return (
    <div className={`subpool-bar ${active ? "on" : ""}`}>
      <Layers size={16} aria-hidden />
      <span className="strong">{t("sp.label")}</span>
      <select
        aria-label={t("sp.label")}
        value={s?.active ?? ""}
        disabled={props.busy || !s}
        onChange={(e) => props.onSwitch(e.target.value || null)}
      >
        <option value="">{t("sp.main")}</option>
        {(s?.subpools ?? []).map((p) => (
          <option key={p.name} value={p.name}>
            {p.name} · {t("sp.count", { n: p.accounts.length })}
          </option>
        ))}
      </select>
      <span className="small muted ellipsis" style={{ flex: 1 }}>
        {active ? t("sp.activeBanner", { name: active.name, n: active.accounts.length }) : t("sp.mainHint")}
      </span>
      {active && (
        <Button disabled={props.busy} onClick={() => props.onSwitch(null)}>
          {t("sp.backToMain")}
        </Button>
      )}
      <Button icon={<Pencil size={16} />} onClick={props.onManage}>
        {t("sp.manage")}
      </Button>
    </div>
  );
}

/** Lists, creates, edits and removes sub-pools. */
export function SubpoolManager(props: {
  state: SubpoolState | undefined;
  accounts: AccountInfo[];
  onClose: () => void;
  onDone: () => void;
}) {
  const action = useAction();
  const confirm = useConfirm();
  const [editing, setEditing] = useState<SubpoolInfo | "new">();
  const s = props.state;

  if (editing) {
    return (
      <SubpoolEditor
        initial={editing === "new" ? undefined : editing}
        accounts={props.accounts}
        onCancel={() => setEditing(undefined)}
        onSaved={() => {
          setEditing(undefined);
          props.onDone();
        }}
      />
    );
  }
  return (
    <Modal
      title={t("sp.manage")}
      onClose={props.onClose}
      wide
      footer={
        <>
          <Button onClick={props.onClose}>{t("common.close")}</Button>
          <Button kind="primary" icon={<Plus size={16} />} onClick={() => setEditing("new")}>
            {t("sp.new")}
          </Button>
        </>
      }
    >
      <Notice>{t("sp.intro")}</Notice>
      <Switch
        checked={s?.remember ?? false}
        disabled={action.busy || !s}
        label={t("sp.remember")}
        onChange={(on) =>
          void action.run(async () => {
            await subpoolApi.setRemember(on);
            props.onDone();
          })
        }
      />
      <p className="small muted">{t("sp.rememberHint")}</p>
      <ErrorText error={action.error} />
      {(s?.subpools ?? []).length === 0 && <p className="muted">{t("sp.empty")}</p>}
      <div className="subpool-list">
        {(s?.subpools ?? []).map((p) => (
          <div key={p.name} className="subpool-row">
            <div style={{ minWidth: 0, flex: 1 }}>
              <div className="row" style={{ gap: 8 }}>
                <span className="strong ellipsis">{p.name}</span>
                {p.active && <Badge tone="ok">{t("sp.current")}</Badge>}
                <span className="small muted">{t("sp.count", { n: p.accounts.length })}</span>
              </div>
              <div className="small muted mono ellipsis" title={p.accounts.join(", ")}>
                {p.accounts.join(", ") || "—"}
              </div>
            </div>
            <IconButton label={t("common.edit")} onClick={() => setEditing(p)}>
              <Pencil size={16} />
            </IconButton>
            <IconButton
              label={t("common.remove")}
              disabled={action.busy}
              onClick={() =>
                void (async () => {
                  if (!(await confirm({ title: t("common.remove"), message: t("sp.deleteConfirm", { name: p.name }), confirmLabel: t("common.remove"), danger: true })))
                    return;
                  await action.run(async () => {
                    await subpoolApi.remove(p.name);
                    props.onDone();
                  });
                })()
              }
            >
              <Trash size={16} />
            </IconButton>
          </div>
        ))}
      </div>
    </Modal>
  );
}

function SubpoolEditor(props: {
  initial?: SubpoolInfo;
  accounts: AccountInfo[];
  onCancel: () => void;
  onSaved: () => void;
}) {
  const action = useAction();
  const [name, setName] = useState(props.initial?.name ?? "");
  const [picked, setPicked] = useState<Set<string>>(new Set(props.initial?.accounts ?? []));
  const toggle = (n: string) => {
    const next = new Set(picked);
    if (next.has(n)) next.delete(n);
    else next.add(n);
    setPicked(next);
  };
  const save = () =>
    void action.run(async () => {
      if (!name.trim()) throw new Error(t("sp.errName"));
      if (picked.size === 0) throw new Error(t("sp.errAccounts"));
      await subpoolApi.save(name.trim(), [...picked], props.initial?.name ?? null);
      props.onSaved();
    });
  const byProvider = new Map<string, AccountInfo[]>();
  for (const a of props.accounts) byProvider.set(a.provider, [...(byProvider.get(a.provider) ?? []), a]);

  return (
    <Modal
      title={props.initial ? t("sp.edit") : t("sp.new")}
      onClose={props.onCancel}
      wide
      footer={
        <>
          <Button onClick={props.onCancel}>{t("common.cancel")}</Button>
          <Button kind="primary" disabled={action.busy} onClick={save}>
            {t("common.save")}
          </Button>
        </>
      }
    >
      <Field label={t("common.name")}>
        <input value={name} maxLength={40} onChange={(e) => setName(e.target.value)} placeholder={t("sp.namePlaceholder")} />
      </Field>
      <div className="row" style={{ justifyContent: "space-between" }}>
        <span className="strong">{t("sp.pickAccounts", { n: picked.size })}</span>
        <div className="row" style={{ gap: 8 }}>
          <Button onClick={() => setPicked(new Set(props.accounts.map(accountName)))}>{t("sp.all")}</Button>
          <Button onClick={() => setPicked(new Set())}>{t("sp.none")}</Button>
        </div>
      </div>
      <div className="subpool-pick">
        {[...byProvider.entries()].map(([provider, list]) => (
          <fieldset key={provider}>
            <legend className="mono small">{provider}</legend>
            {list.map((a) => {
              const n = accountName(a);
              return (
                <label key={n} className="row check-row">
                  <input type="checkbox" checked={picked.has(n)} onChange={() => toggle(n)} />
                  <span>{a.alias}</span>
                  {a.display_name && <span className="small muted">{a.display_name}</span>}
                  {!a.enabled && <Badge>{t("common.disabled")}</Badge>}
                </label>
              );
            })}
          </fieldset>
        ))}
      </div>
      <ErrorText error={action.error} />
    </Modal>
  );
}

/** Two-step export of every API key, encrypted with a passphrase (ExecPlan 020). */
export function KeysExportDialog(props: { accounts: AccountInfo[]; onClose: () => void }) {
  const action = useAction();
  const [step, setStep] = useState<1 | 2>(1);
  const [understood, setUnderstood] = useState(false);
  const [pass, setPass] = useState("");
  const [pass2, setPass2] = useState("");
  const [word, setWord] = useState("");
  const [path, setPath] = useState("");
  const [done, setDone] = useState<KeysExportResult>();
  const n = props.accounts.filter((a) => a.account_type === "api_key" && a.has_key).length;

  const submit = () =>
    void action.run(async () => {
      if ([...pass].length < MIN_PASSPHRASE) throw new Error(t("keys.errShort"));
      if (pass !== pass2) throw new Error(t("keys.errMismatch"));
      if (word.trim() !== CONFIRM_WORD) throw new Error(t("keys.errWord", { word: CONFIRM_WORD }));
      const r = await call<KeysExportResult>("keys.export", {
        path: path.trim() || null,
        passphrase: pass,
        confirm: word.trim(),
      });
      setPass("");
      setPass2("");
      setDone(r);
    });

  return (
    <Modal
      title={t("keys.exportTitle")}
      onClose={props.onClose}
      footer={
        done ? (
          <Button kind="primary" onClick={props.onClose}>
            {t("common.done")}
          </Button>
        ) : step === 1 ? (
          <>
            <Button onClick={props.onClose}>{t("common.cancel")}</Button>
            <Button kind="danger" disabled={!understood || n === 0} onClick={() => setStep(2)}>
              {t("keys.next")}
            </Button>
          </>
        ) : (
          <>
            <Button onClick={() => setStep(1)}>{t("keys.back")}</Button>
            <Button kind="danger" disabled={action.busy} onClick={submit}>
              {action.busy ? t("keys.exporting") : t("keys.doExport")}
            </Button>
          </>
        )
      }
    >
      {done ? (
        <>
          <Notice>{t("keys.exported", { n: done.accounts, path: done.path })}</Notice>
          {done.skipped.length > 0 && (
            <Notice tone="warn">{t("keys.skipped", { list: done.skipped.join(", ") })}</Notice>
          )}
          <p className="small muted">{t("keys.risk3")}</p>
        </>
      ) : step === 1 ? (
        <>
          <div className="strong">
            <ShieldAlert size={16} aria-hidden /> {t("keys.step1")}
          </div>
          <Notice tone="warn">
            <ul className="risk-list">
              <li>{t("keys.risk1", { n })}</li>
              <li>{t("keys.risk2")}</li>
              <li>{t("keys.risk3")}</li>
              <li>{t("keys.risk4")}</li>
              <li>{t("keys.risk5")}</li>
            </ul>
          </Notice>
          {n === 0 && <Notice>{t("keys.none")}</Notice>}
          <label className="row check-row">
            <input type="checkbox" checked={understood} onChange={(e) => setUnderstood(e.target.checked)} />
            <span>{t("keys.understand")}</span>
          </label>
        </>
      ) : (
        <>
          <div className="strong">{t("keys.step2")}</div>
          <Field label={t("keys.passphrase")} hint={t("keys.passphraseHint")}>
            <input type="password" autoComplete="new-password" value={pass} onChange={(e) => setPass(e.target.value)} />
          </Field>
          <Field label={t("keys.passphrase2")}>
            <input type="password" autoComplete="new-password" value={pass2} onChange={(e) => setPass2(e.target.value)} />
          </Field>
          <Field label={t("keys.path")} hint={t("keys.pathHint")}>
            <input value={path} onChange={(e) => setPath(e.target.value)} placeholder="C:\…\loadout-keys.loadout-keys" />
          </Field>
          <Field label={t("keys.confirmWord", { word: CONFIRM_WORD })}>
            <input value={word} onChange={(e) => setWord(e.target.value)} autoComplete="off" />
          </Field>
          <ErrorText error={action.error} />
        </>
      )}
    </Modal>
  );
}

export function KeysImportDialog(props: { onClose: () => void; onDone: () => void }) {
  const action = useAction();
  const toast = useToast();
  const [path, setPath] = useState("");
  const [pass, setPass] = useState("");
  const [overwrite, setOverwrite] = useState(false);
  const [report, setReport] = useState<KeysImportReport>();

  const submit = () =>
    void action.run(async () => {
      if (!path.trim()) throw new Error(t("keys.errFile"));
      const r = await call<KeysImportReport>("keys.import", { path: path.trim(), passphrase: pass, overwrite });
      setPass("");
      setReport(r);
      props.onDone();
      toast(t("keys.imported", { providers: r.providers, accounts: r.accounts, updated: r.keys_updated }), "ok");
    });

  return (
    <Modal
      title={t("keys.importTitle")}
      onClose={props.onClose}
      footer={
        report ? (
          <Button kind="primary" onClick={props.onClose}>
            {t("common.done")}
          </Button>
        ) : (
          <>
            <Button onClick={props.onClose}>{t("common.cancel")}</Button>
            <Button kind="primary" disabled={action.busy} onClick={submit}>
              {action.busy ? t("keys.importing") : t("keys.doImport")}
            </Button>
          </>
        )
      }
    >
      {report ? (
        <>
          <Notice>
            {t("keys.imported", { providers: report.providers, accounts: report.accounts, updated: report.keys_updated })}
          </Notice>
          {report.skipped.length > 0 && <p className="small muted">{t("keys.importSkipped", { list: report.skipped.join("; ") })}</p>}
          {report.sync_errors.length > 0 && (
            <Notice tone="warn">{t("keys.syncErrors", { list: report.sync_errors.join("; ") })}</Notice>
          )}
        </>
      ) : (
        <>
          <Notice>{t("keys.importIntro")}</Notice>
          <Field label={t("keys.file")} hint={t("keys.fileHint")}>
            <input value={path} onChange={(e) => setPath(e.target.value)} placeholder="C:\…\loadout-keys.loadout-keys" />
          </Field>
          <Field label={t("keys.passphrase")}>
            <input type="password" autoComplete="off" value={pass} onChange={(e) => setPass(e.target.value)} />
          </Field>
          <label className="row check-row">
            <input type="checkbox" checked={overwrite} onChange={(e) => setOverwrite(e.target.checked)} />
            <span>{t("keys.overwrite")}</span>
          </label>
          <ErrorText error={action.error} />
        </>
      )}
    </Modal>
  );
}
