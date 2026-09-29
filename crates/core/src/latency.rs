// SPDX-License-Identifier: Apache-2.0

//! Control-plane side of latency routing (ExecPlan 004): history replay, periodic tier
//! evaluation, active probes, tier overrides and pools.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use loadout_ipc::ErrorBody;
use loadout_ipc::api::{BenchResult, ModelInfo, PoolInfo, VirtualInfo};
use loadout_router::{
    RouteTarget, Sample, SampleSource, Tier, account_model_key, now_ms, validate_slug,
};
use loadout_storage::{ProbeSample, StorageError};
use tokio_util::sync::CancellationToken;

use crate::bench::{self, ProbeOutcome};
use crate::control::{ControlPlane, blocking, split_exact, storage};
use crate::route_build::{RESERVED_VIRTUAL_NAMES, VIRTUAL_DEFAULT, VIRTUAL_FAST, VIRTUAL_REALTIME};
use crate::settings::LatencySettings;

/// Evaluation period (the hysteresis "window", ADR-003 §4).
const EVALUATE_EVERY: Duration = Duration::from_secs(60);
/// Automatic probes run every this many evaluations.
const PROBE_EVERY_TICKS: u64 = 10;
/// Automatic probes per round.
const PROBES_PER_ROUND: usize = 4;
/// Concurrent probes.
const PROBE_CONCURRENCY: usize = 4;
/// Minimum output tokens for a throughput sample.
const MIN_TPS_TOKENS: i64 = 16;
/// History replayed at startup.
const HISTORY_PER_MODEL: usize = loadout_router::RING_CAPACITY;

fn history_sample(
    ts: i64,
    probe: bool,
    ttft_ms: i64,
    tokens: Option<i64>,
    tps: Option<f64>,
) -> Sample {
    Sample {
        ts_ms: ts,
        source: if probe {
            SampleSource::Probe
        } else {
            SampleSource::Passive
        },
        ttft_ms: u32::try_from(ttft_ms.max(0)).unwrap_or(u32::MAX),
        tps: tps
            .filter(|_| tokens.is_some_and(|t| t >= MIN_TPS_TOKENS))
            .map(|v| v as f32),
    }
}

/// A passive sample from a completed request, if it measured anything.
pub(crate) fn passive_sample(
    ts_ms: i64,
    status: &str,
    ttft_ms: Option<u64>,
    output_tokens: Option<u64>,
    output_tps: Option<f64>,
) -> Option<Sample> {
    if status != "ok" {
        return None;
    }
    Some(history_sample(
        ts_ms,
        false,
        i64::try_from(ttft_ms?).unwrap_or(i64::MAX),
        output_tokens.and_then(|t| i64::try_from(t).ok()),
        output_tps,
    ))
}

fn parse_tier(s: &str) -> Result<Tier, ErrorBody> {
    match Tier::parse(s) {
        Some(Tier::Unmeasured) | None => Err(ErrorBody::invalid_params(format!(
            "unknown tier {s:?}; use ultra-fast, fast, normal or auto"
        ))),
        Some(t) => Ok(t),
    }
}

impl ControlPlane {
    /// Rebuilds latency statistics from stored history (ExecPlan 004 §3.8).
    pub async fn seed_latency(&self) -> Result<usize, ErrorBody> {
        let db = self.db.clone();
        let since = now_ms() - loadout_router::MAX_SAMPLE_AGE_MS;
        let (history, probes) = blocking(move || {
            Ok((
                db.latency_history(since, HISTORY_PER_MODEL)
                    .map_err(storage)?,
                db.last_probe_times().map_err(storage)?,
            ))
        })
        .await?;
        for h in &history {
            self.latency.record(
                account_model_key(&h.route_target),
                history_sample(h.ts, h.probe, h.ttft_ms, h.output_tokens, h.output_tps),
            );
        }
        if let Ok(mut last) = self.last_probe.lock() {
            last.extend(probes);
        }
        self.evaluate_latency(true).await?;
        Ok(history.len())
    }

    /// Re-ranks every route with hysteresis; rebuilds the route table when the ranking
    /// changed (or when `force`).
    pub async fn evaluate_latency(&self, force: bool) -> Result<bool, ErrorBody> {
        let db = self.db.clone();
        let (settings, overrides) = blocking(move || {
            Ok((
                LatencySettings::load(&db).map_err(storage)?,
                db.tier_overrides().map_err(storage)?,
            ))
        })
        .await?;
        let overrides: HashMap<Arc<str>, Tier> = overrides
            .into_iter()
            .filter_map(|(id, t)| Some((Arc::from(id.as_str()), Tier::parse(&t)?)))
            .collect();
        let mut keys: Vec<Arc<str>> = self
            .routes
            .load()
            .targets()
            .iter()
            .map(|t| Arc::from(account_model_key(&t.id)))
            .collect();
        keys.dedup();
        let changed = self
            .latency
            .evaluate(&keys, &overrides, settings.thresholds, now_ms());
        if changed || force {
            self.rebuild_routes().await?;
        }
        Ok(changed)
    }

    /// One target per account model, in table order.
    fn unique_models<'a>(
        targets: impl Iterator<Item = &'a Arc<RouteTarget>>,
    ) -> Vec<Arc<RouteTarget>> {
        let mut out: Vec<Arc<RouteTarget>> = Vec::new();
        for t in targets {
            if !out
                .iter()
                .any(|o| account_model_key(&o.id) == account_model_key(&t.id))
            {
                out.push(t.clone());
            }
        }
        out
    }

    async fn probe_all(
        &self,
        targets: Vec<Arc<RouteTarget>>,
    ) -> Vec<(Arc<RouteTarget>, ProbeOutcome)> {
        let http = self.http.clone();
        let results: Vec<(Arc<RouteTarget>, ProbeOutcome)> =
            futures_util::stream::iter(targets.into_iter().map(|t| {
                let http = http.clone();
                async move {
                    let outcome = bench::probe(&http, &t).await;
                    (t, outcome)
                }
            }))
            .buffer_unordered(PROBE_CONCURRENCY)
            .collect()
            .await;

        let mut rows = Vec::with_capacity(results.len());
        for (t, o) in &results {
            let key = account_model_key(&t.id);
            if let Some(sample) = o.sample() {
                self.latency.record(key, sample);
            }
            if let Ok(mut last) = self.last_probe.lock() {
                last.insert(key.to_owned(), o.ts_ms);
            }
            tracing::info!(
                target = %t.exact_name(),
                ok = o.ok,
                ttft_ms = ?o.ttft_ms,
                error = ?o.error,
                "latency probe"
            );
            rows.push(ProbeSample {
                ts: o.ts_ms,
                route_target: t.id.to_string(),
                status: if o.ok { "ok" } else { "error" }.to_owned(),
                error: o.error.clone(),
                ttft_ms: o.ttft_ms.and_then(|v| i64::try_from(v).ok()),
                duration_ms: i64::try_from(o.duration_ms).unwrap_or(i64::MAX),
                output_tokens: o.output_tokens.and_then(|v| i64::try_from(v).ok()),
                output_tps: None,
            });
        }
        let db = self.db.clone();
        if let Err(e) = blocking(move || {
            rows.iter()
                .try_for_each(|r| db.insert_probe_sample(r))
                .map_err(storage)
        })
        .await
        {
            tracing::warn!(error = %e.message, "could not store probe results");
        }
        results
    }

    /// `bench.run`: probes the named routes now (all unmeasured ones when empty).
    pub async fn run_bench(&self, names: Vec<String>) -> Result<Vec<BenchResult>, ErrorBody> {
        let table = self.routes.load();
        let targets =
            if names.is_empty() {
                Self::unique_models(table.targets().iter().filter(|t| {
                    self.latency.rank(account_model_key(&t.id)).tier == Tier::Unmeasured
                }))
            } else {
                let mut picked: Vec<&Arc<RouteTarget>> = Vec::new();
                for name in &names {
                    let resolution = table
                        .resolve(name)
                        .map_err(|e| ErrorBody::not_found(e.to_string()))?;
                    if resolution.candidates.is_empty() {
                        return Err(ErrorBody::not_found(format!("{name} has no routes")));
                    }
                    // An exact name also resolves other accounts as failover candidates; measure
                    // only the named one.
                    let only_first = resolution.kind == loadout_router::ResolutionKind::Exact;
                    let first_account = resolution.candidates[0].account_id.clone();
                    picked.extend(
                        resolution
                            .candidates
                            .into_iter()
                            .filter(|t| !only_first || t.account_id == first_account),
                    );
                }
                Self::unique_models(picked.into_iter())
            };
        let results = self.probe_all(targets).await;
        self.evaluate_latency(false).await?;
        let mut out: Vec<BenchResult> = results
            .into_iter()
            .map(|(t, o)| BenchResult {
                name: t.exact_name(),
                protocol: t.protocol.as_str().to_owned(),
                ok: o.ok,
                ttft_ms: o.ttft_ms,
                duration_ms: o.duration_ms,
                output_tokens: o.output_tokens,
                error: o.error,
                tier: self
                    .latency
                    .rank(account_model_key(&t.id))
                    .tier
                    .as_str()
                    .to_owned(),
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// Probes up to [`PROBES_PER_ROUND`] routes whose last measurement is older than the
    /// probe interval (Harness §15: low frequency).
    pub async fn auto_probe(&self) -> Result<usize, ErrorBody> {
        let db = self.db.clone();
        let settings = blocking(move || LatencySettings::load(&db).map_err(storage)).await?;
        if !settings.auto_probe {
            return Ok(0);
        }
        let now = now_ms();
        let table = self.routes.load();
        let last_probe = self
            .last_probe
            .lock()
            .map(|m| m.clone())
            .unwrap_or_default();
        let mut due: Vec<(i64, Arc<RouteTarget>)> = Self::unique_models(table.targets().iter())
            .into_iter()
            .filter(|t| t.health.is_available(now))
            .filter(|t| {
                t.prices
                    .output
                    .is_none_or(|p| p <= settings.max_probe_output_price)
            })
            .filter_map(|t| {
                let key = account_model_key(&t.id);
                let last = self
                    .latency
                    .stats(key, now)
                    .last_sample_ms
                    .unwrap_or(0)
                    .max(last_probe.get(key).copied().unwrap_or(0));
                (now - last >= settings.probe_interval_ms).then_some((last, t))
            })
            .collect();
        due.sort_by_key(|(last, _)| *last);
        due.truncate(PROBES_PER_ROUND);
        let n = due.len();
        if n > 0 {
            self.probe_all(due.into_iter().map(|(_, t)| t).collect())
                .await;
            self.evaluate_latency(false).await?;
        }
        Ok(n)
    }

    pub async fn set_model_tier(
        &self,
        name: String,
        tier: Option<String>,
    ) -> Result<ModelInfo, ErrorBody> {
        let tier = match tier.as_deref() {
            None | Some("auto") => None,
            Some(t) => Some(parse_tier(t)?.as_str()),
        };
        let info = self
            .update_model(name, move |db, m| {
                db.set_tier_override(&m.id, tier).map(|_| ())
            })
            .await?;
        self.evaluate_latency(true).await?;
        Ok(info)
    }

    // ---- pools --------------------------------------------------------------------------

    pub async fn save_pool(
        &self,
        name: String,
        ordering: String,
        members: Vec<String>,
    ) -> Result<PoolInfo, ErrorBody> {
        validate_slug(&name).map_err(|e| ErrorBody::invalid_params(format!("pool name: {e}")))?;
        if RESERVED_VIRTUAL_NAMES.contains(&name.as_str()) {
            return Err(ErrorBody::invalid_params(format!(
                "loadout/{name} is reserved for a built-in virtual model"
            )));
        }
        if !matches!(ordering.as_str(), "manual" | "latency") {
            return Err(ErrorBody::invalid_params(
                "ordering must be manual or latency",
            ));
        }
        if members.is_empty() {
            return Err(ErrorBody::invalid_params("a pool needs at least one model"));
        }
        let db = self.db.clone();
        let info = blocking(move || {
            let mut ids = Vec::with_capacity(members.len());
            for m in &members {
                let (ps, al, model) = split_exact(m)?;
                let p = Self::load_provider(&db, ps)?;
                let a = Self::load_account(&db, &p, al)?;
                let am = db
                    .account_models()
                    .find(&a.id, model)
                    .map_err(storage)?
                    .ok_or_else(|| ErrorBody::not_found(format!("model {m} not found")))?;
                ids.push(am.id);
            }
            let pool = db.save_pool(&name, &ordering, &ids).map_err(storage)?;
            pool_info(&db, pool).map_err(storage)
        })
        .await?;
        self.rebuild_routes().await?;
        Ok(info)
    }

    pub async fn list_pools(&self) -> Result<Vec<PoolInfo>, ErrorBody> {
        let db = self.db.clone();
        blocking(move || {
            db.pools()
                .and_then(|pools| pools.into_iter().map(|p| pool_info(&db, p)).collect())
                .map_err(storage)
        })
        .await
    }

    pub async fn remove_pool(&self, name: String) -> Result<bool, ErrorBody> {
        let db = self.db.clone();
        let removed = blocking(move || db.delete_pool(&name).map_err(storage)).await?;
        if removed {
            self.rebuild_routes().await?;
        }
        Ok(removed)
    }

    /// Virtual models and pools with their current route order (one entry per account
    /// model).
    pub async fn virtuals(&self) -> Result<Vec<VirtualInfo>, ErrorBody> {
        let db = self.db.clone();
        let pools = blocking(move || db.pools().map_err(storage)).await?;
        let table = self.routes.load();
        let builtin = [VIRTUAL_REALTIME, VIRTUAL_FAST, VIRTUAL_DEFAULT];
        Ok(table
            .virtuals()
            .into_iter()
            .map(|(name, targets)| {
                let pool = pools.iter().find(|p| p.name == name);
                VirtualInfo {
                    name: format!("loadout/{name}"),
                    kind: if builtin.contains(&name) {
                        "builtin"
                    } else {
                        "pool"
                    }
                    .to_owned(),
                    ordering: pool.map(|p| p.ordering.clone()),
                    routes: Self::unique_models(targets.into_iter())
                        .iter()
                        .map(|t| t.exact_name())
                        .collect(),
                }
            })
            .collect())
    }
}

fn pool_info(
    db: &loadout_storage::Database,
    pool: loadout_storage::Pool,
) -> Result<PoolInfo, StorageError> {
    let names = db.model_names()?;
    Ok(PoolInfo {
        name: pool.name,
        ordering: pool.ordering,
        members: pool
            .members
            .iter()
            .filter_map(|id| names.get(id).cloned())
            .collect(),
    })
}

/// Background task: evaluate every minute, probe every ten (restartable).
pub(crate) async fn run(
    control: Arc<ControlPlane>,
    token: CancellationToken,
) -> Result<(), String> {
    let mut tick: u64 = 0;
    loop {
        tokio::select! {
            () = tokio::time::sleep(EVALUATE_EVERY) => {}
            () = token.cancelled() => return Ok(()),
        }
        tick += 1;
        if let Err(e) = control.evaluate_latency(false).await {
            tracing::warn!(error = %e.message, "latency evaluation failed");
        }
        if tick % PROBE_EVERY_TICKS == 2 {
            tokio::select! {
                r = control.auto_probe() => match r {
                    Ok(0) => {}
                    Ok(n) => tracing::info!(probes = n, "automatic latency probes done"),
                    Err(e) => tracing::warn!(error = %e.message, "automatic probes failed"),
                },
                () = token.cancelled() => return Ok(()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passive_samples_need_success_and_ttft() {
        assert!(passive_sample(1, "error", Some(10), Some(100), Some(50.0)).is_none());
        assert!(passive_sample(1, "ok", None, Some(100), Some(50.0)).is_none());
        let s = passive_sample(1, "ok", Some(300), Some(100), Some(50.0)).expect("sample");
        assert_eq!((s.ttft_ms, s.tps), (300, Some(50.0)));
        let short = passive_sample(1, "ok", Some(300), Some(3), Some(50.0)).expect("sample");
        assert_eq!(
            short.tps, None,
            "short outputs say nothing about throughput"
        );
    }

    #[test]
    fn tiers_parse_for_overrides() {
        assert_eq!(parse_tier("ultra-fast").expect("tier"), Tier::UltraFast);
        assert!(parse_tier("unmeasured").is_err());
        assert!(parse_tier("warp").is_err());
    }
}
