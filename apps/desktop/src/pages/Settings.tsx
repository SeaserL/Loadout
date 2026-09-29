// SPDX-License-Identifier: Apache-2.0

import { useState } from "react";
import { api, type DaemonStatus, type Setting } from "../api";
import { Badge, Button, Card, ErrorText, LanguageSelect, Page } from "../components";
import { time } from "../format";
import { useAction, usePoll } from "../hooks";
import { settingDescription, t } from "../i18n";

export function Settings(props: { status: DaemonStatus; onDaemonChange: () => Promise<void> }) {
  const settings = usePoll(api.settings, 0);
  const autostart = usePoll(api.autostart, 0);
  const action = useAction();
  const s = props.status;

  return (
    <Page title={t("settings.title")}>
      <ErrorText error={action.error ?? settings.error?.message} />
      <Card title={t("settings.gateway")}>
        <table>
          <tbody>
            <tr>
              <td className="muted">{t("settings.address")}</td>
              <td className="mono">http://{s.gateway.addr}/v1</td>
            </tr>
            <tr>
              <td className="muted">{t("settings.version")}</td>
              <td>{t("settings.versionValue", { version: s.version, schema: s.schema_version, pid: s.pid })}</td>
            </tr>
            <tr>
              <td className="muted">{t("settings.since")}</td>
              <td>{time(s.started_at_ms)}</td>
            </tr>
            <tr>
              <td className="muted">{t("settings.dataDir")}</td>
              <td className="mono">{s.data_dir}</td>
            </tr>
            <tr>
              <td className="muted">{t("settings.autostart")}</td>
              <td>
                <label className="row">
                  <input
                    type="checkbox"
                    checked={!!autostart.data}
                    disabled={action.busy || autostart.data === undefined}
                    onChange={(e) =>
                      void action.run(async () => {
                        await api.setAutostart(e.target.checked);
                        await autostart.reload();
                      })
                    }
                  />
                  {t("settings.autostartLabel")}
                </label>
              </td>
            </tr>
          </tbody>
        </table>
        <div className="row" style={{ marginTop: 12 }}>
          <div className="spacer" />
          <Button
            kind="danger"
            disabled={action.busy}
            onClick={() => {
              if (confirm(t("settings.confirmStop"))) {
                void action.run(async () => {
                  await api.stopDaemon();
                  await props.onDaemonChange();
                });
              }
            }}
          >
            {t("settings.stop")}
          </Button>
        </div>
      </Card>
      <Card title={t("settings.appearance")}>
        <div className="row">
          <span className="muted">{t("settings.language")}</span>
          <LanguageSelect />
        </div>
      </Card>
      <BackupCard />
      <Card title={t("settings.options")}>
        <table>
          <thead>
            <tr>
              <th>{t("settings.setting")}</th>
              <th>{t("settings.value")}</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {(settings.data ?? []).map((setting) => (
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
      <td style={{ maxWidth: 420 }}>
        <div className="mono">{s.key}</div>
        <div className="small muted">{settingDescription(s.key, s.description)}</div>
        {s.restart_required && <Badge tone="warn">{t("settings.restart")}</Badge>}
      </td>
      <td>
        {boolean ? (
          <select
            value={value}
            onChange={(e) => {
              setValue(e.target.value);
              props.onSave(e.target.value);
            }}
            disabled={props.busy}
          >
            <option value="true">{t("settings.on")}</option>
            <option value="false">{t("settings.off")}</option>
          </select>
        ) : (
          <input value={value} onChange={(e) => setValue(e.target.value)} style={{ width: 220 }} />
        )}
      </td>
      <td className="num">
        <div className="row" style={{ justifyContent: "flex-end" }}>
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

function BackupCard() {
  const action = useAction();
  const [path, setPath] = useState("");
  const [message, setMessage] = useState<string>();
  return (
    <Card title={t("settings.backup")}>
      <p className="small muted">{t("settings.backupIntro")}</p>
      <div className="row wrap">
        <input
          style={{ flex: 1, minWidth: 320 }}
          value={path}
          onChange={(e) => setPath(e.target.value)}
          placeholder={t("settings.pathPlaceholder")}
        />
        <Button
          disabled={action.busy}
          onClick={() =>
            void action.run(async () => {
              const r = await api.exportConfig(path.trim() || null);
              setMessage(
                t("settings.exported", {
                  providers: r.providers,
                  accounts: r.accounts,
                  models: r.models,
                  pools: r.pools,
                  clients: r.clients,
                  path: r.path,
                }),
              );
            })
          }
        >
          {t("settings.export")}
        </Button>
        <Button
          disabled={action.busy || !path.trim()}
          onClick={() =>
            void action.run(async () => {
              const r = await api.importConfig(path.trim());
              const keys = r.accounts_needing_keys.length
                ? t("settings.needKeys", { accounts: r.accounts_needing_keys.join(", ") })
                : "";
              const skipped = r.skipped.length ? t("settings.skipped", { items: r.skipped.join(", ") }) : "";
              setMessage(
                t("settings.imported", {
                  providers: r.providers,
                  accounts: r.accounts,
                  models: r.models,
                  pools: r.pools,
                  clients: r.clients,
                  settings: r.settings,
                }) +
                  keys +
                  skipped,
              );
            })
          }
        >
          {t("settings.import")}
        </Button>
        <Button
          disabled={action.busy}
          onClick={() =>
            void action.run(async () => {
              const r = await api.backupDatabase(path.trim() || null);
              setMessage(t("settings.dbCopied", { path: r.path }));
            })
          }
        >
          {t("settings.copyDb")}
        </Button>
      </div>
      <ErrorText error={action.error} />
      {message && (
        <div className="notice small" style={{ marginTop: 10 }}>
          {message}
        </div>
      )}
    </Card>
  );
}
