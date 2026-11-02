// SPDX-License-Identifier: Apache-2.0
//
// What OmniRoute knows beyond your own accounts: which models are free, what models cost, and how
// the gateway waits and retries. Each lives next to what it serves (Models, Routes).

import { useMemo, useState } from "react";
import { Plus, RefreshCw, Search } from "lucide-react";
import { api, native } from "../api";
import { Button, Empty, ErrorText, Switch, useToast } from "../components";
import { ago, tokens } from "../format";
import { useAction, usePoll } from "../hooks";
import { t } from "../i18n";

type RadarEntry = {
  modelId: string;
  displayName?: string;
  provider: string;
  monthlyTokens?: number;
  creditTokens?: number;
  freeType?: string;
  tos?: string;
  trainsOnPrompts?: boolean;
};
type RadarStatus = {
  optIn?: boolean;
  feeds?: { catalog?: { fetchedAt?: string | null; generatedAt?: string | null; available?: boolean } };
};

const round = (n: number | undefined) => tokens(n).replace(/\.0+([kMB])$/, "$1");

function allowance(e: RadarEntry): string {
  if ((e.monthlyTokens ?? 0) > 0) return t("res.perMonth", { n: round(e.monthlyTokens) });
  if ((e.creditTokens ?? 0) > 0) return t("res.oneTime", { n: round(e.creditTokens) });
  return t("res.uncapped");
}

export function FreeCatalog() {
  const catalog = usePoll(() => native<{ entries: RadarEntry[] }>("/api/radar/catalog"), 0);
  const status = usePoll(() => native<RadarStatus>("/api/radar/status"), 0);
  const settings = usePoll(() => native<{ optIn?: boolean }>("/api/radar/settings"), 0);
  const providers = usePoll(api.providers, 0);
  const action = useAction();
  const toast = useToast();
  const [query, setQuery] = useState("");
  const fetched = status.data?.feeds?.catalog?.fetchedAt;
  const q = query.trim().toLowerCase();
  const connected = useMemo(() => {
    const s = new Set<string>();
    for (const p of providers.data ?? []) {
      s.add(p.slug);
      s.add(p.kind);
    }
    return s;
  }, [providers.data]);
  const groups = useMemo(() => {
    const out = new Map<string, RadarEntry[]>();
    for (const e of catalog.data?.entries ?? []) {
      if (q && !`${e.provider} ${e.modelId} ${e.displayName ?? ""}`.toLowerCase().includes(q)) continue;
      out.set(e.provider, [...(out.get(e.provider) ?? []), e]);
    }
    // What you can already use first, then the most generous.
    return [...out.entries()].sort(
      (a, b) => Number(connected.has(b[0])) - Number(connected.has(a[0])) || b[1].length - a[1].length,
    );
  }, [catalog.data, q, connected]);
  return (
    <>
      <div className="toolbar">
        <label className="search grow">
          <Search size={16} aria-hidden />
          <input placeholder={t("res.freeSearch")} aria-label={t("res.freeSearch")} value={query} onChange={(e) => setQuery(e.target.value)} />
        </label>
        <span className="check-row small">
          <Switch
            checked={!!settings.data?.optIn}
            disabled={action.busy || !settings.data}
            label={t("res.radarAuto")}
            onChange={(on) =>
              void action.run(async () => {
                await native("/api/radar/settings", "POST", { optIn: on });
                await settings.reload();
              })
            }
          />
          {t("res.radarAuto")}
        </span>
        <span className="small muted">{fetched ? ago(Date.parse(fetched)) : t("res.neverFetched")}</span>
        <Button
          icon={<RefreshCw size={15} />}
          disabled={action.busy}
          onClick={() =>
            void action.run(async () => {
              await native("/api/radar/sync", "POST", {});
              await Promise.all([catalog.reload(), status.reload()]);
              toast(t("res.synced"), "ok");
            })
          }
        >
          {t("res.syncNow")}
        </Button>
      </div>
      <ErrorText error={action.error ?? catalog.error?.message} />
      {groups.length === 0 && !catalog.loading && <Empty>{t("res.freeEmpty")}</Empty>}
      <div className="catalog-grid">
        {groups.slice(0, 80).map(([provider, list]) => (
          <section className={`catalog-card ${connected.has(provider) ? "have" : ""}`} key={provider}>
            <header>
              <span className="strong ellipsis">{provider}</span>
              {connected.has(provider) ? (
                <span className="small catalog-have">{t("res.connected")}</span>
              ) : (
                <a className="catalog-add small" href="#/accounts">
                  <Plus size={13} aria-hidden /> {t("res.connect")}
                </a>
              )}
            </header>
            <ul>
              {list.slice(0, 6).map((m) => (
                <li key={m.modelId} title={m.modelId}>
                  <span className="ellipsis">{m.displayName ?? m.modelId}</span>
                  <span className={`small num nowrap ${m.trainsOnPrompts || m.tos === "avoid" ? "warn-text" : "muted"}`} title={m.trainsOnPrompts ? t("res.trains") : m.tos === "avoid" ? t("res.tosAvoid") : undefined}>
                    {allowance(m)}
                  </span>
                </li>
              ))}
              {list.length > 6 && <li className="small muted">{t("res.more", { n: list.length - 6 })}</li>}
            </ul>
          </section>
        ))}
      </div>
    </>
  );
}

type Pricing = {
  pricing: Record<string, Record<string, { input?: number; output?: number; cached?: number }>>;
  sourceMap?: Record<string, Record<string, string>>;
};

export function PriceTable() {
  const table = usePoll(() => native<Pricing>("/api/pricing?includeSources=1"), 0);
  const sync = usePoll(() => native<{ lastSync?: string | null; enabled?: boolean }>("/api/pricing/sync"), 0);
  const action = useAction();
  const toast = useToast();
  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();
  const rows = useMemo(() => {
    const out: { provider: string; model: string; input?: number; output?: number; cached?: number; source: string }[] = [];
    for (const [provider, models] of Object.entries(table.data?.pricing ?? {}))
      for (const [model, p] of Object.entries(models))
        if (!q || `${provider} ${model}`.toLowerCase().includes(q))
          out.push({ provider, model, ...p, source: table.data?.sourceMap?.[provider]?.[model] ?? "" });
    return out;
  }, [table.data, q]);
  return (
    <>
      <div className="toolbar">
        <label className="search grow">
          <Search size={16} aria-hidden />
          <input placeholder={t("res.pricingSearch")} aria-label={t("res.pricingSearch")} value={query} onChange={(e) => setQuery(e.target.value)} />
        </label>
        <span className="small muted">{sync.data?.lastSync ? ago(Date.parse(sync.data.lastSync)) : t("res.neverFetched")}</span>
        <Button
          icon={<RefreshCw size={15} />}
          disabled={action.busy}
          onClick={() =>
            void action.run(async () => {
              await native("/api/pricing/sync", "POST", { sources: ["litellm"] });
              await Promise.all([table.reload(), sync.reload()]);
              toast(t("res.synced"), "ok");
            })
          }
        >
          {t("res.syncNow")}
        </Button>
      </div>
      <ErrorText error={action.error ?? table.error?.message} />
      <div className="table-wrap">
        <table>
          <thead>
            <tr>
              <th>{t("models.name")}</th>
              <th>{t("res.provider")}</th>
              <th className="num">{t("models.input")}</th>
              <th className="num">{t("models.output")}</th>
              <th className="num">{t("res.cached")}</th>
              <th>{t("res.source")}</th>
            </tr>
          </thead>
          <tbody>
            {rows.slice(0, 200).map((r) => (
              <tr key={`${r.provider}/${r.model}`}>
                <td>{r.model}</td>
                <td className="muted">{r.provider}</td>
                <td className="num">{r.input ?? "?"}</td>
                <td className="num">{r.output ?? "?"}</td>
                <td className="num">{r.cached ?? "-"}</td>
                <td className="small muted">{r.source}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {rows.length > 200 && <p className="small muted">{t("res.showing", { n: 200, all: rows.length })}</p>}
    </>
  );
}

type ResilienceConfig = {
  connectionCooldown?: Record<string, { baseCooldownMs?: number; useUpstreamRetryHints?: boolean; maxBackoffSteps?: number }>;
  providerBreaker?: Record<string, { failureThreshold?: number; degradationThreshold?: number; resetTimeoutMs?: number }>;
  comboCooldownWait?: { enabled?: boolean; maxWaitMs?: number; maxAttempts?: number; budgetMs?: number };
};

const seconds = (ms: number | undefined) => (ms == null ? "-" : t("res.seconds", { n: Math.round(ms / 1000) }));

export function Resilience() {
  const config = usePoll(() => native<ResilienceConfig>("/api/resilience"), 0);
  const c = config.data;
  // Direct requests never wait in this build (fusionSafety pins it); only groups can.
  const wait = c?.comboCooldownWait;
  const action = useAction();
  const toast = useToast();
  const rows: { label: string; api: string; sub: string }[] = [
    {
      label: t("res.rowRest"),
      api: seconds(c?.connectionCooldown?.apikey?.baseCooldownMs),
      sub: seconds(c?.connectionCooldown?.oauth?.baseCooldownMs),
    },
    {
      label: t("res.rowDouble"),
      api: String(c?.connectionCooldown?.apikey?.maxBackoffSteps ?? "-"),
      sub: String(c?.connectionCooldown?.oauth?.maxBackoffSteps ?? "-"),
    },
    {
      label: t("res.rowHints"),
      api: c?.connectionCooldown?.apikey?.useUpstreamRetryHints ? t("settings.on") : t("settings.off"),
      sub: c?.connectionCooldown?.oauth?.useUpstreamRetryHints ? t("settings.on") : t("settings.off"),
    },
    {
      label: t("res.rowDegrade"),
      api: String(c?.providerBreaker?.apikey?.degradationThreshold ?? "-"),
      sub: String(c?.providerBreaker?.oauth?.degradationThreshold ?? "-"),
    },
    {
      label: t("res.rowBreak"),
      api: String(c?.providerBreaker?.apikey?.failureThreshold ?? "-"),
      sub: String(c?.providerBreaker?.oauth?.failureThreshold ?? "-"),
    },
    {
      label: t("res.rowReset"),
      api: seconds(c?.providerBreaker?.apikey?.resetTimeoutMs),
      sub: seconds(c?.providerBreaker?.oauth?.resetTimeoutMs),
    },
  ];
  return (
    <>
      <ErrorText error={action.error ?? config.error?.message} />
      {wait && (
        <div className="toolbar">
          <span className="check-row">
            <Switch
              checked={!!wait.enabled}
              disabled={action.busy}
              label={t("res.waitEnable")}
              onChange={(on) =>
                void action.run(async () => {
                  // An instance that never waited has a budget of nothing: turning it on needs one.
                  const budget = on && !wait.maxAttempts ? { maxAttempts: 2, maxWaitMs: 30000, budgetMs: 30000 } : {};
                  await native("/api/resilience", "PATCH", { comboCooldownWait: { ...budget, enabled: on } });
                  await config.reload();
                  toast(t("res.saved"), "ok");
                })
              }
            />
            {t("res.waitEnable")}
          </span>
          {wait.enabled && <span className="small muted">{t("res.waitBudget", { n: Math.round((wait.maxWaitMs ?? 0) / 1000), retries: wait.maxAttempts ?? 0 })}</span>}
        </div>
      )}
      <div className="table-wrap resilience-table">
        <table>
          <thead>
            <tr>
              <th />
              <th className="num">{t("res.colApi")}</th>
              <th className="num">{t("res.colSub")}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr key={r.label}>
                <td>{r.label}</td>
                <td className="num">{r.api}</td>
                <td className="num">{r.sub}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </>
  );
}
