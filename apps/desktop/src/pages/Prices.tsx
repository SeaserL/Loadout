// SPDX-License-Identifier: Apache-2.0

import { useState } from "react";
import { RefreshCw, Search } from "lucide-react";
import { call } from "../api";
import { Badge, Card, Empty, ErrorText, IconButton, Page, Switch } from "../components";
import { usePoll } from "../hooks";
import { t } from "../i18n";

/** Mirrors `loadout_ipc::api::PriceTokenMix`. */
interface PriceTokenMix {
  source: "model" | "all" | "default" | string;
  requests: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
}

/** Mirrors `loadout_ipc::api::PriceOffer`; prices are USD per 1M tokens, null = unknown. */
interface PriceOffer {
  name: string;
  provider: string;
  account: string;
  upstream_model: string;
  enabled: boolean;
  input_price: number | null;
  output_price: number | null;
  cache_read_price: number | null;
  cache_write_price: number | null;
  price_source: string | null;
  price_updated_at_ms: number | null;
  blended_per_million: number | null;
}

interface PriceGroup {
  key: string;
  mix: PriceTokenMix;
  offers: PriceOffer[];
}

const comparePrices = (model: string | null) => call<PriceGroup[]>("price.compare", { model });

function price(v: number | null): string {
  if (v == null) return "?";
  return `$${Number(v.toFixed(4))}`;
}

function describeMix(mix: PriceTokenMix): string {
  const total = Math.max(1, mix.input_tokens + mix.output_tokens);
  const input = Math.round((mix.input_tokens / total) * 100);
  const cached =
    mix.input_tokens > 0
      ? Math.round(((mix.cache_read_tokens + mix.cache_write_tokens) / mix.input_tokens) * 100)
      : 0;
  const shares = t("prices.mix", { input, output: 100 - input, cached });
  const source =
    mix.source === "model"
      ? t("prices.mix.model", { n: mix.requests })
      : mix.source === "all"
        ? t("prices.mix.all", { n: mix.requests })
        : t("prices.mix.default");
  return `${shares} · ${source}`;
}

function SourceBadge(props: { source: string | null }) {
  if (props.source === "user") return <Badge tone="accent">{t("prices.source.user")}</Badge>;
  if (props.source === "catalog") return <Badge tone="info">{t("prices.source.catalog")}</Badge>;
  return <Badge tone="muted">{t("prices.source.unknown")}</Badge>;
}

export function Prices() {
  const [search, setSearch] = useState("");
  const [showAll, setShowAll] = useState(false);
  const query = search.trim();
  const groups = usePoll(() => comparePrices(query === "" ? null : query), 30000, [query]);

  const visible = (groups.data ?? [])
    .map((g) => ({ ...g, offers: showAll ? g.offers : g.offers.filter((o) => o.enabled) }))
    .filter((g) => g.offers.length > 0 && (showAll || query !== "" || g.offers.length > 1));

  return (
    <Page
      title={t("prices.title")}
      actions={
        <IconButton label={t("common.refresh")} onClick={() => void groups.reload()}>
          <RefreshCw size={16} />
        </IconButton>
      }
    >
      <div className="row wrap" style={{ marginBottom: 16, gap: 14 }}>
        <label className="search" style={{ flex: 1, maxWidth: 420 }}>
          <Search size={16} aria-hidden />
          <input
            placeholder={t("prices.searchPlaceholder")}
            aria-label={t("prices.search")}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </label>
        <span className="check-row">
          <Switch checked={showAll} onChange={setShowAll} label={t("prices.showAll")} />
          {t("prices.showAll")}
        </span>
      </div>

      <ErrorText error={groups.error?.message} />
      {visible.length === 0 && !groups.loading && !groups.error && <Empty>{t("prices.empty")}</Empty>}

      {visible.map((g) => {
        const cheapest = g.offers[0]?.blended_per_million;
        return (
          <Card key={g.key} title={<span className="mono">{g.key}</span>} subtitle={describeMix(g.mix)}>
            <div className="table-wrap">
              <table>
                <thead>
                  <tr>
                    <th>{t("prices.route")}</th>
                    <th className="num">{t("prices.input")}</th>
                    <th className="num">{t("prices.output")}</th>
                    <th className="num">{t("prices.cacheRead")}</th>
                    <th className="num">{t("prices.cacheWrite")}</th>
                    <th className="num">{t("prices.blended")}</th>
                    <th>{t("prices.source")}</th>
                  </tr>
                </thead>
                <tbody>
                  {g.offers.map((o) => (
                    <tr key={o.name} className={o.enabled ? undefined : "muted"}>
                      <td className="mono">
                        {o.name}
                        {!o.enabled && <span className="muted small"> · {t("common.disabled")}</span>}
                      </td>
                      <td className="num">{price(o.input_price)}</td>
                      <td className="num">{price(o.output_price)}</td>
                      <td className="num">{price(o.cache_read_price)}</td>
                      <td className="num">{price(o.cache_write_price)}</td>
                      <td className="num strong">
                        {o.blended_per_million == null ? (
                          <span className="muted">{t("prices.unknown")}</span>
                        ) : (
                          <>
                            {price(o.blended_per_million)}
                            {g.offers.length > 1 && o.blended_per_million === cheapest && (
                              <>
                                {" "}
                                <Badge tone="ok">{t("prices.cheapest")}</Badge>
                              </>
                            )}
                          </>
                        )}
                      </td>
                      <td>
                        <SourceBadge source={o.price_source} />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </Card>
        );
      })}
      <p className="small muted">{t("prices.note")}</p>
    </Page>
  );
}
