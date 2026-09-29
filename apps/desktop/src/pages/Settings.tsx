// SPDX-License-Identifier: Apache-2.0

import { useState } from "react";
import { api, type DaemonStatus, type Setting } from "../api";
import { Badge, Button, Card, ErrorText, Page } from "../components";
import { time } from "../format";
import { useAction, usePoll } from "../hooks";

export function Settings(props: { status: DaemonStatus; onDaemonChange: () => Promise<void> }) {
  const settings = usePoll(api.settings, 0);
  const autostart = usePoll(api.autostart, 0);
  const action = useAction();
  const s = props.status;

  return (
    <Page title="Settings">
      <ErrorText error={action.error ?? settings.error?.message} />
      <Card title="Gateway">
        <table>
          <tbody>
            <tr>
              <td className="muted">Address</td>
              <td className="mono">http://{s.gateway.addr}/v1</td>
            </tr>
            <tr>
              <td className="muted">Version</td>
              <td>
                {s.version} (schema {s.schema_version}, pid {s.pid})
              </td>
            </tr>
            <tr>
              <td className="muted">Running since</td>
              <td>{time(s.started_at_ms)}</td>
            </tr>
            <tr>
              <td className="muted">Data folder</td>
              <td className="mono">{s.data_dir}</td>
            </tr>
            <tr>
              <td className="muted">Start at sign-in</td>
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
                  Start the gateway when I sign in to Windows
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
              if (confirm("Stop the gateway? Apps using Loadout lose access until it is started again.")) {
                void action.run(async () => {
                  await api.stopDaemon();
                  await props.onDaemonChange();
                });
              }
            }}
          >
            Stop gateway
          </Button>
        </div>
      </Card>
      <Card title="Options">
        <table>
          <thead>
            <tr>
              <th>Setting</th>
              <th>Value</th>
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
        <div className="small muted">{s.description}</div>
        {s.restart_required && <Badge tone="warn">applies after restarting the gateway</Badge>}
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
            <option value="true">on</option>
            <option value="false">off</option>
          </select>
        ) : (
          <input value={value} onChange={(e) => setValue(e.target.value)} style={{ width: 220 }} />
        )}
      </td>
      <td className="num">
        <div className="row" style={{ justifyContent: "flex-end" }}>
          {!boolean && (
            <Button kind="primary" disabled={!changed || props.busy} onClick={() => props.onSave(value)}>
              Save
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
              Default
            </Button>
          )}
        </div>
      </td>
    </tr>
  );
}
