// SPDX-License-Identifier: Apache-2.0

//! Per-RouteTarget circuit breaker (ADR-003 §7, glossary §5.2). Lock-free on the request
//! path: atomics only.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use loadout_provider_sdk::FailureKind;

use crate::TargetId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum HealthState {
    Unknown = 0,
    Healthy = 1,
    Cooldown = 2,
    RateLimited = 3,
    QuotaExhausted = 4,
    AuthFailed = 5,
}

impl HealthState {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Healthy,
            2 => Self::Cooldown,
            3 => Self::RateLimited,
            4 => Self::QuotaExhausted,
            5 => Self::AuthFailed,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "UNKNOWN",
            Self::Healthy => "HEALTHY",
            Self::Cooldown => "COOLDOWN",
            Self::RateLimited => "RATE_LIMITED",
            Self::QuotaExhausted => "QUOTA_EXHAUSTED",
            Self::AuthFailed => "AUTH_FAILED",
        }
    }
}

const COOLDOWN_BASE: Duration = Duration::from_secs(5);
const COOLDOWN_MAX: Duration = Duration::from_secs(300);
const RATE_LIMIT_BASE: Duration = Duration::from_secs(30);
const RATE_LIMIT_MAX: Duration = Duration::from_secs(600);
const QUOTA_DEFAULT: Duration = Duration::from_secs(3600);
/// Rejected credentials stay blocked this long, then one request probes again.
const AUTH_RETRY: Duration = Duration::from_secs(3600);

/// Wall-clock milliseconds, the time base for breaker deadlines.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Default)]
pub struct TargetHealth {
    state: AtomicU8,
    /// Unavailable until this time (ms); 0 when available.
    until_ms: AtomicI64,
    consecutive_failures: AtomicU32,
    in_flight: AtomicU32,
    successes: AtomicU64,
    failures: AtomicU64,
    last_success_ms: AtomicI64,
    last_failure_ms: AtomicI64,
    last_error: Mutex<Option<String>>,
}

/// Point-in-time view for status displays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthSnapshot {
    pub state: HealthState,
    pub until_ms: Option<i64>,
    pub consecutive_failures: u32,
    pub in_flight: u32,
    pub successes: u64,
    pub failures: u64,
    pub last_success_ms: Option<i64>,
    pub last_failure_ms: Option<i64>,
    pub last_error: Option<String>,
}

fn backoff(base: Duration, max: Duration, failures: u32) -> Duration {
    let factor = 1u32 << failures.saturating_sub(1).min(16);
    base.saturating_mul(factor).min(max)
}

impl TargetHealth {
    pub fn state(&self) -> HealthState {
        HealthState::from_u8(self.state.load(Ordering::Acquire))
    }

    /// Whether a request may be sent now. Once a blocking state expires the target is
    /// available again (half-open): the next result decides its state.
    pub fn is_available(&self, now_ms: i64) -> bool {
        match self.state() {
            HealthState::Unknown | HealthState::Healthy => true,
            _ => self.until_ms.load(Ordering::Acquire) <= now_ms,
        }
    }

    pub fn record_success(&self, now_ms: i64) {
        self.state
            .store(HealthState::Healthy as u8, Ordering::Release);
        self.until_ms.store(0, Ordering::Release);
        self.consecutive_failures.store(0, Ordering::Release);
        self.successes.fetch_add(1, Ordering::Relaxed);
        self.last_success_ms.store(now_ms, Ordering::Relaxed);
    }

    /// Updates the breaker for a failure that should affect routing. Request-level errors
    /// (invalid request) must not be recorded here.
    pub fn record_failure(&self, kind: FailureKind, message: &str, now_ms: i64) {
        let failures = self.consecutive_failures.fetch_add(1, Ordering::AcqRel) + 1;
        let (state, wait) = match kind {
            FailureKind::Auth => (HealthState::AuthFailed, AUTH_RETRY),
            FailureKind::RateLimited { retry_after } => (
                HealthState::RateLimited,
                retry_after.unwrap_or_else(|| backoff(RATE_LIMIT_BASE, RATE_LIMIT_MAX, failures)),
            ),
            FailureKind::QuotaExhausted { reset_after } => (
                HealthState::QuotaExhausted,
                reset_after.unwrap_or(QUOTA_DEFAULT),
            ),
            FailureKind::Server
            | FailureKind::Timeout
            | FailureKind::Connect
            | FailureKind::Other => (
                HealthState::Cooldown,
                backoff(COOLDOWN_BASE, COOLDOWN_MAX, failures),
            ),
            FailureKind::ModelNotFound => (HealthState::Cooldown, COOLDOWN_MAX),
            FailureKind::InvalidRequest => return,
        };
        self.state.store(state as u8, Ordering::Release);
        self.until_ms.store(
            now_ms.saturating_add(wait.as_millis() as i64),
            Ordering::Release,
        );
        self.failures.fetch_add(1, Ordering::Relaxed);
        self.last_failure_ms.store(now_ms, Ordering::Relaxed);
        if let Ok(mut e) = self.last_error.lock() {
            *e = Some(message.to_owned());
        }
    }

    /// Marks quota exhausted until a known reset (from a Usage driver).
    pub fn mark_quota_exhausted(&self, until_ms: i64) {
        self.state
            .store(HealthState::QuotaExhausted as u8, Ordering::Release);
        self.until_ms.store(until_ms, Ordering::Release);
    }

    /// Clears any blocking state (e.g. after the user updates credentials).
    pub fn reset(&self) {
        self.state
            .store(HealthState::Unknown as u8, Ordering::Release);
        self.until_ms.store(0, Ordering::Release);
        self.consecutive_failures.store(0, Ordering::Release);
    }

    /// RAII in-flight counter.
    pub fn begin(self: &Arc<Self>) -> InFlight {
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        InFlight(self.clone())
    }

    pub fn snapshot(&self) -> HealthSnapshot {
        let nonzero = |v: i64| (v != 0).then_some(v);
        HealthSnapshot {
            state: self.state(),
            until_ms: nonzero(self.until_ms.load(Ordering::Acquire)),
            consecutive_failures: self.consecutive_failures.load(Ordering::Acquire),
            in_flight: self.in_flight.load(Ordering::Acquire),
            successes: self.successes.load(Ordering::Relaxed),
            failures: self.failures.load(Ordering::Relaxed),
            last_success_ms: nonzero(self.last_success_ms.load(Ordering::Relaxed)),
            last_failure_ms: nonzero(self.last_failure_ms.load(Ordering::Relaxed)),
            last_error: self.last_error.lock().ok().and_then(|e| e.clone()),
        }
    }
}

#[derive(Debug)]
pub struct InFlight(Arc<TargetHealth>);

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Health state per target id, surviving route-table rebuilds.
#[derive(Debug, Default)]
pub struct HealthRegistry {
    entries: RwLock<HashMap<TargetId, Arc<TargetHealth>>>,
}

impl HealthRegistry {
    pub fn get_or_create(&self, id: &TargetId) -> Arc<TargetHealth> {
        if let Some(h) = self.entries.read().ok().and_then(|m| m.get(id).cloned()) {
            return h;
        }
        let mut map = self.entries.write().unwrap_or_else(|e| e.into_inner());
        map.entry(id.clone()).or_default().clone()
    }

    pub fn get(&self, id: &str) -> Option<Arc<TargetHealth>> {
        self.entries.read().ok()?.get(id).cloned()
    }

    /// Resets every target whose id starts with one of `account_model_ids` (credential update).
    pub fn reset_where(&self, mut predicate: impl FnMut(&str) -> bool) {
        if let Ok(map) = self.entries.read() {
            for (id, h) in map.iter() {
                if predicate(id) {
                    h.reset();
                }
            }
        }
    }

    /// Drops entries for targets that no longer exist.
    pub fn retain(&self, mut keep: impl FnMut(&str) -> bool) {
        if let Ok(mut map) = self.entries.write() {
            map.retain(|id, _| keep(id));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_errors_back_off_exponentially_then_half_open() {
        let h = TargetHealth::default();
        assert!(h.is_available(0));
        h.record_failure(FailureKind::Server, "boom", 1_000);
        assert_eq!(h.state(), HealthState::Cooldown);
        assert!(!h.is_available(5_999));
        assert!(h.is_available(6_000), "5 s cooldown");
        h.record_failure(FailureKind::Server, "boom", 6_000);
        assert!(!h.is_available(15_999));
        assert!(h.is_available(16_000), "10 s after the second failure");
        h.record_success(16_000);
        assert_eq!(h.state(), HealthState::Healthy);
        assert_eq!(h.snapshot().consecutive_failures, 0);
    }

    #[test]
    fn rate_limit_honours_retry_after_and_quota_waits() {
        let h = TargetHealth::default();
        h.record_failure(
            FailureKind::RateLimited {
                retry_after: Some(Duration::from_secs(2)),
            },
            "slow",
            0,
        );
        assert!(!h.is_available(1_999));
        assert!(h.is_available(2_000));

        let q = TargetHealth::default();
        q.record_failure(
            FailureKind::QuotaExhausted { reset_after: None },
            "quota",
            0,
        );
        assert_eq!(q.state(), HealthState::QuotaExhausted);
        assert!(!q.is_available(3_599_999));
        q.mark_quota_exhausted(10);
        assert!(q.is_available(10));
    }

    #[test]
    fn invalid_requests_do_not_trip_the_breaker() {
        let h = TargetHealth::default();
        h.record_failure(FailureKind::InvalidRequest, "bad", 0);
        assert_eq!(h.state(), HealthState::Unknown);
        assert!(h.is_available(0));
    }

    #[test]
    fn auth_failure_blocks_until_reset() {
        let h = TargetHealth::default();
        h.record_failure(FailureKind::Auth, "401", 0);
        assert!(!h.is_available(60_000));
        h.reset();
        assert!(h.is_available(60_000));
        assert_eq!(h.snapshot().last_error.as_deref(), Some("401"));
    }

    #[test]
    fn in_flight_guard_counts() {
        let h = Arc::new(TargetHealth::default());
        let a = h.begin();
        let b = h.begin();
        assert_eq!(h.snapshot().in_flight, 2);
        drop(a);
        drop(b);
        assert_eq!(h.snapshot().in_flight, 0);
    }

    #[test]
    fn registry_keeps_state_across_lookups() {
        let r = HealthRegistry::default();
        let id: TargetId = Arc::from("m1:chat");
        r.get_or_create(&id)
            .record_failure(FailureKind::Server, "x", 0);
        assert_eq!(r.get_or_create(&id).state(), HealthState::Cooldown);
        r.reset_where(|t| t.starts_with("m1:"));
        assert_eq!(
            r.get("m1:chat").expect("entry").state(),
            HealthState::Unknown
        );
        r.retain(|_| false);
        assert!(r.get("m1:chat").is_none());
    }
}
