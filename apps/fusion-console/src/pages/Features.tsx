// SPDX-License-Identifier: Apache-2.0
//
// The 1.1 feature switches, grouped by what they are for. Each control sits next to what it
// changes; what a feature does is explained in the guide, not here.

import { api, type Feature, type FeatureGroup, type FeatureList } from "../api";
import { Download } from "lucide-react";
import { Badge, Button, Card, ErrorText, Page, Segmented, ShareBar, Switch } from "../components";
import { useAction, usePoll } from "../hooks";
import { t, tk } from "../i18n";

const GROUPS: FeatureGroup[] = ["save", "safety", "extend"];

export function Features() {
  const list = usePoll(api.features, 0);
  const busyPack = list.data?.ai_pack.phase === "downloading" || list.data?.ai_pack.phase === "installing";
  const pack = usePoll(api.features, busyPack ? 1500 : 0, [busyPack]);
  const shown = (busyPack ? pack.data : undefined) ?? list.data;
  const action = useAction();
  const run = (work: () => Promise<unknown>) =>
    void action.run(async () => {
      await work();
      await list.reload();
    });
  const data = shown;
  return (
    <Page title={t("features.title")} narrow help="features">
      <ErrorText error={action.error ?? list.error?.message} />
      {data &&
        GROUPS.map((group) => (
          <Card key={group} title={tk(`features.group.${group}`, group)}>
            <div className="feature-list">
              {data.features
                .filter((f) => f.group === group)
                .map((f) => (
                  <FeatureRow
                    key={f.key}
                    feature={f}
                    busy={action.busy}
                    onToggle={(on) => run(() => api.setFeature(f.key, on))}
                    onOption={(option, value) => run(() => api.setFeatureOption(f.key, option, value))}
                  />
                ))}
            </div>
          </Card>
        ))}
      {data && (
        <PackCard
          pack={data.ai_pack}
          busy={action.busy}
          onInstall={() => run(() => api.installAiPack())}
        />
      )}
    </Page>
  );
}

const mb = (n: number) => `${Math.round(n / 1048576)} MB`;

function PackCard(props: { pack: FeatureList["ai_pack"]; busy: boolean; onInstall: () => void }) {
  const p = props.pack;
  const working = p.phase === "downloading" || p.phase === "installing";
  return (
    <Card
      title={t("features.pack")}
      actions={
        !p.installed &&
        !working && (
          <Button icon={<Download size={16} />} disabled={props.busy} onClick={props.onInstall}>
            {t("features.packInstall")}
          </Button>
        )
      }
    >
      {p.installed ? (
        <Badge tone="ok" dot>
          {t("features.packInstalled", { version: p.version ?? "" })}
        </Badge>
      ) : working ? (
        <div className="feature-option">
          <span className="muted">{t(p.phase === "installing" ? "features.packInstalling" : "features.packDownloading")}</span>
          {p.progress && (
            <>
              <ShareBar value={p.progress.done} max={p.progress.total} />
              <span className="small muted">
                {mb(p.progress.done)} / {mb(p.progress.total)}
              </span>
            </>
          )}
        </div>
      ) : (
        <Badge tone="muted">{t("features.packAbsent")}</Badge>
      )}
      <ErrorText error={p.error} />
    </Card>
  );
}

function FeatureRow(props: {
  feature: Feature;
  busy: boolean;
  onToggle: (on: boolean) => void;
  onOption: (option: string, value: string) => void;
}) {
  const f = props.feature;
  const name = tk(`features.${f.key}`, f.key);
  return (
    <div className={`feature-row ${f.enabled ? "on" : ""}`}>
      <div className="feature-head">
        <span className="strong">{name}</span>
        {!f.available && <Badge tone="warn">{t("features.needsPack")}</Badge>}
        {f.stores_content && f.enabled && <Badge tone="info">{t("features.storesContent")}</Badge>}
        <div className="spacer" />
        <Switch
          checked={f.enabled}
          disabled={props.busy || !f.available}
          label={name}
          onChange={props.onToggle}
        />
      </div>
      {f.enabled &&
        f.options.map((o) => (
          <div className="feature-option" key={o.key}>
            <span className="muted">{tk(`features.${f.key}.${o.key}`, o.key)}</span>
            {o.kind === "model" ? (
              <select
                value={o.value}
                aria-label={tk(`features.${f.key}.${o.key}`, o.key)}
                disabled={props.busy}
                onChange={(e) => props.onOption(o.key, e.target.value)}
              >
                {!o.choices.some((c) => c.value === o.value) && <option value={o.value}>{o.value || t("features.pickModel")}</option>}
                {o.choices.map((c) => (
                  <option key={c.value} value={c.value}>
                    {c.value}
                  </option>
                ))}
              </select>
            ) : (
            <Segmented
              label={tk(`features.${f.key}.${o.key}`, o.key)}
              value={o.value}
              options={o.choices.map((c) => ({
                value: c.value,
                label: tk(`features.${f.key}.${o.key}.${c.value}`, c.value),
                ...(c.available === false ? { title: t("features.needsPack") } : {}),
              }))}
              onChange={(value) => {
                if (o.choices.find((c) => c.value === value)?.available !== false) props.onOption(o.key, value);
              }}
            />
            )}
          </div>
        ))}
    </div>
  );
}
