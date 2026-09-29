// SPDX-License-Identifier: Apache-2.0

//! Per-client policies (ADR-011 §2, ExecPlan 006 §3.1): default model, allowed models,
//! daily token / cost limits and a request rate limit. All checks are in memory; usage is
//! recorded after each request by the daemon's telemetry sink.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use arc_swap::ArcSwap;

use crate::ClientId;

const DAY_MS: i64 = 86_400_000;
const MINUTE_MS: i64 = 60_000;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientPolicy {
    /// Used when the requested model is not a Loadout name (or is empty).
    pub default_model: Option<String>,
    /// Exact names or prefixes ending in `*`; empty allows everything.
    pub allowed_models: Vec<String>,
    pub daily_token_limit: Option<u64>,
    pub daily_cost_limit_usd: Option<f64>,
    pub requests_per_minute: Option<u32>,
}

impl ClientPolicy {
    pub fn allows(&self, model: &str) -> bool {
        self.allowed_models.is_empty()
            || self
                .allowed_models
                .iter()
                .any(|p| match p.strip_suffix('*') {
                    Some(prefix) => model.starts_with(prefix),
                    None => model == p,
                })
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Why a request was refused.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum PolicyViolation {
    #[error("this client key may not use model {0}")]
    ModelNotAllowed(String),
    #[error("this client key is limited to {0} requests per minute")]
    RateLimited(u32),
    #[error("this client key reached its daily token limit ({0} tokens, resets at 00:00 UTC)")]
    DailyTokens(u64),
    #[error("this client key reached its daily cost limit (${0:.2}, resets at 00:00 UTC)")]
    DailyCost(f64),
}

#[derive(Debug, Default)]
struct Counters {
    day: i64,
    tokens: u64,
    cost: f64,
    minute: i64,
    minute_requests: u32,
}

impl Counters {
    /// Moves to the day of `now_ms` if it is later than the current one.
    fn roll_day(&mut self, now_ms: i64) {
        let day = now_ms.div_euclid(DAY_MS);
        if day > self.day {
            self.day = day;
            self.tokens = 0;
            self.cost = 0.0;
        }
    }

    fn roll(&mut self, now_ms: i64) {
        self.roll_day(now_ms);
        let minute = now_ms.div_euclid(MINUTE_MS);
        if minute != self.minute {
            self.minute = minute;
            self.minute_requests = 0;
        }
    }
}

/// Today's usage of one client, for display.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ClientUsageToday {
    pub tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Default)]
pub struct ClientPolicies {
    policies: ArcSwap<HashMap<ClientId, Arc<ClientPolicy>>>,
    counters: Mutex<HashMap<ClientId, Counters>>,
}

impl ClientPolicies {
    fn counters(&self) -> std::sync::MutexGuard<'_, HashMap<ClientId, Counters>> {
        self.counters.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Replaces all policies (clients without an entry have none).
    pub fn replace(&self, policies: impl IntoIterator<Item = (ClientId, ClientPolicy)>) {
        self.policies.store(Arc::new(
            policies
                .into_iter()
                .filter(|(_, p)| !p.is_empty())
                .map(|(id, p)| (id, Arc::new(p)))
                .collect(),
        ));
    }

    pub fn get(&self, client: &str) -> Option<Arc<ClientPolicy>> {
        self.policies.load().get(client).cloned()
    }

    /// The model to route: the client's default when the request names no Loadout model.
    pub fn effective_model<'a>(
        &self,
        policy: Option<&'a ClientPolicy>,
        requested: &'a str,
    ) -> &'a str {
        match policy.and_then(|p| p.default_model.as_deref()) {
            Some(default)
                if requested.is_empty() || loadout_router::parse_name(requested).is_none() =>
            {
                default
            }
            _ => requested,
        }
    }

    /// Checks the model and the limits, counting the request against the rate limit.
    pub fn admit(
        &self,
        client: &ClientId,
        policy: Option<&ClientPolicy>,
        model: &str,
        now_ms: i64,
    ) -> Result<(), PolicyViolation> {
        let Some(p) = policy else {
            return Ok(());
        };
        if !p.allows(model) {
            return Err(PolicyViolation::ModelNotAllowed(model.to_owned()));
        }
        if p.daily_token_limit.is_none()
            && p.daily_cost_limit_usd.is_none()
            && p.requests_per_minute.is_none()
        {
            return Ok(());
        }
        let mut map = self.counters();
        let c = map.entry(client.clone()).or_default();
        c.roll(now_ms);
        if let Some(limit) = p.daily_token_limit
            && c.tokens >= limit
        {
            return Err(PolicyViolation::DailyTokens(limit));
        }
        if let Some(limit) = p.daily_cost_limit_usd
            && c.cost >= limit
        {
            return Err(PolicyViolation::DailyCost(limit));
        }
        if let Some(rpm) = p.requests_per_minute {
            if c.minute_requests >= rpm {
                return Err(PolicyViolation::RateLimited(rpm));
            }
            c.minute_requests += 1;
        }
        Ok(())
    }

    /// Adds a finished request's usage (called after the response).
    pub fn record(&self, client: &str, tokens: u64, cost_usd: f64, ts_ms: i64) {
        let mut map = self.counters();
        if !map.contains_key(client) {
            map.insert(Arc::from(client), Counters::default());
        }
        let Some(c) = map.get_mut(client) else {
            return;
        };
        c.roll_day(ts_ms);
        if ts_ms.div_euclid(DAY_MS) == c.day {
            c.tokens = c.tokens.saturating_add(tokens);
            c.cost += cost_usd;
        }
    }

    pub fn usage_today(&self, client: &str, now_ms: i64) -> ClientUsageToday {
        let mut map = self.counters();
        match map.get_mut(client) {
            Some(c) => {
                c.roll(now_ms);
                ClientUsageToday {
                    tokens: c.tokens,
                    cost_usd: c.cost,
                }
            }
            None => ClientUsageToday::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_790_000_000_000;

    fn id(s: &str) -> ClientId {
        Arc::from(s)
    }

    #[test]
    fn patterns_and_default_model() {
        let p = ClientPolicy {
            default_model: Some("loadout/fast".into()),
            allowed_models: vec!["loadout/*".into(), "deepseek/main/deepseek-chat".into()],
            ..ClientPolicy::default()
        };
        assert!(p.allows("loadout/realtime"));
        assert!(p.allows("deepseek/main/deepseek-chat"));
        assert!(!p.allows("deepseek/main/deepseek-reasoner"));
        assert!(ClientPolicy::default().allows("anything"));

        let policies = ClientPolicies::default();
        assert_eq!(policies.effective_model(Some(&p), "gpt-4o"), "loadout/fast");
        assert_eq!(policies.effective_model(Some(&p), ""), "loadout/fast");
        assert_eq!(
            policies.effective_model(Some(&p), "loadout/default"),
            "loadout/default"
        );
        assert_eq!(policies.effective_model(None, "gpt-4o"), "gpt-4o");
    }

    #[test]
    fn limits_rate_and_daily_usage() {
        let policies = ClientPolicies::default();
        let p = ClientPolicy {
            daily_token_limit: Some(1000),
            requests_per_minute: Some(2),
            ..ClientPolicy::default()
        };
        policies.replace([(id("c"), p.clone())]);
        let c = id("c");
        let got = policies.get("c").expect("policy");
        assert!(policies.admit(&c, Some(&got), "m", T0).is_ok());
        assert!(policies.admit(&c, Some(&got), "m", T0 + 1).is_ok());
        assert_eq!(
            policies.admit(&c, Some(&got), "m", T0 + 2),
            Err(PolicyViolation::RateLimited(2))
        );
        assert!(
            policies.admit(&c, Some(&got), "m", T0 + MINUTE_MS).is_ok(),
            "next minute"
        );

        policies.record("c", 1200, 0.5, T0 + MINUTE_MS);
        assert_eq!(
            policies.admit(&c, Some(&got), "m", T0 + 2 * MINUTE_MS),
            Err(PolicyViolation::DailyTokens(1000))
        );
        assert_eq!(policies.usage_today("c", T0 + 2 * MINUTE_MS).tokens, 1200);
        let tomorrow = (T0.div_euclid(DAY_MS) + 1) * DAY_MS;
        assert!(
            policies.admit(&c, Some(&got), "m", tomorrow).is_ok(),
            "new day"
        );
        assert_eq!(policies.usage_today("c", tomorrow).tokens, 0);

        let cost = ClientPolicy {
            daily_cost_limit_usd: Some(1.0),
            ..ClientPolicy::default()
        };
        policies.record("d", 10, 1.5, T0);
        assert_eq!(
            policies.admit(&id("d"), Some(&cost), "m", T0),
            Err(PolicyViolation::DailyCost(1.0))
        );
        // Usage from a previous day does not count.
        policies.record("e", 10, 5.0, T0 - DAY_MS);
        assert!(policies.admit(&id("e"), Some(&cost), "m", T0).is_ok());

        // Recording an older request does not reset the rate window.
        let rpm = ClientPolicy {
            requests_per_minute: Some(1),
            ..ClientPolicy::default()
        };
        assert!(policies.admit(&id("f"), Some(&rpm), "m", T0).is_ok());
        policies.record("f", 1, 0.0, T0 - MINUTE_MS);
        assert!(policies.admit(&id("f"), Some(&rpm), "m", T0 + 1).is_err());
    }

    #[test]
    fn empty_policies_are_dropped_and_model_filter_applies() {
        let policies = ClientPolicies::default();
        policies.replace([
            (id("a"), ClientPolicy::default()),
            (
                id("b"),
                ClientPolicy {
                    allowed_models: vec!["x/*".into()],
                    ..ClientPolicy::default()
                },
            ),
        ]);
        assert!(policies.get("a").is_none());
        let b = policies.get("b").expect("b");
        assert_eq!(
            policies.admit(&id("b"), Some(&b), "y/m", T0),
            Err(PolicyViolation::ModelNotAllowed("y/m".into()))
        );
    }
}
