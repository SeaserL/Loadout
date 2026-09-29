// SPDX-License-Identifier: Apache-2.0

//! Latency statistics and tiers per account model (ExecPlan 004 §3.1–3.3, ADR-003 §4).
//!
//! Samples are recorded after a request completes (never on the selection path). The
//! control plane periodically calls [`LatencyBook::evaluate`], which applies hysteresis and
//! reports whether the ranking changed; route tables are then rebuilt from
//! [`LatencyBook::ranks`].

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

/// Samples kept per account model.
pub const RING_CAPACITY: usize = 64;
/// Minimum samples for a measured tier.
pub const MIN_SAMPLES: usize = 3;
/// Samples older than this are ignored.
pub const MAX_SAMPLE_AGE_MS: i64 = 7 * 24 * 3600 * 1000;
/// Recency weighting: a sample's weight halves every this many newer samples.
const HALF_LIFE_SAMPLES: f64 = 16.0;
/// Tier dead band around each threshold.
const TIER_MARGIN: f64 = 0.10;
/// Relative TPS change that may reorder targets…
const TPS_CHANGE: f64 = 0.20;
/// …once it persisted this many evaluations.
const TPS_PERSIST: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    UltraFast,
    Fast,
    Normal,
    Unmeasured,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UltraFast => "ULTRA_FAST",
            Self::Fast => "FAST",
            Self::Normal => "NORMAL",
            Self::Unmeasured => "UNMEASURED",
        }
    }

    /// Parses `ULTRA_FAST` / `ultra-fast` / `ultra_fast` etc.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().replace('-', "_").as_str() {
            "ultra_fast" => Some(Self::UltraFast),
            "fast" => Some(Self::Fast),
            "normal" => Some(Self::Normal),
            "unmeasured" => Some(Self::Unmeasured),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleSource {
    /// A real client request.
    Passive,
    /// A synthetic probe.
    Probe,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub ts_ms: i64,
    pub source: SampleSource,
    pub ttft_ms: u32,
    /// Output tokens per second after the first token; `None` for short outputs.
    pub tps: Option<f32>,
}

/// TTFT thresholds (P90) for the tiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TierThresholds {
    pub ultra_fast_ms: u32,
    pub fast_ms: u32,
}

impl Default for TierThresholds {
    fn default() -> Self {
        Self {
            ultra_fast_ms: 800,
            fast_ms: 2000,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LatencyStats {
    /// Samples the statistics are computed from.
    pub samples: usize,
    pub passive_samples: usize,
    pub probe_samples: usize,
    pub ttft_p50_ms: Option<u32>,
    pub ttft_p90_ms: Option<u32>,
    pub ttft_p95_ms: Option<u32>,
    pub tps_p50: Option<f32>,
    pub last_sample_ms: Option<i64>,
}

/// What routing uses: the stable (hysteresis-filtered) tier and throughput.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rank {
    pub tier: Tier,
    pub rank_tps: Option<f32>,
    /// The tier was set by the user.
    pub pinned: bool,
}

impl Default for Rank {
    fn default() -> Self {
        Self {
            tier: Tier::Unmeasured,
            rank_tps: None,
            pinned: false,
        }
    }
}

#[derive(Debug, Default)]
struct Entry {
    ring: VecDeque<Sample>,
    rank: Rank,
    measured_tier: Option<Tier>,
    tps_pending: u8,
}

/// Weighted quantile over `(value, weight)` pairs.
fn weighted_quantile(mut values: Vec<(f64, f64)>, q: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: f64 = values.iter().map(|v| v.1).sum();
    let target = q * total;
    let mut acc = 0.0;
    for (v, w) in &values {
        acc += w;
        if acc >= target {
            return Some(*v);
        }
    }
    values.last().map(|v| v.0)
}

fn compute(ring: &VecDeque<Sample>, now_ms: i64) -> LatencyStats {
    let fresh: Vec<&Sample> = ring
        .iter()
        .rev() // newest first
        .filter(|s| now_ms - s.ts_ms <= MAX_SAMPLE_AGE_MS)
        .collect();
    let passive = fresh
        .iter()
        .filter(|s| s.source == SampleSource::Passive)
        .count();
    let probe = fresh.len() - passive;
    // Real traffic wins once there is enough of it; probes use tiny prompts and are not
    // comparable with real contexts.
    let used: Vec<&Sample> = if passive >= MIN_SAMPLES {
        fresh
            .into_iter()
            .filter(|s| s.source == SampleSource::Passive)
            .collect()
    } else {
        fresh
    };
    let weight = |i: usize| 0.5f64.powf(i as f64 / HALF_LIFE_SAMPLES);
    let ttft: Vec<(f64, f64)> = used
        .iter()
        .enumerate()
        .map(|(i, s)| (f64::from(s.ttft_ms), weight(i)))
        .collect();
    let tps: Vec<(f64, f64)> = used
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.tps.map(|t| (f64::from(t), weight(i))))
        .collect();
    let q = |v: &Vec<(f64, f64)>, p: f64| weighted_quantile(v.clone(), p);
    LatencyStats {
        samples: used.len(),
        passive_samples: passive,
        probe_samples: probe,
        ttft_p50_ms: q(&ttft, 0.5).map(|v| v as u32),
        ttft_p90_ms: q(&ttft, 0.9).map(|v| v as u32),
        ttft_p95_ms: q(&ttft, 0.95).map(|v| v as u32),
        tps_p50: q(&tps, 0.5).map(|v| v as f32),
        last_sample_ms: ring.back().map(|s| s.ts_ms),
    }
}

/// Tier for a P90 TTFT, with a dead band around the thresholds relative to `current`.
fn classify(p90_ms: u32, current: Option<Tier>, th: TierThresholds) -> Tier {
    let p90 = f64::from(p90_ms);
    let bound = |threshold: u32, better: Tier| {
        let t = f64::from(threshold);
        match current {
            // Already at or above this boundary: harder to fall below it.
            Some(c) if c <= better => t * (1.0 + TIER_MARGIN),
            Some(_) => t * (1.0 - TIER_MARGIN),
            None => t,
        }
    };
    if p90 < bound(th.ultra_fast_ms, Tier::UltraFast) {
        Tier::UltraFast
    } else if p90 < bound(th.fast_ms, Tier::Fast) {
        Tier::Fast
    } else {
        Tier::Normal
    }
}

/// Latency samples and ranks per account model id.
#[derive(Debug, Default)]
pub struct LatencyBook {
    entries: Mutex<HashMap<Arc<str>, Entry>>,
}

impl LatencyBook {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Arc<str>, Entry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn record(&self, key: &str, sample: Sample) {
        let mut map = self.lock();
        if !map.contains_key(key) {
            map.insert(Arc::from(key), Entry::default());
        }
        let Some(entry) = map.get_mut(key) else {
            return;
        };
        if entry.ring.len() == RING_CAPACITY {
            entry.ring.pop_front();
        }
        // Keep time order (replayed history may arrive after live samples).
        let pos = entry
            .ring
            .iter()
            .rposition(|s| s.ts_ms <= sample.ts_ms)
            .map_or(0, |p| p + 1);
        entry.ring.insert(pos, sample);
    }

    pub fn stats(&self, key: &str, now_ms: i64) -> LatencyStats {
        self.lock()
            .get(key)
            .map(|e| compute(&e.ring, now_ms))
            .unwrap_or_default()
    }

    pub fn rank(&self, key: &str) -> Rank {
        self.lock().get(key).map(|e| e.rank).unwrap_or_default()
    }

    /// Stable ranks of every known account model.
    pub fn ranks(&self) -> HashMap<Arc<str>, Rank> {
        self.lock()
            .iter()
            .map(|(k, e)| (k.clone(), e.rank))
            .collect()
    }

    /// Recomputes ranks with hysteresis. `overrides` pins tiers chosen by the user; every
    /// key in `keys` gets an entry. Returns whether any rank changed.
    pub fn evaluate(
        &self,
        keys: &[Arc<str>],
        overrides: &HashMap<Arc<str>, Tier>,
        thresholds: TierThresholds,
        now_ms: i64,
    ) -> bool {
        let mut map = self.lock();
        let mut changed = false;
        for key in keys {
            if !map.contains_key(key) {
                map.insert(key.clone(), Entry::default());
            }
        }
        for (key, entry) in map.iter_mut() {
            let before = entry.rank;
            let stats = compute(&entry.ring, now_ms);
            let measured = match stats.ttft_p90_ms {
                Some(p90) if stats.samples >= MIN_SAMPLES => {
                    Some(classify(p90, entry.measured_tier, thresholds))
                }
                _ => None,
            };
            entry.measured_tier = measured;

            let tps = if measured.is_some() {
                stats.tps_p50
            } else {
                None
            };
            match (entry.rank.rank_tps, tps) {
                (_, None) => {
                    if measured.is_none() {
                        entry.rank.rank_tps = None;
                    }
                    entry.tps_pending = 0;
                }
                (None, Some(new)) => {
                    entry.rank.rank_tps = Some(new);
                    entry.tps_pending = 0;
                }
                (Some(old), Some(new)) => {
                    let delta = (f64::from(new) - f64::from(old)).abs() / f64::from(old).max(0.1);
                    if delta > TPS_CHANGE {
                        entry.tps_pending += 1;
                        if entry.tps_pending >= TPS_PERSIST {
                            entry.rank.rank_tps = Some(new);
                            entry.tps_pending = 0;
                        }
                    } else {
                        entry.tps_pending = 0;
                    }
                }
            }

            match overrides.get(key) {
                Some(&tier) => {
                    entry.rank.tier = tier;
                    entry.rank.pinned = true;
                }
                None => {
                    entry.rank.tier = measured.unwrap_or(Tier::Unmeasured);
                    entry.rank.pinned = false;
                }
            }
            changed |= entry.rank != before;
        }
        changed
    }

    /// Drops account models that no longer exist.
    pub fn retain(&self, mut keep: impl FnMut(&str) -> bool) {
        self.lock().retain(|k, _| keep(k));
    }
}

/// One entry to order for a virtual model or latency-ordered pool.
#[derive(Clone, Debug)]
pub struct RankedItem<K> {
    pub key: K,
    pub rank: Rank,
    pub degraded: bool,
    /// Tie-breaker: the user's provider / account / model order.
    pub base_order: usize,
}

/// Orders by tier (unmeasured last), healthy before degraded, higher throughput first, then
/// the user's order.
pub fn order_by_latency<K>(items: &mut [RankedItem<K>]) {
    items.sort_by(|a, b| {
        a.rank
            .tier
            .cmp(&b.rank.tier)
            .then(a.degraded.cmp(&b.degraded))
            .then_with(|| match (a.rank.rank_tps, b.rank.rank_tps) {
                (Some(x), Some(y)) => y.total_cmp(&x),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            })
            .then(a.base_order.cmp(&b.base_order))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 10_000_000_000;

    fn s(ts: i64, ttft: u32, tps: Option<f32>) -> Sample {
        Sample {
            ts_ms: ts,
            source: SampleSource::Passive,
            ttft_ms: ttft,
            tps,
        }
    }

    fn probe(ts: i64, ttft: u32) -> Sample {
        Sample {
            source: SampleSource::Probe,
            ..s(ts, ttft, None)
        }
    }

    fn eval(book: &LatencyBook, key: &str) -> bool {
        book.evaluate(
            &[Arc::from(key)],
            &HashMap::new(),
            TierThresholds::default(),
            NOW,
        )
    }

    #[test]
    fn weighted_quantiles_favour_recent_samples() {
        let book = LatencyBook::default();
        for i in 0..32 {
            book.record("m", s(NOW - 1000 + i, 3000, Some(50.0)));
        }
        for i in 0..32 {
            book.record("m", s(NOW - 500 + i, 500, Some(100.0)));
        }
        let st = book.stats("m", NOW);
        assert_eq!(st.samples, 64);
        assert_eq!(
            st.ttft_p50_ms,
            Some(500),
            "recent half dominates the weight"
        );
        assert_eq!(st.ttft_p95_ms, Some(3000));
        assert_eq!(st.tps_p50, Some(100.0));
    }

    #[test]
    fn passive_samples_replace_probes_and_old_samples_expire() {
        let book = LatencyBook::default();
        book.record("m", probe(NOW - 10, 300));
        book.record("m", probe(NOW - 9, 300));
        let st = book.stats("m", NOW);
        assert_eq!((st.samples, st.probe_samples), (2, 2));
        assert!(!eval(&book, "m"), "2 samples: still unmeasured");
        assert_eq!(book.rank("m").tier, Tier::Unmeasured);

        book.record("m", probe(NOW - 8, 300));
        assert!(eval(&book, "m"));
        assert_eq!(book.rank("m").tier, Tier::UltraFast);

        for i in 0..3 {
            book.record("m", s(NOW - 5 + i, 2500, None));
        }
        eval(&book, "m");
        let st = book.stats("m", NOW);
        assert_eq!(st.samples, 3, "only passive samples used");
        assert_eq!(book.rank("m").tier, Tier::Normal);

        let later = NOW + MAX_SAMPLE_AGE_MS + 1000;
        assert_eq!(book.stats("m", later).samples, 0);
        book.evaluate(
            &[Arc::from("m")],
            &HashMap::new(),
            TierThresholds::default(),
            later,
        );
        assert_eq!(book.rank("m").tier, Tier::Unmeasured);
    }

    #[test]
    fn tiers_have_a_dead_band() {
        let th = TierThresholds::default();
        assert_eq!(classify(790, None, th), Tier::UltraFast);
        assert_eq!(classify(850, Some(Tier::UltraFast), th), Tier::UltraFast);
        assert_eq!(classify(890, Some(Tier::UltraFast), th), Tier::Fast);
        assert_eq!(classify(750, Some(Tier::Fast), th), Tier::Fast);
        assert_eq!(classify(700, Some(Tier::Fast), th), Tier::UltraFast);
        assert_eq!(classify(2100, Some(Tier::Fast), th), Tier::Fast);
        assert_eq!(classify(1900, Some(Tier::Normal), th), Tier::Normal);
        assert_eq!(classify(1700, Some(Tier::Normal), th), Tier::Fast);
    }

    #[test]
    fn throughput_changes_must_persist() {
        let book = LatencyBook::default();
        for i in 0..3 {
            book.record("m", s(NOW - 100 + i, 500, Some(100.0)));
        }
        eval(&book, "m");
        assert_eq!(book.rank("m").rank_tps, Some(100.0));

        // Small change: ignored.
        for i in 0..40 {
            book.record("m", s(NOW - 50 + i, 500, Some(110.0)));
        }
        assert!(!eval(&book, "m"));
        assert_eq!(book.rank("m").rank_tps, Some(100.0));

        // Large change: applied on the third consecutive evaluation.
        for i in 0..64 {
            book.record("m", s(NOW - 10 + i / 8, 500, Some(200.0)));
        }
        assert!(!eval(&book, "m"));
        assert!(!eval(&book, "m"));
        assert!(eval(&book, "m"));
        assert_eq!(book.rank("m").rank_tps, Some(200.0));
    }

    #[test]
    fn overrides_pin_the_tier() {
        let book = LatencyBook::default();
        let key: Arc<str> = Arc::from("m");
        let overrides = HashMap::from([(key.clone(), Tier::UltraFast)]);
        assert!(book.evaluate(&[key], &overrides, TierThresholds::default(), NOW));
        let r = book.rank("m");
        assert_eq!(r.tier, Tier::UltraFast);
        assert!(r.pinned);
    }

    #[test]
    fn ordering_by_tier_health_and_throughput() {
        let rank = |tier, tps| Rank {
            tier,
            rank_tps: tps,
            pinned: false,
        };
        let item = |key, r, degraded, base_order| RankedItem {
            key,
            rank: r,
            degraded,
            base_order,
        };
        let mut items = vec![
            item("unmeasured", Rank::default(), false, 0),
            item("normal", rank(Tier::Normal, Some(300.0)), false, 1),
            item("fast-slow", rank(Tier::Fast, Some(40.0)), false, 2),
            item("fast-quick", rank(Tier::Fast, Some(90.0)), false, 3),
            item("fast-degraded", rank(Tier::Fast, Some(500.0)), true, 4),
            item("ultra", rank(Tier::UltraFast, None), false, 5),
        ];
        order_by_latency(&mut items);
        let keys: Vec<&str> = items.iter().map(|i| i.key).collect();
        assert_eq!(
            keys,
            vec![
                "ultra",
                "fast-quick",
                "fast-slow",
                "fast-degraded",
                "normal",
                "unmeasured"
            ]
        );
    }

    #[test]
    fn replayed_history_keeps_time_order() {
        let book = LatencyBook::default();
        book.record("m", s(NOW, 100, None));
        book.record("m", s(NOW - 10, 900, None));
        let st = book.stats("m", NOW);
        assert_eq!(st.last_sample_ms, Some(NOW));
    }
}
