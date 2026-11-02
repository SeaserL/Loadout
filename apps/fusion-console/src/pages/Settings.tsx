// SPDX-License-Identifier: Apache-2.0

import { useState } from "react";
import { CircleStop, Database, Download, RefreshCw, RotateCw } from "lucide-react";
import { api, type DaemonStatus, type Setting, type UpdateStatus } from "../api";
import {
  Badge,
  Button,
  Card,
  ErrorText,
  LanguageSelect,
  MotionRate,
  Notice,
  Page,
  PaletteGrid,
  Switch,
  ThemeSwitch,
  useConfirm,
} from "../components";
import { time } from "../format";
import { useAction, usePoll } from "../hooks";
import { settingDescription, t } from "../i18n";

export function Settings(props: { status: DaemonStatus; onDaemonChange: () => Promise<void> }) {
  const settings = usePoll(api.settings, 0);
  const action = useAction();
  const confirm = useConfirm();
  const s = props.status;

  return (
    <Page title={t("settings.title")} narrow help="settings">
      <ErrorText error={action.error ?? settings.error?.message} />
      <div className="grid cols-2 collapse" style={{ marginBottom: 16 }}>
        <Card title={t("settings.gateway")}>
          <dl className="kv">
            <dt>{t("settings.address")}</dt>
            <dd className="mono">http://{s.gateway.addr}/v1</dd>
            <dt>{t("settings.version")}</dt>
            <dd>{t("settings.versionValue", { version: s.version, schema: s.schema_version, pid: s.pid })}</dd>
            <dt>{t("settings.since")}</dt>
            <dd>{time(s.started_at_ms)}</dd>
            <dt>{t("settings.dataDir")}</dt>
            <dd className="mono small">{s.data_dir}</dd>
            {s.log_retention_days != null && (
              <>
                <dt>{t("settings.retention")}</dt>
                <dd>{t("settings.retentionDays", { n: s.log_retention_days })}</dd>
              </>
            )}
            <dt>{t("settings.network")}</dt>
            <dd>{s.network === "direct" ? t("settings.networkDirect") : t("settings.networkLocal")}</dd>
          </dl>
          <div className="card-foot">
            <div className="spacer" />
            <Button
              icon={<RotateCw size={16} />}
              disabled={action.busy}
              onClick={() =>
                void confirm({
                  title: t("dash.confirmRestartTitle"),
                  message: t("settings.confirmRestart"),
                  confirmLabel: t("settings.restart2"),
                }).then(
                  (ok) =>
                    ok &&
                    void action.run(async () => {
                      await api.restartDaemon();
                    }),
                )
              }
            >
              {t("settings.restart2")}
            </Button>
            <Button
              kind="danger"
              icon={<CircleStop size={16} />}
              disabled={action.busy}
              onClick={() =>
                void confirm({
                  title: t("settings.confirmStopTitle"),
                  message: t("settings.confirmStop"),
                  confirmLabel: t("settings.stop"),
                  danger: true,
                }).then(
                  (ok) =>
                    ok &&
                    void action.run(async () => {
                      await api.stopDaemon();
                      await props.onDaemonChange();
                    }),
                )
              }
            >
              {t("settings.stop")}
            </Button>
          </div>
        </Card>
        <Card title={t("settings.appearance")}>
          <dl className="kv">
            <dt>{t("theme.label")}</dt>
            <dd>
              <div style={{ display: "inline-flex" }}>
                <ThemeSwitch />
              </div>
            </dd>
            <dt>{t("settings.language")}</dt>
            <dd>
              <LanguageSelect />
            </dd>
            <dt>{t("settings.motion")}</dt>
            <dd>
              <MotionRate />
            </dd>
          </dl>
          <div className="settings-palette">
            <PaletteGrid />
          </div>
        </Card>
      </div>
      <UpdateCard />
      <BackupCard />
      <Card title={t("settings.options")}>
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>{t("settings.setting")}</th>
                <th>{t("settings.value")}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {(settings.data ?? []).filter(s => !s.key.startsWith("updates.")).map((setting) => (
                <SettingRow
                  key={setting.key}
                  setting={setting}
                  busy={action.busy}
                  onSave={(value) =>
                    void action.run(async () => {
                      await api.setSetting(setting.key, value);
                      await settings.reload();
                    })
                  }
                  onReset={() =>
                    void action.run(async () => {
                      await api.resetSetting(setting.key);
                      await settings.reload();
                    })
                  }
                />
              ))}
            </tbody>
          </table>
        </div>
      </Card>
    </Page>
  );
}

function SettingRow(props: {
  setting: Setting;
  busy: boolean;
  onSave: (value: string) => void;
  onReset: () => void;
}) {
  const s = props.setting;
  const [value, setValue] = useState(s.value);
  const boolean = s.default === "true" || s.default === "false";
  const changed = value !== s.value;
  return (
    <tr>
      <td style={{ maxWidth: 460 }}>
        <div className="row wrap">
          <span className="strong" title={s.key}>{s.name ?? s.key}</span>
          {s.restart_required && <Badge tone="warn">{t("settings.restart")}</Badge>}
        </div>
        <div className="small muted">{settingDescription(s.key, s.description)}</div>
      </td>
      <td>
        {boolean ? (
          <Switch
            checked={value === "true"}
            disabled={props.busy}
            label={value === "true" ? t("settings.on") : t("settings.off")}
            onChange={(on) =>
              void (async () => {
                // Browser sessions carry account risks: confirm before turning them on
                // (ADR-014).
                const v = on ? "true" : "false";
                setValue(v);
                props.onSave(v);
              })()
            }
          />
        ) : (
          <input
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && changed && !props.busy) props.onSave(value);
            }}
            style={{ width: 200 }}
            aria-label={s.key}
          />
        )}
      </td>
      <td className="actions-cell">
        <div className="row">
          {!boolean && (
            <Button kind="primary" disabled={!changed || props.busy} onClick={() => props.onSave(value)}>
              {t("common.save")}
            </Button>
          )}
          {!s.is_default && (
            <Button
              disabled={props.busy}
              onClick={() => {
                setValue(s.default);
                props.onReset();
              }}
            >
              {t("settings.default")}
            </Button>
          )}
        </div>
      </td>
    </tr>
  );
}

const BUSY = new Set(["checking", "downloading", "extracting", "verifying", "installing"]);
const megabytes = (bytes: number) => `${Math.round(bytes / 1048576)} MB`;

function UpdateCard() {
  const status = usePoll(api.updateStatus, 2000);
  const [fresh, setFresh] = useState<UpdateStatus>();
  const action = useAction();
  const confirm = useConfirm();
  const u = fresh ?? status.data;
  if (!u) return null;
  const start = (call: () => Promise<UpdateStatus>) =>
    void action.run(async () => {
      setFresh(await call());
      window.setTimeout(() => setFresh(undefined), 1500);
    });
  const busy = BUSY.has(u.phase) || action.busy;
  const latest = u.latest_version ?? "";
  const line =
    u.phase === "checking"
      ? t("update.checking")
      : u.phase === "current"
        ? t("update.upToDate")
        : u.phase === "available"
          ? t("update.available", { version: latest })
          : u.phase === "downloading"
            ? t("update.downloading")
            : u.phase === "extracting"
              ? t("update.extracting")
              : u.phase === "verifying"
                ? t("update.verifying")
                : u.phase === "ready"
                  ? t("update.ready", { version: latest })
                  : u.phase === "installing"
                    ? t("update.installing")
                    : u.phase === "disabled"
                      ? t("update.disabled")
                      : null;
  const share = u.progress && u.progress.total > 0 ? Math.min(100, (u.progress.done / u.progress.total) * 100) : null;
  return (
    <Card title={t("update.title")}>
      <dl className="kv">
        <dt>{t("update.current")}</dt>
        <dd>{u.current_version}</dd>
        {line && (
          <>
            <dt>{t("update.state")}</dt>
            <dd>
              {line}
              {u.release_url && (u.phase === "available" || u.phase === "ready") && (
                <>
                  {" "}
                  <a href={u.release_url} onClick={(e) => { e.preventDefault(); void api.updateOpenNotes().catch(() => undefined); }}>
                    {t("update.notes")}
                  </a>
                </>
              )}
            </dd>
          </>
        )}
        {u.checked_at_ms != null && (
          <>
            <dt>{t("update.checkedAt")}</dt>
            <dd>{time(u.checked_at_ms)}</dd>
          </>
        )}
        {u.phase !== "disabled" && (
          <>
            <dt>{t("update.auto")}</dt>
            <dd>
              <Switch checked={u.auto_check} label={t("update.auto")} onChange={(on) => start(() => api.updateSetAutoCheck(on))} />
            </dd>
          </>
        )}
      </dl>
      {share != null && (
        <div className="update-bar" role="progressbar" aria-valuenow={Math.round(share)} aria-valuemin={0} aria-valuemax={100}>
          <i style={{ width: `${share}%` }} />
        </div>
      )}
      {u.last_result?.status === "installed" && <Notice>{t("update.installed", { from: u.last_result.from, to: u.last_result.to })}</Notice>}
      {u.last_result?.status === "failed" && (
        <Notice tone="warn">{t("update.failed", { from: u.last_result.from, to: u.last_result.to })}</Notice>
      )}
      <ErrorText error={action.error ?? u.error} />
      {u.phase !== "disabled" && (
        <div className="card-foot">
          <div className="spacer" />
          <Button icon={<RefreshCw size={16} />} disabled={busy} onClick={() => start(api.updateCheck)}>
            {t("update.check")}
          </Button>
          {u.phase === "available" && (
            <Button kind="primary" icon={<Download size={16} />} disabled={busy} onClick={() => start(api.updateDownload)}>
              {t("update.download", { version: latest, size: u.size ? megabytes(u.size) : "" })}
            </Button>
          )}
          {u.phase === "ready" && (
            <Button
              kind="primary"
              icon={<RotateCw size={16} />}
              disabled={busy}
              onClick={() =>
                void confirm({
                  title: t("update.confirmTitle", { version: latest }),
                  message: t("update.confirmBody"),
                  confirmLabel: t("update.install"),
                }).then((ok) => ok && start(api.updateInstall))
              }
            >
              {t("update.install")}
            </Button>
          )}
        </div>
      )}
    </Card>
  );
}

function BackupCard() {
  const action = useAction();
  const [message, setMessage] = useState<string>();
  return (
    <Card title={t("settings.backup")}>
      <div className="row wrap" style={{ gap: 10 }}>
        <Button
          icon={<Database size={16} />}
          disabled={action.busy}
          onClick={() =>
            void action.run(async () => {
              const r = await api.backupDatabase(null);
              setMessage(t("settings.dbCopied", { path: r.path }));
            })
          }
        >
          {t("settings.copyDb")}
        </Button>
        <a className="btn" href="#/accounts">
          {t("settings.exportKeysLink")}
        </a>
      </div>
      {message && <Notice>{message}</Notice>}
      <ErrorText error={action.error} />
    </Card>
  );
}
