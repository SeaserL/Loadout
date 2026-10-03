// SPDX-License-Identifier: Apache-2.0

//! Cross-account price comparison (ExecPlan 011). Groups every account model by a
//! normalized model key and blends its prices with the user's own token mix. Unknown
//! prices stay unknown; they are never treated as free.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use loadout_ipc::api::{PriceGroup, PriceOffer, PriceTokenMix};
use loadout_router::Prices;
use loadout_storage::{Database, ModelTokenTotals, PricedModel, StorageError};

/// How far back the token mix looks.
pub(crate) const MIX_WINDOW_MS: i64 = 30 * 86_400_000;

/// Token mix used when there is no request history: 75% input, 25% output.
const DEFAULT_INPUT: i64 = 750_000;
const DEFAULT_OUTPUT: i64 = 250_000;

/// Normalized model key: lowercase, without a vendor prefix (`anthropic/`), a variant
/// suffix (`:free`) or a trailing release date (`-20250929`, `-2025-09-29`, `@20250929`),
/// with `.` spelled `-` (`claude-sonnet-4.5` = `claude-sonnet-4-5`).
pub(crate) fn normalize_model_key(id: &str) -> String {
    let lower = id.trim().to_ascii_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    let base = base.split(':').next().unwrap_or(base);
    strip_date(base).replace('.', "-")
}

fn strip_date(s: &str) -> &str {
    let b = s.as_bytes();
    let digits = |r: &[u8]| r.iter().all(u8::is_ascii_digit);
    // -YYYYMMDD / @YYYYMMDD
    if b.len() > 9 {
        let (head, tail) = b.split_at(b.len() - 9);
        if matches!(tail[0], b'-' | b'@') && tail[1..3] == *b"20" && digits(&tail[1..]) {
            return &s[..head.len()];
        }
    }
    // -YYYY-MM-DD
    if b.len() > 11 {
        let (head, tail) = b.split_at(b.len() - 11);
        if tail[0] == b'-'
            && tail[1..3] == *b"20"
            && digits(&tail[1..5])
            && tail[5] == b'-'
            && digits(&tail[6..8])
            && tail[8] == b'-'
            && digits(&tail[9..11])
        {
            return &s[..head.len()];
        }
    }
    s
}

#[derive(Clone, Copy, Default)]
struct Totals {
    requests: i64,
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
}

impl Totals {
    fn add(&mut self, t: &ModelTokenTotals) {
        self.requests += t.requests;
        self.input += t.input_tokens.max(0);
        self.output += t.output_tokens.max(0);
        self.cache_read += t.cache_read_tokens.max(0);
        self.cache_write += t.cache_write_tokens.max(0);
    }

    fn has_tokens(&self) -> bool {
        self.input + self.output > 0
    }

    fn mix(&self, source: &str) -> PriceTokenMix {
        PriceTokenMix {
            source: source.into(),
            requests: self.requests,
            input_tokens: self.input,
            output_tokens: self.output,
            cache_read_tokens: self.cache_read,
            cache_write_tokens: self.cache_write,
        }
    }
}

/// USD per million tokens at `mix`; `None` when the input or output price is unknown.
pub(crate) fn blended_per_million(prices: &Prices, mix: &PriceTokenMix) -> Option<f64> {
    let total = mix.input_tokens + mix.output_tokens;
    if total <= 0 {
        return None;
    }
    let n = |v: i64| u64::try_from(v).unwrap_or(0);
    let cost = prices.cost(
        n(mix.input_tokens),
        n(mix.output_tokens),
        n(mix.cache_read_tokens),
        n(mix.cache_write_tokens),
    )?;
    Some(cost * 1_000_000.0 / total as f64)
}

fn offer_order(a: &PriceOffer, b: &PriceOffer) -> Ordering {
    match (a.blended_per_million, b.blended_per_million) {
        (Some(x), Some(y)) => x.total_cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
    .then_with(|| a.name.cmp(&b.name))
}

/// Groups `models` by normalized key and prices each offer at the group's token mix.
/// `filter` keeps groups whose key contains the normalized filter, or that serve the
/// filter verbatim as an upstream id.
pub(crate) fn compare(
    models: &[PricedModel],
    totals: &[ModelTokenTotals],
    filter: Option<&str>,
) -> Vec<PriceGroup> {
    let mut usage: BTreeMap<String, Totals> = BTreeMap::new();
    let mut all = Totals::default();
    for t in totals {
        usage
            .entry(normalize_model_key(&t.upstream_model))
            .or_default()
            .add(t);
        all.add(t);
    }

    let mut groups: BTreeMap<String, Vec<&PricedModel>> = BTreeMap::new();
    for m in models {
        groups
            .entry(normalize_model_key(&m.model.upstream_model_id))
            .or_default()
            .push(m);
    }

    let filter = filter.map(str::trim).filter(|f| !f.is_empty());
    let needle = filter.map(normalize_model_key);
    groups
        .into_iter()
        .filter(|(key, members)| match (filter, needle.as_deref()) {
            (Some(raw), Some(needle)) => {
                key.contains(needle)
                    || members
                        .iter()
                        .any(|m| m.model.upstream_model_id.eq_ignore_ascii_case(raw))
            }
            _ => true,
        })
        .map(|(key, members)| {
            let mix = match usage.get(&key) {
                Some(t) if t.has_tokens() => t.mix("model"),
                _ if all.has_tokens() => all.mix("all"),
                _ => PriceTokenMix {
                    source: "default".into(),
                    input_tokens: DEFAULT_INPUT,
                    output_tokens: DEFAULT_OUTPUT,
                    ..PriceTokenMix::default()
                },
            };
            let mut offers: Vec<PriceOffer> = members
                .into_iter()
                .map(|m| {
                    let am = &m.model;
                    let prices = Prices {
                        input: am.input_price,
                        output: am.output_price,
                        cache_read: am.cache_read_price,
                        cache_write: am.cache_write_price,
                    };
                    PriceOffer {
                        name: format!(
                            "{}/{}/{}",
                            m.provider_slug, m.account_alias, am.upstream_model_id
                        ),
                        provider: m.provider_slug.clone(),
                        account: m.account_alias.clone(),
                        upstream_model: am.upstream_model_id.clone(),
                        enabled: m.account_enabled && am.enabled,
                        input_price: am.input_price,
                        output_price: am.output_price,
                        cache_read_price: am.cache_read_price,
                        cache_write_price: am.cache_write_price,
                        price_source: am.price_source.clone(),
                        price_updated_at_ms: am.price_updated_at,
                        blended_per_million: blended_per_million(&prices, &mix),
                    }
                })
                .collect();
            offers.sort_by(offer_order);
            PriceGroup { key, mix, offers }
        })
        .collect()
}

/// Reads the account models and the last 30 days of token totals, then compares.
pub(crate) fn compare_from_db(
    db: &Database,
    filter: Option<&str>,
    now_ms: i64,
) -> Result<Vec<PriceGroup>, StorageError> {
    let models = db.priced_models()?;
    let totals = db.token_totals_by_model(now_ms - MIX_WINDOW_MS)?;
    Ok(compare(&models, &totals, filter))
}

#[cfg(test)]
mod tests {
    use loadout_storage::AccountModel;

    use super::*;

    fn priced(provider: &str, model: &str, input: Option<f64>, output: Option<f64>) -> PricedModel {
        PricedModel {
            provider_slug: provider.into(),
            account_alias: "main".into(),
            account_enabled: true,
            model: AccountModel {
                id: format!("{provider}-{model}"),
                account_id: provider.into(),
                upstream_model_id: model.into(),
                model_id: None,
                protocols_json: "[]".into(),
                context_window: None,
                max_output_tokens: None,
                capabilities_json: "[]".into(),
                input_price: input,
                output_price: output,
                cache_read_price: None,
                cache_write_price: None,
                enabled: true,
                source: "catalog".into(),
                tier_override: None,
                price_source: input.map(|_| "catalog".into()),
                price_updated_at: None,
            },
        }
    }

    fn totals(model: &str, input: i64, output: i64, cache_read: i64) -> ModelTokenTotals {
        ModelTokenTotals {
            upstream_model: model.into(),
            requests: 1,
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cache_read,
            cache_write_tokens: 0,
        }
    }

    #[test]
    fn normalizes_vendor_suffix_and_dates() {
        for (id, key) in [
            ("anthropic/claude-sonnet-4.5", "claude-sonnet-4-5"),
            ("claude-sonnet-4-5-20250929", "claude-sonnet-4-5"),
            ("claude-sonnet-4-5@20250929", "claude-sonnet-4-5"),
            ("openai/gpt-4o-2024-08-06", "gpt-4o"),
            ("deepseek/DeepSeek-R1:free", "deepseek-r1"),
            ("accounts/fireworks/models/llama-v3", "llama-v3"),
            ("gpt-4o-mini", "gpt-4o-mini"),
            // Not dates: kept.
            ("qwen-72b-12345678", "qwen-72b-12345678"),
            ("model-v2", "model-v2"),
            ("", ""),
        ] {
            assert_eq!(normalize_model_key(id), key, "{id}");
        }
    }

    #[test]
    fn groups_across_providers_and_sorts_cheapest_first() {
        let models = [
            priced(
                "anthropic",
                "claude-sonnet-4-5-20250929",
                Some(3.0),
                Some(15.0),
            ),
            priced(
                "openrouter",
                "anthropic/claude-sonnet-4.5",
                Some(2.0),
                Some(10.0),
            ),
            priced("relay", "claude-sonnet-4-5", None, Some(1.0)),
            priced("openai", "gpt-4o", Some(2.5), Some(10.0)),
        ];
        let groups = compare(&models, &[], None);
        assert_eq!(groups.len(), 2);
        let sonnet = &groups[0];
        assert_eq!(sonnet.key, "claude-sonnet-4-5");
        assert_eq!(sonnet.mix.source, "default");
        let names: Vec<_> = sonnet.offers.iter().map(|o| o.provider.as_str()).collect();
        assert_eq!(names, ["openrouter", "anthropic", "relay"]);
        // Default mix 75/25: 0.75 * 2 + 0.25 * 10 = 4.
        assert!((sonnet.offers[0].blended_per_million.expect("known") - 4.0).abs() < 1e-9);
        assert_eq!(
            sonnet.offers[2].blended_per_million, None,
            "a missing price is unknown, not free"
        );
        assert_eq!(sonnet.offers[2].price_source, None);
    }

    #[test]
    fn uses_own_token_mix_with_cache() {
        let mut cached = priced("a", "m", Some(10.0), Some(20.0));
        cached.model.cache_read_price = Some(1.0);
        let plain = priced("b", "vendor/m", Some(10.0), Some(20.0));
        // 1M input of which 900k cached, 0 output.
        let usage = [totals("m", 1_000_000, 0, 900_000)];
        let groups = compare(&[cached, plain], &usage, None);
        let g = &groups[0];
        assert_eq!(g.mix.source, "model");
        // Cached: 100k * 10 + 900k * 1 = 1.9 per 1M; plain account bills cache at input.
        assert_eq!(g.offers[0].provider, "a");
        assert!((g.offers[0].blended_per_million.expect("a") - 1.9).abs() < 1e-9);
        assert!((g.offers[1].blended_per_million.expect("b") - 10.0).abs() < 1e-9);
    }

    #[test]
    fn falls_back_to_overall_mix_and_filters() {
        let models = [
            priced("a", "m1", Some(1.0), Some(3.0)),
            priced("a", "other-model", Some(1.0), Some(3.0)),
        ];
        let usage = [totals("other-model", 500, 500, 0)];
        let groups = compare(&models, &usage, Some("vendor/M1"));
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].key, "m1");
        assert_eq!(groups[0].mix.source, "all");
        assert!((groups[0].offers[0].blended_per_million.expect("p") - 2.0).abs() < 1e-9);
        assert_eq!(compare(&models, &usage, Some("model")).len(), 1);
        assert_eq!(compare(&models, &usage, Some("  ")).len(), 2);
        assert!(compare(&models, &usage, Some("nope")).is_empty());
    }
}
